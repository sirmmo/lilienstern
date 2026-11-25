//! The map: places (provinces, districts, neighbourhoods) and which ones border each other.
//!
//! A map is either generated (a round city of small hexagonal neighbourhoods, with a river,
//! bridges, a rich centre, rich enclaves and poor outskirts) or imported from GeoJSON polygons,
//! where polygons sharing a border are neighbours.
//!
//! A MapLibre style (`"version": 8`) can stand for the GeoJSON: its GeoJSON source holds the
//! places, and the style becomes the map drawn under the game.
//!
//! Generated fields are sampled from continuous fields of the city, not of its cells: the same
//! city cut into 150 or 2400 neighbourhoods has the same wealth and density at each point.
//! That is what makes granularity experiments possible.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::Deserialize;

use crate::rng::{fbm, Rng};

fn d_neighborhoods() -> u32 {
    600
}
fn d_radius() -> f64 {
    10.0
}
fn d_true() -> bool {
    true
}
fn d_bridges() -> u32 {
    5
}
fn d_enclaves() -> u32 {
    2
}
fn d_city_seed() -> i64 {
    -1
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CityDef {
    /// About how many neighbourhoods the generated city has.
    #[serde(default = "d_neighborhoods")]
    pub neighborhoods: u32,
    #[serde(default = "d_radius")]
    pub radius_km: f64,
    #[serde(default = "d_true")]
    pub river: bool,
    #[serde(default = "d_bridges")]
    pub bridges: u32,
    /// Rich districts away from the centre.
    #[serde(default = "d_enclaves")]
    pub enclaves: u32,
    /// The seed of the generated city; -1 uses the run's seed. Set it to compare runs on one
    /// city.
    #[serde(default = "d_city_seed")]
    pub city_seed: i64,
    /// [lon, lat] of the centre, for GeoJSON.
    #[serde(default)]
    pub origin: Option<[f64; 2]>,
    /// Import the places from this GeoJSON file (relative to the game) instead.
    #[serde(default)]
    pub geojson: Option<String>,
    /// Imported places: the property holding the name.
    #[serde(default)]
    pub name_property: Option<String>,
    /// When `geojson` is a MapLibre style: the GeoJSON source with the places (default: the
    /// first GeoJSON source).
    #[serde(default)]
    pub source: Option<String>,
    /// Imported places: fields taken from other properties, as `field = "property"`.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// Imported places without `wealth`, `density` or `population` get them from the same
    /// continuous fields as a generated city.
    #[serde(default = "d_true")]
    pub synthesize: bool,
    /// What the viewer draws under the game: "none", "osm" (OpenStreetMap tiles), or a MapLibre
    /// style (a URL, or a file relative to the game). Default: an imported MapLibre style
    /// itself, else "osm" for an imported map and "none" for a generated one.
    #[serde(default)]
    pub basemap: Option<String>,
    /// Imported places: extra borders, by place name (`[["Messina", "Reggio di Calabria"]]`):
    /// straits, sea routes.
    #[serde(default)]
    pub links: Vec<[String; 2]>,
    /// Imported places: join every group of places cut off from the largest (islands) to its
    /// nearest place in it, instead of dropping them.
    #[serde(default)]
    pub connect: bool,
}

impl CityDef {
    /// The file to import, if any (an empty string means none, so it can be a parameter).
    pub fn import_file(&self) -> Option<&str> {
        self.geojson.as_deref().filter(|s| !s.trim().is_empty())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub id: String,
    pub name: String,
    /// Metres east and north of the origin.
    pub centre: [f64; 2],
    /// Outline, in metres.
    pub ring: Vec<[f64; 2]>,
    /// Values maps can start from: `x`, `y` (-1 to 1), `centrality` (1 at the centre, 0 at the
    /// edge), `area_km2`, and for a generated city `wealth`, `density`, `population`,
    /// `elevation` and `forest` (0 to 1, for terrain); imported places add their numeric
    /// properties (and the generated ones they lack, with `synthesize`).
    pub fields: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct City {
    pub places: Vec<Place>,
    /// Neighbours of each place, in a fixed order.
    pub neighbours: Vec<Vec<usize>>,
    /// [lon, lat] of (0, 0).
    pub origin: [f64; 2],
    /// The MapLibre style the places came from (its GeoJSON sources inlined), if any.
    pub style: Option<serde_json::Value>,
    /// The style's source with the places.
    pub style_source: Option<String>,
    /// Whether the places were imported (and so have a place on Earth).
    pub imported: bool,
    /// Credit for imported data (a GeoJSON's top-level `attribution`).
    pub attribution: Option<String>,
}

const M_PER_DEG_LAT: f64 = 110_540.0;
const M_PER_DEG_LON: f64 = 111_320.0;

impl City {
    /// [lon, lat] of a point in metres.
    pub fn lonlat(&self, p: [f64; 2]) -> [f64; 2] {
        let k = (self.origin[1].to_radians()).cos().max(0.01);
        [
            self.origin[0] + p[0] / (M_PER_DEG_LON * k),
            self.origin[1] + p[1] / M_PER_DEG_LAT,
        ]
    }

    pub fn len(&self) -> usize {
        self.places.len()
    }

    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }

    /// The value of a field at a place (0 when it has none).
    pub fn field(&self, place: usize, name: &str) -> f64 {
        self.places[place].fields.get(name).copied().unwrap_or(0.0)
    }

    pub fn distance(&self, a: usize, b: usize) -> f64 {
        let (p, q) = (self.places[a].centre, self.places[b].centre);
        ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
    }

    /// Groups of connected places, largest first: the group of each place.
    fn components(&self) -> (Vec<usize>, usize) {
        let n = self.places.len();
        let mut comp = vec![usize::MAX; n];
        let mut sizes: Vec<usize> = Vec::new();
        for s in 0..n {
            if comp[s] != usize::MAX {
                continue;
            }
            let c = sizes.len();
            comp[s] = c;
            let mut stack = vec![s];
            let mut size = 0;
            while let Some(p) = stack.pop() {
                size += 1;
                for &q in &self.neighbours[p] {
                    if comp[q] == usize::MAX {
                        comp[q] = c;
                        stack.push(q);
                    }
                }
            }
            sizes.push(size);
        }
        let best = (0..sizes.len())
            .max_by_key(|&c| (sizes[c], usize::MAX - c))
            .unwrap_or(0);
        (comp, best)
    }

    /// Joins each group of places cut off from the largest to it, by a border between the
    /// closest pair of places (centre to centre): a sea crossing.
    fn connect_islands(&mut self) {
        loop {
            let (comp, main) = self.components();
            let Some(other) = (0..comp.len()).map(|p| comp[p]).find(|&c| c != main) else {
                return;
            };
            let mut best: Option<(f64, usize, usize)> = None;
            for a in (0..comp.len()).filter(|&p| comp[p] == other) {
                for b in (0..comp.len()).filter(|&p| comp[p] == main) {
                    let d = self.distance(a, b);
                    if best.map_or(true, |(bd, _, _)| d < bd) {
                        best = Some((d, a, b));
                    }
                }
            }
            let Some((_, a, b)) = best else { return };
            self.neighbours[a].push(b);
            self.neighbours[b].push(a);
        }
    }

    /// Keeps the largest group of connected places.
    fn keep_largest_component(&mut self) {
        let n = self.places.len();
        let mut comp = vec![usize::MAX; n];
        let mut sizes = Vec::new();
        for s in 0..n {
            if comp[s] != usize::MAX {
                continue;
            }
            let c = sizes.len();
            let mut stack = vec![s];
            comp[s] = c;
            let mut size = 0;
            while let Some(p) = stack.pop() {
                size += 1;
                for &q in &self.neighbours[p] {
                    if comp[q] == usize::MAX {
                        comp[q] = c;
                        stack.push(q);
                    }
                }
            }
            sizes.push(size);
        }
        let Some(best) = (0..sizes.len()).max_by_key(|&c| (sizes[c], usize::MAX - c)) else {
            return;
        };
        if sizes[best] == n {
            return;
        }
        let mut remap = vec![usize::MAX; n];
        let mut places = Vec::new();
        for (i, p) in self.places.iter().enumerate() {
            if comp[i] == best {
                remap[i] = places.len();
                places.push(p.clone());
            }
        }
        let neighbours = (0..n)
            .filter(|&i| comp[i] == best)
            .map(|i| self.neighbours[i].iter().map(|&q| remap[q]).collect())
            .collect();
        self.places = places;
        self.neighbours = neighbours;
    }
}

const PREFIXES: [&str; 12] = [
    "Borgo ",
    "Porta ",
    "San ",
    "Santa ",
    "Villa ",
    "Ponte ",
    "Corte ",
    "Prati di ",
    "Case ",
    "Croce di ",
    "",
    "",
];
const SYLLABLES: [&str; 24] = [
    "ma", "ri", "lo", "sa", "ve", "ta", "ne", "ga", "bo", "ci", "ro", "la", "mi", "to", "ce", "na",
    "sel", "van", "mar", "bran", "tor", "gal", "len", "del",
];

fn name(rng: &mut Rng) -> String {
    let mut root = String::new();
    for _ in 0..2 + rng.below(2) {
        root.push_str(SYLLABLES[rng.below(SYLLABLES.len())]);
    }
    let mut c = root.chars();
    let root = c
        .next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default();
    format!("{}{}", PREFIXES[rng.below(PREFIXES.len())], root)
}

fn enclaves(seed: u64, n: u32, radius: f64) -> Vec<[f64; 2]> {
    let mut rng = Rng::new(seed ^ 0xC17E);
    (0..n)
        .map(|_| {
            [
                (rng.f64() * 2.0 - 1.0) * 0.6 * radius,
                (rng.f64() * 2.0 - 1.0) * 0.6 * radius,
            ]
        })
        .collect()
}

/// Wealth (2 to 100) and density (people per km²) at a point of a city of this radius: richer
/// and denser towards the centre and around the enclaves, with noise.
fn synthetic(x: f64, y: f64, radius: f64, seed: u64, enclaves: &[[f64; 2]]) -> (f64, f64) {
    let c = (1.0 - (x * x + y * y).sqrt() / radius).max(0.0);
    let mut wealth = 12.0 + 50.0 * c * c + (fbm(seed, x / 3000.0, y / 3000.0) - 0.5) * 45.0;
    for e in enclaves {
        let dd = ((x - e[0]).powi(2) + (y - e[1]).powi(2)).sqrt() / (0.1 * radius);
        wealth += 40.0 / (1.0 + dd * dd);
    }
    let density =
        (3000.0 + 15000.0 * c + (fbm(seed ^ 0xD3, x / 2500.0, y / 2500.0) - 0.5) * 9000.0)
            .max(500.0);
    (wealth.clamp(2.0, 100.0), density)
}

/// Elevation and forest cover (0 to 1) at a point of a map of this radius: smooth fields for
/// terrain, independent of how the map is cut into places.
fn landscape(x: f64, y: f64, radius: f64, seed: u64) -> (f64, f64) {
    let k = radius.max(1.0) / 3.0;
    let elevation = fbm(seed ^ 0xE1E7, x / k, y / k);
    let forest = fbm(seed ^ 0xF0E5, x / (0.6 * k), y / (0.6 * k));
    (elevation.clamp(0.0, 1.0), forest.clamp(0.0, 1.0))
}

/// Generates a round city of about `def.neighborhoods` hexagonal neighbourhoods.
pub fn generate(def: &CityDef, seed: u64) -> City {
    let seed = if def.city_seed >= 0 {
        def.city_seed as u64
    } else {
        seed
    };
    let radius = def.radius_km * 1000.0;
    let n = f64::from(def.neighborhoods.max(7));
    let sq3 = 3f64.sqrt();
    let cell_area = std::f64::consts::PI * radius * radius / n;
    let s = (2.0 * cell_area / (3.0 * sq3)).sqrt(); // centre to corner
    let width = s * sq3;
    let enclaves = enclaves(seed, def.enclaves, radius);
    let river_y = |x: f64| {
        let u = x / radius;
        radius * (0.1 + 0.15 * u - 0.3 * u * u)
    };
    let river_w = 350f64.max(width * 1.1);
    let bridges: Vec<f64> = (0..def.bridges)
        .map(|k| radius * (-0.8 + 1.6 * (f64::from(k) + 0.5) / f64::from(def.bridges)))
        .collect();
    let corners = [
        [sq3 / 2.0, 0.5],
        [0.0, 1.0],
        [-sq3 / 2.0, 0.5],
        [-sq3 / 2.0, -0.5],
        [0.0, -1.0],
        [sq3 / 2.0, -0.5],
    ];
    let rmax = (radius / (1.5 * s)).ceil() as i64 + 1;
    let qmax = (radius / width).ceil() as i64 + rmax;
    let mut places = Vec::new();
    let mut index: HashMap<(i64, i64), usize> = HashMap::new();
    let mut names = Rng::new(seed ^ 0xA11E);
    for r in -rmax..=rmax {
        for q in -qmax..=qmax {
            let x = width * (q as f64 + r as f64 / 2.0);
            let y = 1.5 * s * r as f64;
            let d = (x * x + y * y).sqrt();
            if d > radius {
                continue;
            }
            if def.river
                && (y - river_y(x)).abs() < river_w / 2.0
                && !bridges.iter().any(|&b| (x - b).abs() < width * 0.6)
            {
                continue;
            }
            let c = 1.0 - d / radius;
            let (wealth, density) = synthetic(x, y, radius, seed, &enclaves);
            let area_km2 = 1.5 * sq3 * s * s / 1e6;
            let (elevation, forest) = landscape(x, y, radius, seed);
            let fields = BTreeMap::from([
                ("elevation".to_string(), elevation),
                ("forest".to_string(), forest),
                ("x".to_string(), x / radius),
                ("y".to_string(), y / radius),
                ("centrality".to_string(), c),
                ("wealth".to_string(), wealth),
                ("density".to_string(), density),
                ("population".to_string(), density * area_km2),
                ("area_km2".to_string(), area_km2),
            ]);
            index.insert((q, r), places.len());
            places.push(Place {
                id: format!("n{:04}", places.len()),
                name: name(&mut names),
                centre: [x, y],
                ring: corners
                    .iter()
                    .map(|k| [x + k[0] * s, y + k[1] * s])
                    .collect(),
                fields,
            });
        }
    }
    let mut coords = vec![(0i64, 0i64); places.len()];
    for (&k, &i) in &index {
        coords[i] = k;
    }
    let dirs = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];
    let neighbours = coords
        .iter()
        .map(|&(q, r)| {
            dirs.iter()
                .filter_map(|&(dq, dr)| index.get(&(q + dq, r + dr)).copied())
                .collect()
        })
        .collect();
    let mut city = City {
        places,
        neighbours,
        origin: def.origin.unwrap_or([0.0, 0.0]),
        style: None,
        style_source: None,
        imported: false,
        attribution: None,
    };
    city.keep_largest_component();
    for (i, p) in city.places.iter_mut().enumerate() {
        p.id = format!("n{i:04}");
    }
    city
}

fn ring_area_centroid(ring: &[[f64; 2]]) -> (f64, [f64; 2]) {
    let mut a = 0.0;
    let (mut cx, mut cy) = (0.0, 0.0);
    for i in 0..ring.len() {
        let p = ring[i];
        let q = ring[(i + 1) % ring.len()];
        let cross = p[0] * q[1] - q[0] * p[1];
        a += cross;
        cx += (p[0] + q[0]) * cross;
        cy += (p[1] + q[1]) * cross;
    }
    if a.abs() < 1e-9 {
        let n = ring.len().max(1) as f64;
        return (
            0.0,
            [
                ring.iter().map(|p| p[0]).sum::<f64>() / n,
                ring.iter().map(|p| p[1]).sum::<f64>() / n,
            ],
        );
    }
    (a.abs() / 2.0, [cx / (3.0 * a), cy / (3.0 * a)])
}

fn outer_rings(geom: &serde_json::Value) -> Vec<Vec<[f64; 2]>> {
    let ring = |r: &serde_json::Value| -> Vec<[f64; 2]> {
        r.as_array()
            .map(|pts| {
                pts.iter()
                    .filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]))
                    .collect()
            })
            .unwrap_or_default()
    };
    let coords = &geom["coordinates"];
    match geom["type"].as_str() {
        Some("Polygon") => coords.get(0).map(ring).into_iter().collect(),
        Some("MultiPolygon") => coords
            .as_array()
            .map(|ps| ps.iter().filter_map(|p| p.get(0).map(ring)).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn is_style(json: &serde_json::Value) -> bool {
    json.get("version").is_some() && json["sources"].is_object()
}

/// Loads places from a GeoJSON file or a MapLibre style file. In a style, GeoJSON sources whose
/// `data` is a file name are read relative to the style and inlined; sources on the web are
/// left as they are, except the one with the places, which must be local or inline.
pub fn load(path: &Path, def: &CityDef, seed: u64) -> Result<City, String> {
    let read = |name: &str| std::fs::read_to_string(name).map_err(|e| format!("{name}: {e}"));
    load_with(&path.to_string_lossy(), &read, def, seed)
}

/// [`load`], reading files with `read` (paths are `/` separated).
pub fn load_with(
    name: &str,
    read: &dyn Fn(&str) -> Result<String, String>,
    def: &CityDef,
    seed: u64,
) -> Result<City, String> {
    let json: serde_json::Value =
        serde_json::from_str(&read(name)?).map_err(|e| format!("{name}: not JSON: {e}"))?;
    if !is_style(&json) {
        return from_value(&json, def, seed).map_err(|e| format!("{name}: {e}"));
    }
    let mut style = json;
    let sources = style["sources"].as_object_mut().expect("checked");
    for src in sources.values_mut() {
        if src["type"] == "geojson" {
            if let Some(file) = src["data"].as_str().filter(|f| !f.contains("://")) {
                let text = read(&crate::game::sibling(name, file))?;
                src["data"] =
                    serde_json::from_str(&text).map_err(|e| format!("{file}: not JSON: {e}"))?;
            }
        }
    }
    let source = match &def.source {
        Some(n) => n.clone(),
        None => sources
            .iter()
            .find(|(_, s)| s["type"] == "geojson")
            .map(|(k, _)| k.clone())
            .ok_or_else(|| format!("{name}: the style has no GeoJSON source"))?,
    };
    let data = sources
        .get(&source)
        .filter(|s| s["type"] == "geojson")
        .map(|s| s["data"].clone())
        .ok_or_else(|| format!("{name}: no GeoJSON source {source:?}"))?;
    if data.is_string() {
        return Err(format!(
            "{name}: source {source:?} is on the web ({}); download it and give its file name",
            data.as_str().unwrap_or_default()
        ));
    }
    let mut city = from_value(&data, def, seed).map_err(|e| format!("{name}: {e}"))?;
    city.style = Some(style);
    city.style_source = Some(source);
    Ok(city)
}

/// Imports places from GeoJSON text (see [`from_value`]).
pub fn from_geojson(text: &str, def: &CityDef) -> Result<City, String> {
    let json: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    from_value(&json, def, 0)
}

/// Imports places from a GeoJSON FeatureCollection of polygons. Polygons sharing at least two
/// vertices are neighbours; a place sharing none is joined to its two nearest places.
pub fn from_value(json: &serde_json::Value, def: &CityDef, seed: u64) -> Result<City, String> {
    let features = json["features"]
        .as_array()
        .ok_or("not a GeoJSON FeatureCollection")?;
    let mut shapes = Vec::new();
    for (i, f) in features.iter().enumerate() {
        let mut rings = outer_rings(&f["geometry"]);
        rings.retain(|r| r.len() >= 3);
        if !rings.is_empty() {
            shapes.push((i, f, rings));
        }
    }
    if shapes.len() < 2 {
        return Err("the GeoJSON has fewer than two polygons".into());
    }
    let origin = def.origin.unwrap_or_else(|| {
        let pts: Vec<[f64; 2]> = shapes
            .iter()
            .flat_map(|s| s.2.iter().flatten().copied())
            .collect();
        let n = pts.len() as f64;
        [
            pts.iter().map(|p| p[0]).sum::<f64>() / n,
            pts.iter().map(|p| p[1]).sum::<f64>() / n,
        ]
    });
    let k = origin[1].to_radians().cos().max(0.01);
    let project = |p: [f64; 2]| {
        [
            (p[0] - origin[0]) * M_PER_DEG_LON * k,
            (p[1] - origin[1]) * M_PER_DEG_LAT,
        ]
    };
    let name_key = def.name_property.clone().unwrap_or_else(|| "name".into());
    let mut places = Vec::new();
    for (i, f, rings) in &shapes {
        let projected: Vec<Vec<[f64; 2]>> = rings
            .iter()
            .map(|r| r.iter().map(|&p| project(p)).collect())
            .collect();
        let (area, centre, ring) = projected
            .iter()
            .map(|r| {
                let (a, c) = ring_area_centroid(r);
                (a, c, r.clone())
            })
            .fold((0.0, [0.0, 0.0], Vec::new()), |best, x| {
                if x.0 > best.0 || best.2.is_empty() {
                    x
                } else {
                    best
                }
            });
        let total_area: f64 = projected.iter().map(|r| ring_area_centroid(r).0).sum();
        let props = f["properties"].as_object();
        let mut fields = BTreeMap::new();
        if let Some(props) = props {
            for (k, v) in props {
                if let Some(x) = v.as_f64() {
                    fields.insert(k.to_lowercase(), x);
                }
            }
        }
        fields.insert("area_km2".into(), total_area.max(area) / 1e6);
        for (field, property) in &def.fields {
            match props.and_then(|p| p.get(property)).and_then(|v| v.as_f64()) {
                Some(x) => {
                    fields.insert(field.clone(), x);
                }
                None => {
                    return Err(format!(
                        "feature {i}: no numeric property {property:?} for field {field:?}"
                    ))
                }
            }
        }
        let prop = |k: &str| {
            props.and_then(|p| p.get(k)).and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })
        };
        places.push(Place {
            id: prop("id")
                .or_else(|| f["id"].as_str().map(str::to_string))
                .or_else(|| f["id"].as_i64().map(|n| n.to_string()))
                .unwrap_or_else(|| format!("p{i}")),
            name: prop(&name_key).unwrap_or_else(|| format!("Place {}", places.len() + 1)),
            centre,
            ring,
            fields,
        });
    }
    let far = places
        .iter()
        .map(|p| (p.centre[0].powi(2) + p.centre[1].powi(2)).sqrt())
        .fold(1.0f64, f64::max);
    let enclaves = enclaves(seed, def.enclaves, far);
    for p in &mut places {
        let d = (p.centre[0].powi(2) + p.centre[1].powi(2)).sqrt();
        p.fields.insert("x".into(), p.centre[0] / far);
        p.fields.insert("y".into(), p.centre[1] / far);
        p.fields.insert("centrality".into(), 1.0 - d / far);
        let area = p.fields["area_km2"].max(1e-6);
        if def.synthesize {
            let (wealth, density) = synthetic(p.centre[0], p.centre[1], far, seed, &enclaves);
            p.fields.entry("wealth".into()).or_insert(wealth);
            let (elevation, forest) = landscape(p.centre[0], p.centre[1], far, seed);
            p.fields.entry("elevation".into()).or_insert(elevation);
            p.fields.entry("forest".into()).or_insert(forest);
            if !p.fields.contains_key("density") && !p.fields.contains_key("population") {
                p.fields.insert("density".into(), density);
            }
        }
        match (
            p.fields.get("population").copied(),
            p.fields.get("density").copied(),
        ) {
            (Some(pop), None) => {
                p.fields.insert("density".into(), pop / area);
            }
            (None, Some(den)) => {
                p.fields.insert("population".into(), den * area);
            }
            _ => {}
        }
    }
    // Shared vertices, rounded to about 10 cm.
    let mut by_vertex: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (pi, (_, _, rings)) in shapes.iter().enumerate() {
        let mut mine: Vec<(i64, i64)> = rings
            .iter()
            .flatten()
            .map(|p| ((p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64))
            .collect();
        mine.sort_unstable();
        mine.dedup();
        for v in mine {
            by_vertex.entry(v).or_default().push(pi);
        }
    }
    let mut shared: BTreeMap<(usize, usize), u32> = BTreeMap::new();
    for ps in by_vertex.values() {
        for a in 0..ps.len() {
            for b in a + 1..ps.len() {
                *shared
                    .entry((ps[a].min(ps[b]), ps[a].max(ps[b])))
                    .or_default() += 1;
            }
        }
    }
    let mut neighbours = vec![Vec::new(); places.len()];
    for (&(a, b), &count) in &shared {
        if count >= 2 {
            neighbours[a].push(b);
            neighbours[b].push(a);
        }
    }
    let mut city = City {
        places,
        neighbours,
        origin,
        style: None,
        style_source: None,
        imported: true,
        attribution: json["attribution"].as_str().map(str::to_string),
    };
    // The map's own crossings (bridges, tunnels, ferries): `"links": [[a, b], ...]` beside the
    // features, by place id or name.
    if let Some(links) = json["links"].as_array() {
        for link in links {
            let pair = link.as_array().filter(|l| l.len() == 2);
            let ends: Option<Vec<usize>> = pair.map(|l| {
                l.iter()
                    .filter_map(|v| {
                        let key = v
                            .as_str()
                            .map(str::to_string)
                            .or_else(|| v.as_i64().map(|n| n.to_string()))?;
                        city.places
                            .iter()
                            .position(|p| p.id == key)
                            .or_else(|| city.places.iter().position(|p| p.name == key))
                    })
                    .collect()
            });
            match ends.as_deref() {
                Some(&[a, b]) if a != b => {
                    city.neighbours[a].push(b);
                    city.neighbours[b].push(a);
                }
                _ => return Err(format!("links: {link} does not name two places of the map")),
            }
        }
    }
    for i in 0..city.len() {
        if city.neighbours[i].is_empty() {
            let mut near: Vec<usize> = (0..city.len()).filter(|&j| j != i).collect();
            near.sort_by(|&a, &b| {
                city.distance(i, a)
                    .total_cmp(&city.distance(i, b))
                    .then(a.cmp(&b))
            });
            for &j in near.iter().take(2) {
                city.neighbours[i].push(j);
                city.neighbours[j].push(i);
            }
        }
    }
    for [a, b] in &def.links {
        let find = |name: &str| {
            city.places
                .iter()
                .position(|p| p.name.eq_ignore_ascii_case(name.trim()))
                .ok_or_else(|| format!("links: there is no place called {name:?}"))
        };
        let (a, b) = (find(a)?, find(b)?);
        city.neighbours[a].push(b);
        city.neighbours[b].push(a);
    }
    if def.connect {
        city.connect_islands();
    }
    for n in &mut city.neighbours {
        n.sort_unstable();
        n.dedup();
    }
    city.keep_largest_component();
    Ok(city)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(n: u32) -> CityDef {
        toml::from_str(&format!("neighborhoods = {n}")).unwrap()
    }

    #[test]
    fn generates_about_the_asked_number_of_connected_neighbourhoods() {
        for n in [150, 600, 2400] {
            let c = generate(&def(n), 1);
            let got = c.len() as f64 / f64::from(n);
            assert!((0.8..1.1).contains(&got), "{n}: {}", c.len());
            for (i, ns) in c.neighbours.iter().enumerate() {
                assert!(!ns.is_empty());
                for &j in ns {
                    assert!(c.neighbours[j].contains(&i));
                }
            }
        }
        assert_eq!(generate(&def(300), 5), generate(&def(300), 5));
        assert_ne!(
            generate(&def(300), 5).places[10].fields,
            generate(&def(300), 6).places[10].fields
        );
    }

    #[test]
    fn granularity_keeps_the_city() {
        let total = |c: &City| c.places.iter().map(|p| p.fields["population"]).sum::<f64>();
        let (a, b) = (generate(&def(200), 3), generate(&def(1600), 3));
        assert!(
            (total(&a) / total(&b) - 1.0).abs() < 0.08,
            "{} {}",
            total(&a),
            total(&b)
        );
    }

    #[test]
    fn imports_geojson_neighbours() {
        // Four unit squares in a 2x2 block, and one square far away.
        let sq = |x: f64, y: f64, name: &str| {
            format!(
                r#"{{"type":"Feature","properties":{{"name":"{name}","population":100}},"geometry":{{"type":"Polygon","coordinates":[[[{x},{y}],[{},{y}],[{},{}],[{x},{}],[{x},{y}]]]}}}}"#,
                x + 0.01,
                x + 0.01,
                y + 0.01,
                y + 0.01
            )
        };
        let text = format!(
            r#"{{"type":"FeatureCollection","features":[{},{},{},{},{}]}}"#,
            sq(11.0, 44.0, "A"),
            sq(11.01, 44.0, "B"),
            sq(11.0, 44.01, "C"),
            sq(11.01, 44.01, "D"),
            sq(11.5, 44.5, "E")
        );
        let c = from_geojson(&text, &def(1)).unwrap();
        assert_eq!(c.len(), 5);
        let a = c.places.iter().position(|p| p.name == "A").unwrap();
        let names: Vec<&str> = c.neighbours[a]
            .iter()
            .map(|&j| c.places[j].name.as_str())
            .collect();
        assert_eq!(names, vec!["B", "C"]); // D only touches at a corner
        let e = c.places.iter().position(|p| p.name == "E").unwrap();
        assert_eq!(c.neighbours[e].len(), 2); // joined to its nearest
        assert!(c.places[a].fields["density"] > 0.0);
        // A map's own links (a bridge to E) are borders, by id or name.
        let linked = text.replacen(r#""features""#, r#""links":[["A","E"]],"features""#, 1);
        let c = from_geojson(&linked, &def(1)).unwrap();
        let e = c.places.iter().position(|p| p.name == "E").unwrap();
        let a = c.places.iter().position(|p| p.name == "A").unwrap();
        assert_eq!(c.neighbours[e], vec![a]); // the bridge, not its nearest two
        let bad = text.replacen(r#""features""#, r#""links":[["A","Z"]],"features""#, 1);
        assert!(from_geojson(&bad, &def(1)).is_err());
        let ll = c.lonlat(c.places[a].centre);
        assert!(
            (ll[0] - 11.005).abs() < 1e-3 && (ll[1] - 44.005).abs() < 1e-3,
            "{ll:?}"
        );
    }

    #[test]
    fn imports_a_maplibre_style_and_fills_in_missing_fields() {
        let dir = std::env::temp_dir().join(format!("lilienstern-style-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sq = |x: f64, y: f64| {
            format!(
                r#"{{"type":"Feature","properties":{{"pop":{}}},"geometry":{{"type":"Polygon","coordinates":[[[{x},{y}],[{},{y}],[{},{}],[{x},{}],[{x},{y}]]]}}}}"#,
                x * 100.0,
                x + 0.01,
                x + 0.01,
                y + 0.01,
                y + 0.01
            )
        };
        let gj = format!(
            r#"{{"type":"FeatureCollection","features":[{},{},{}]}}"#,
            sq(11.0, 44.0),
            sq(11.01, 44.0),
            sq(11.02, 44.0)
        );
        std::fs::write(dir.join("blocks.geojson"), gj).unwrap();
        let style = r#"{"version":8,"sources":{"tiles":{"type":"raster","tiles":["https://x/{z}/{x}/{y}.png"]},"blocks":{"type":"geojson","data":"blocks.geojson"}},"layers":[]}"#;
        std::fs::write(dir.join("map.json"), style).unwrap();
        let mut d = def(1);
        d.fields.insert("population".into(), "pop".into());
        let c = load(&dir.join("map.json"), &d, 4).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c.style_source.as_deref(), Some("blocks"));
        assert!(
            c.style.as_ref().unwrap()["sources"]["blocks"]["data"]["features"].is_array(),
            "inlined"
        );
        for p in &c.places {
            assert!((2.0..=100.0).contains(&p.fields["wealth"]), "made up");
            assert!(p.fields["population"] > 1000.0 && p.fields["density"] > 0.0);
        }
        d.source = Some("nope".into());
        assert!(load(&dir.join("map.json"), &d, 4)
            .unwrap_err()
            .contains("nope"));
        std::fs::remove_dir_all(dir).ok();
    }
}
