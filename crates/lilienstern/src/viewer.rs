//! A map viewer for a run: `map.geojson` (the last day), `style.json` (a MapLibre style with the
//! basemap and the game's layers) and `index.html`, a page that replays the game over the map
//! (open it in a browser; it loads MapLibre GL JS from unpkg and the basemap from the web).

use std::path::Path;

use serde_json::{json, Value};

use crate::geojson::to_geojson;
use crate::world::World;

const MAPLIBRE: &str = "4.7.1";
const OSM_CREDIT: &str =
    "© <a href=\"https://www.openstreetmap.org/copyright\">OpenStreetMap</a> contributors";

/// What to draw under the game: a style object, or a style URL the page loads.
enum Basemap {
    Style(Value),
    Url(String),
}

fn blank() -> Value {
    json!({ "version": 8, "sources": {}, "layers": [
        { "id": "background", "type": "background", "paint": { "background-color": "#f3f0e8" } }
    ] })
}

fn osm() -> Value {
    json!({ "version": 8,
        "sources": { "osm": { "type": "raster", "tiles": ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"], "tileSize": 256, "maxzoom": 19, "attribution": OSM_CREDIT } },
        "layers": [ { "id": "osm", "type": "raster", "source": "osm", "paint": { "raster-saturation": -0.6, "raster-opacity": 0.85 } } ] })
}

fn basemap(w: &World) -> Result<Basemap, String> {
    let choice = w
        .game
        .city
        .basemap
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    Ok(match choice {
        None => match &w.city.style {
            Some(s) => Basemap::Style(s.clone()),
            None if w.city.imported => Basemap::Style(osm()),
            None => Basemap::Style(blank()),
        },
        Some("none") => Basemap::Style(blank()),
        Some("osm") => Basemap::Style(osm()),
        Some(url) if url.contains("://") => Basemap::Url(url.to_string()),
        Some(file) => {
            let path = w.game.dir.join(file);
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("basemap {}: {e}", path.display()))?;
            Basemap::Style(
                serde_json::from_str(&text)
                    .map_err(|e| format!("basemap {}: {e}", path.display()))?,
            )
        }
    })
}

/// The game's source and layers, added on top of a style.
fn add_game(style: &mut Value, data: Value) {
    style["sources"]["lilienstern"] = json!({ "type": "geojson", "data": data });
    let layers = style["layers"].as_array_mut().expect("a style has layers");
    layers.push(
        json!({ "id": "lilienstern-fill", "type": "fill", "source": "lilienstern",
        "paint": { "fill-color": ["get", "fill"], "fill-opacity": ["get", "fill-opacity"] } }),
    );
    layers.push(
        json!({ "id": "lilienstern-line", "type": "line", "source": "lilienstern",
        "paint": { "line-color": "#333333", "line-width": 0.6, "line-opacity": 0.6 } }),
    );
}

/// The MapLibre style for `style.json`: the basemap (OpenStreetMap when it is a URL, which only
/// the page can merge) with the game's layers, reading `map.geojson`.
pub fn style(w: &World) -> Result<Value, String> {
    let mut s = match basemap(w)? {
        Basemap::Style(s) => s,
        Basemap::Url(_) => osm(),
    };
    if !s["layers"].is_array() {
        s["layers"] = json!([]);
    }
    add_game(&mut s, json!("map.geojson"));
    Ok(s)
}

fn bounds(w: &World) -> [[f64; 2]; 2] {
    let mut b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in &w.city.places {
        for &pt in &p.ring {
            let ll = w.city.lonlat(pt);
            b[0] = [b[0][0].min(ll[0]), b[0][1].min(ll[1])];
            b[1] = [b[1][0].max(ll[0]), b[1][1].max(ll[1])];
        }
    }
    b
}

/// Values as short JSON numbers: whole numbers when `coarse`, else one decimal; zero as `0`.
fn compact(v: &[f32], coarse: bool) -> Value {
    Value::Array(
        v.iter()
            .map(|&x| {
                let x = f64::from(x);
                if coarse || x == 0.0 {
                    json!(x.round() as i64)
                } else {
                    json!((x * 10.0).round() / 10.0)
                }
            })
            .collect(),
    )
}

/// Everything the page needs, as one JSON object.
fn page_data(w: &World) -> Result<Value, String> {
    let g = w.game;
    let geojson = to_geojson(w);
    let (style, url) = match basemap(w)? {
        Basemap::Style(mut s) => {
            if !s["layers"].is_array() {
                s["layers"] = json!([]);
            }
            // The page puts the GeoJSON in, so it is not sent twice.
            add_game(
                &mut s,
                json!({ "type": "FeatureCollection", "features": [] }),
            );
            (s, Value::Null)
        }
        Basemap::Url(u) => (Value::Null, json!(u)),
    };
    let frames: Vec<crate::world::Frame> = match &w.history {
        Some(h) if !h.is_empty() => h.clone(),
        _ => Vec::new(),
    };
    // Maps that never change are sent once.
    let maps: Vec<Value> = g
        .rules
        .maps
        .iter()
        .enumerate()
        .map(|(m, def)| {
            let values: Vec<&Vec<f32>> = frames.iter().map(|f| &f.maps[m]).collect();
            let fixed = values.windows(2).all(|p| p[0] == p[1]);
            let max = values
                .iter()
                .flat_map(|v| v.iter())
                .fold(0f32, |a, &b| a.max(b));
            json!({ "id": def.id, "max": max, "fixed": fixed })
        })
        .collect();
    let fixed: Vec<bool> = maps
        .iter()
        .map(|m| m["fixed"].as_bool().unwrap_or(false))
        .collect();
    // Maps that reach past 20 are sent as whole numbers, the others to one decimal.
    let coarse: Vec<bool> = maps
        .iter()
        .map(|m| m["max"].as_f64().unwrap_or(0.0) > 20.0)
        .collect();
    let frames: Vec<Value> = frames
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let m: Vec<Value> = f
                .maps
                .iter()
                .enumerate()
                .map(|(k, v)| {
                    if fixed[k] && i > 0 {
                        Value::Null
                    } else {
                        compact(v, coarse[k])
                    }
                })
                .collect();
            json!({ "day": f.day, "owner": f.owner, "territory": f.territory, "maps": m })
        })
        .collect();
    let factions: Vec<Value> = g
        .factions
        .iter()
        .enumerate()
        .map(|(i, f)| json!({ "id": f.id, "name": f.name, "color": f.color, "holds": w.gangs.contains(&i) }))
        .collect();
    let credit = [
        Some(format!("Lilienstern · {}", g.name)),
        w.city.attribution.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    Ok(json!({
        "title": format!("{} · seed {}", g.name, w.seed),
        "credit": credit,
        "style": style,
        "basemapUrl": url,
        "geojson": geojson,
        "bounds": bounds(w),
        "factions": factions,
        "maps": maps,
        "frames": frames,
        "violence": g.rules.maps[g.violence_map].id,
    }))
}

/// The viewer's files: `index.html`, `style.json` and `map.geojson`, with their contents.
pub fn files(w: &World) -> Result<Vec<(String, String)>, String> {
    let style = serde_json::to_string_pretty(&style(w)?).map_err(|e| e.to_string())?;
    let data = page_data(w)?.to_string().replace("</", "<\\/");
    let html = PAGE
        .replace("{{MAPLIBRE}}", MAPLIBRE)
        .replace("{{TITLE}}", &html_escape(&w.game.name))
        .replace("{{DATA}}", &data);
    Ok(vec![
        ("index.html".into(), html),
        ("style.json".into(), style),
        ("map.geojson".into(), to_geojson(w).to_string()),
    ])
}

/// Writes the viewer's files into `dir`.
pub fn write(dir: &Path, w: &World) -> Result<(), String> {
    write_files(dir, &files(w)?)
}

pub fn write_files(dir: &Path, files: &[(String, String)]) -> Result<(), String> {
    let io = |e: std::io::Error| format!("{}: {e}", dir.display());
    std::fs::create_dir_all(dir).map_err(io)?;
    for (name, text) in files {
        std::fs::write(dir.join(name), text).map_err(io)?;
    }
    Ok(())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

const PAGE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{{TITLE}}</title>
<link rel="stylesheet" href="https://unpkg.com/maplibre-gl@{{MAPLIBRE}}/dist/maplibre-gl.css">
<script src="https://unpkg.com/maplibre-gl@{{MAPLIBRE}}/dist/maplibre-gl.js"></script>
<style>
  html, body, #map { margin: 0; height: 100%; }
  body { font: 13px/1.4 system-ui, sans-serif; color: #222; }
  #panel { position: absolute; top: 10px; left: 10px; width: 270px; max-height: calc(100% - 40px); overflow: auto;
    background: rgba(255,255,255,.94); border-radius: 8px; box-shadow: 0 2px 10px rgba(0,0,0,.2); padding: 10px 12px; }
  h1 { font-size: 15px; margin: 0 0 6px; }
  .row { display: flex; align-items: center; gap: 6px; margin: 6px 0; }
  input[type=range] { flex: 1; }
  select, button { font: inherit; }
  #legend div { display: flex; align-items: center; gap: 6px; }
  #legend i { width: 12px; height: 12px; border-radius: 2px; display: inline-block; border: 1px solid #0003; }
  #legend .n { margin-left: auto; font-variant-numeric: tabular-nums; color: #555; }
  #ramp { height: 10px; border-radius: 3px; background: linear-gradient(90deg, #ffffcc, #fd8d3c, #bd0026); }
  #info { margin-top: 8px; border-top: 1px solid #ddd; padding-top: 6px; }
  #info b { display: block; }
  .muted { color: #666; }
</style>
</head>
<body>
<div id="map"></div>
<div id="panel">
  <h1 id="title"></h1>
  <div class="row"><button id="play">▶</button><input id="day" type="range" min="0" value="0"><span id="daylabel"></span></div>
  <div class="row">Show <select id="view"></select></div>
  <div id="legend"></div>
  <div id="info" class="muted">Hover over a place.</div>
</div>
<script type="application/json" id="data">{{DATA}}</script>
<script>
const D = JSON.parse(document.getElementById('data').textContent);
const geo = D.geojson;
const N = geo.features.length;
const $ = (id) => document.getElementById(id);
$('title').textContent = D.title;
const frames = D.frames.length ? D.frames : [{ day: null, owner: geo.features.map((f) => D.factions.findIndex((x) => x.id === f.properties.owner)), territory: [], maps: D.maps.map((m) => geo.features.map((f) => f.properties[m.id])) }];
const mapValues = (fi, m) => { for (let i = fi; i >= 0; i--) if (frames[i].maps[m]) return frames[i].maps[m]; return []; };
let frame = frames.length - 1, view = 'territory', timer = null;

const sel = $('view');
sel.add(new Option('Territory', 'territory'));
D.maps.forEach((m, i) => sel.add(new Option(m.id, String(i))));
const violence = D.maps.findIndex((m) => m.id === D.violence);
$('day').max = frames.length - 1;
$('day').value = frame;

function paint() {
  const f = frames[frame];
  const vals = view === 'territory' ? null : mapValues(frame, +view);
  geo.features.forEach((ft, i) => {
    const o = f.owner[i];
    const fac = o >= 0 ? D.factions[o] : null;
    ft.properties.fill = fac ? fac.color : '#d8d8d8';
    ft.properties['fill-opacity'] = fac ? 0.6 : 0.15;
    ft.properties.owner_name = fac ? fac.name : null;
    if (vals) ft.properties.v = vals[i] ?? 0;
  });
  const src = map.getSource('lilienstern');
  if (src) src.setData(geo);
  if (map.getLayer('lilienstern-fill')) {
    if (view === 'territory') {
      map.setPaintProperty('lilienstern-fill', 'fill-color', ['get', 'fill']);
      map.setPaintProperty('lilienstern-fill', 'fill-opacity', ['get', 'fill-opacity']);
    } else {
      const max = Math.max(D.maps[+view].max, 0.001);
      map.setPaintProperty('lilienstern-fill', 'fill-color', ['interpolate', ['linear'], ['get', 'v'], 0, '#ffffcc', max / 2, '#fd8d3c', max, '#bd0026']);
      map.setPaintProperty('lilienstern-fill', 'fill-opacity', ['interpolate', ['linear'], ['get', 'v'], 0, 0.1, max * 0.05, 0.65]);
    }
  }
  $('daylabel').textContent = f.day === null ? '' : `day ${f.day}`;
  legend();
}

function legend() {
  const f = frames[frame];
  if (view !== 'territory') {
    const m = D.maps[+view];
    $('legend').innerHTML = `<div id="ramp"></div><div><span>0</span><span class="n">${m.max.toFixed(1)}</span></div>`;
    return;
  }
  const counts = D.factions.map(() => 0);
  f.owner.forEach((o) => { if (o >= 0) counts[o]++; });
  $('legend').innerHTML = D.factions.filter((x) => x.holds).map((x) => {
    const i = D.factions.indexOf(x);
    return `<div><i style="background:${x.color}"></i>${x.name}<span class="n">${counts[i]}</span></div>`;
  }).join('') + `<div><i style="background:#d8d8d8"></i>nobody<span class="n">${f.owner.filter((o) => o < 0).length}</span></div>`;
}

function info(i) {
  if (i == null) { $('info').innerHTML = '<span class="muted">Hover over a place.</span>'; return; }
  const p = geo.features[i].properties;
  const o = frames[frame].owner[i];
  const rows = D.maps.map((m, k) => `${m.id}: ${(mapValues(frame, k)[i] ?? 0).toFixed(1)}`).join('<br>');
  $('info').innerHTML = `<b>${p.name}</b>${o >= 0 ? 'held by ' + D.factions[o].name : '<span class="muted">nobody holds it</span>'}<br>${rows}`;
}

if (D.style) D.style.sources.lilienstern.data = geo;
const style = D.style || D.basemapUrl;
const map = new maplibregl.Map({ container: 'map', style, bounds: D.bounds, fitBoundsOptions: { padding: 40 }, attributionControl: false });
map.addControl(new maplibregl.AttributionControl({ compact: true, customAttribution: D.credit }));
map.addControl(new maplibregl.NavigationControl(), 'top-right');
map.on('load', () => {
  if (!map.getSource('lilienstern')) {
    map.addSource('lilienstern', { type: 'geojson', data: geo });
    map.addLayer({ id: 'lilienstern-fill', type: 'fill', source: 'lilienstern', paint: { 'fill-color': ['get', 'fill'], 'fill-opacity': ['get', 'fill-opacity'] } });
    map.addLayer({ id: 'lilienstern-line', type: 'line', source: 'lilienstern', paint: { 'line-color': '#333', 'line-width': 0.6, 'line-opacity': 0.6 } });
  }
  paint();
  let hovered = null;
  map.on('mousemove', 'lilienstern-fill', (e) => {
    const id = geo.features.findIndex((f) => f.properties.id === e.features[0].properties.id);
    if (id !== hovered) { hovered = id; info(id); }
  });
  map.on('mouseleave', 'lilienstern-fill', () => { hovered = null; info(null); });
});
$('day').oninput = (e) => { frame = +e.target.value; paint(); };
sel.onchange = (e) => { view = e.target.value; paint(); };
$('play').onclick = () => {
  if (timer) { clearInterval(timer); timer = null; $('play').textContent = '▶'; return; }
  if (frame >= frames.length - 1) frame = 0;
  $('play').textContent = '❚❚';
  timer = setInterval(() => {
    frame++; $('day').value = frame; paint();
    if (frame >= frames.length - 1) { clearInterval(timer); timer = null; $('play').textContent = '▶'; }
  }, 120);
};
if (violence < 0) sel.value = 'territory';
</script>
</body>
</html>
"##;
