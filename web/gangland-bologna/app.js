// Gangland: Bologna. The Lilienstern engine runs as WebAssembly (lilienstern.wasm); this page
// sends it JSON requests (see crates/lilienstern-web) and draws the city with MapLibre.

const GAME_FILES = ['params.toml', 'game.toml', 'economy.gbx', 'politics.lil'];
const MAPS = { sections: 'maps/bologna-sections.geojson', areas: 'maps/bologna-areas.geojson' };
const VIEWS = ['Violence', 'Police', 'Heat', 'Wealth', 'Traffic'];

const $ = (id) => document.getElementById(id);
const texts = {};
let wasm = null;
let info = null; // what `new` answered
let st = null; // the latest state
let player = 0;
let selected = null;
let view = 'territory';
let map = null;
let busy = false;

// ---------- the engine ----------

async function loadEngine() {
  const res = await fetch('lilienstern.wasm');
  let module;
  try {
    module = await WebAssembly.instantiateStreaming(res.clone(), {});
  } catch {
    module = await WebAssembly.instantiate(await res.arrayBuffer(), {});
  }
  wasm = module.instance.exports;
}

function call(request) {
  const bytes = new TextEncoder().encode(JSON.stringify(request));
  const ptr = wasm.lil_alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
  const out = wasm.lil_call(ptr, bytes.length);
  const answer = JSON.parse(new TextDecoder().decode(new Uint8Array(wasm.memory.buffer, out, wasm.lil_out_len())));
  if (answer && answer.error) throw new Error(answer.error);
  return answer;
}

async function text(path) {
  if (!texts[path]) {
    const r = await fetch('game/' + path);
    if (!r.ok) throw new Error(`${path}: ${r.status}`);
    texts[path] = await r.text();
  }
  return texts[path];
}

async function files(mapKey) {
  const out = {};
  for (const f of GAME_FILES) out[f] = await text(f);
  out[MAPS[mapKey]] = await text(MAPS[mapKey]);
  return out;
}

// ---------- start ----------

function chosenMap() {
  return document.querySelector('input[name=map]:checked').value;
}

async function boot() {
  $('seed').value = 1 + Math.floor(Math.random() * 9999);
  try {
    await loadEngine();
    // A first game on the small map, to learn the gangs' names and colours.
    const probe = call({ op: 'new', files: await files('areas'), params: { city_file: MAPS.areas }, seed: 1 });
    const gangs = probe.factions.map((f, i) => ({ ...f, i })).filter((f) => f.gang);
    $('gangChoice').innerHTML = '';
    gangs.forEach((g, k) => {
      const b = document.createElement('button');
      b.innerHTML = `<i class="chip" style="background:${g.color}"></i>${g.name}`;
      b.onclick = () => { player = g.i; [...$('gangChoice').children].forEach((c) => c.classList.remove('on')); b.classList.add('on'); };
      if (k === 0) { b.classList.add('on'); player = g.i; }
      $('gangChoice').appendChild(b);
    });
    $('bStart').disabled = false;
  } catch (e) {
    $('startError').textContent = 'The engine did not start: ' + e.message;
  }
}

async function start() {
  $('bStart').disabled = true;
  $('bStart').textContent = 'Loading the city…';
  try {
    const key = chosenMap();
    info = call({ op: 'new', files: await files(key), params: { city_file: MAPS[key] }, seed: +$('seed').value || 1, player });
    st = info.state;
    info.geojson.features.forEach((f, i) => { f.properties.idx = i; });
    $('start').hidden = true;
    $('hud').hidden = false;
    selected = null;
    $('card').hidden = true;
    setupHud();
    drawMap();
    if (!localStorage.getItem('gangland.seenHelp')) { $('help').hidden = false; localStorage.setItem('gangland.seenHelp', '1'); }
  } catch (e) {
    $('startError').textContent = e.message;
  } finally {
    $('bStart').disabled = false;
    $('bStart').textContent = 'Start';
  }
}

// ---------- the map ----------

function bounds(gj) {
  const b = [[180, 90], [-180, -90]];
  for (const f of gj.features) for (const [x, y] of f.geometry.coordinates[0]) {
    b[0][0] = Math.min(b[0][0], x); b[0][1] = Math.min(b[0][1], y);
    b[1][0] = Math.max(b[1][0], x); b[1][1] = Math.max(b[1][1], y);
  }
  return b;
}

function narrow() {
  return window.innerWidth < 720;
}

/** Room left for the city around the panels. */
function framePadding() {
  if (!narrow()) return { top: 30, bottom: 30, left: 310, right: 30 };
  const hud = $('hud').getBoundingClientRect();
  return { top: Math.min(hud.bottom + 10, window.innerHeight * 0.5), bottom: 20, left: 10, right: 10 };
}

function drawMap() {
  const credit = ['Lilienstern', info.credit].filter(Boolean).join(' · ');
  const style = {
    version: 8,
    sources: {
      osm: { type: 'raster', tiles: ['https://tile.openstreetmap.org/{z}/{x}/{y}.png'], tileSize: 256, maxzoom: 19, attribution: '© <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors' },
      places: { type: 'geojson', data: info.geojson, attribution: credit },
    },
    layers: [
      { id: 'osm', type: 'raster', source: 'osm', paint: { 'raster-saturation': -0.7, 'raster-opacity': 0.9 } },
      { id: 'fill', type: 'fill', source: 'places', paint: { 'fill-color': ['get', 'fill'], 'fill-opacity': ['get', 'fill-opacity'] } },
      { id: 'line', type: 'line', source: 'places', paint: { 'line-color': '#333', 'line-width': 0.4, 'line-opacity': 0.5 } },
      { id: 'mine', type: 'line', source: 'places', filter: ['==', ['get', 'mine'], 1], paint: { 'line-color': '#111', 'line-width': 1.6 } },
      { id: 'selected', type: 'line', source: 'places', filter: ['==', ['get', 'idx'], -1], paint: { 'line-color': '#000', 'line-width': 3.5 } },
    ],
  };
  if (map) map.remove();
  map = new maplibregl.Map({ container: 'map', style, bounds: bounds(info.geojson), fitBoundsOptions: { padding: framePadding() }, attributionControl: false });
  map.addControl(new maplibregl.AttributionControl({ compact: true }), 'bottom-right');
  if (!narrow()) map.addControl(new maplibregl.NavigationControl(), 'top-right');
  map.on('load', () => {
    if (narrow()) document.querySelectorAll('.maplibregl-ctrl-attrib').forEach((e) => e.classList.remove('maplibregl-compact-show'));
    paint();
    map.on('click', 'fill', (e) => select(e.features[0].properties.idx));
    map.on('mouseenter', 'fill', () => { map.getCanvas().style.cursor = 'pointer'; });
    map.on('mouseleave', 'fill', () => { map.getCanvas().style.cursor = ''; });
  });
}

function paint() {
  if (!map || !map.getSource('places')) return;
  const values = view === 'territory' ? null : st.values || [];
  info.geojson.features.forEach((f, i) => {
    const o = st.owner[i];
    const fac = o >= 0 ? info.factions[o] : null;
    f.properties.fill = fac ? fac.color : '#cfcfcf';
    f.properties['fill-opacity'] = fac ? (o === player ? 0.75 : 0.5) : 0.08;
    f.properties.mine = o === player ? 1 : 0;
    if (values) f.properties.v = values[i] || 0;
  });
  map.getSource('places').setData(info.geojson);
  if (view === 'territory') {
    map.setPaintProperty('fill', 'fill-color', ['get', 'fill']);
    map.setPaintProperty('fill', 'fill-opacity', ['get', 'fill-opacity']);
  } else {
    const max = Math.max(1, ...(st.values || [1]));
    map.setPaintProperty('fill', 'fill-color', ['interpolate', ['linear'], ['get', 'v'], 0, '#ffffcc', max / 2, '#fd8d3c', max, '#bd0026']);
    map.setPaintProperty('fill', 'fill-opacity', ['interpolate', ['linear'], ['get', 'v'], 0, 0.05, max * 0.03, 0.7]);
  }
  map.setFilter('selected', ['==', ['get', 'idx'], selected ?? -1]);
}

// ---------- the panels ----------

function setupHud() {
  const me = info.factions[player];
  $('gangColor').style.background = me.color;
  $('gangName').textContent = me.name;
  $('days').textContent = info.days;
  $('view').innerHTML = '';
  $('view').add(new Option('Territory', 'territory'));
  VIEWS.filter((v) => info.maps.includes(v)).forEach((v) => $('view').add(new Option(v, v)));
  $('view').value = view;
  hud();
}

function fmt(v) {
  return v >= 1000 ? Math.round(v).toLocaleString('en') : (Math.round(v * 10) / 10).toString();
}

function hud() {
  const me = info.factions[player];
  const acct = st.accounts[player] || {};
  $('day').textContent = `Day ${st.day}`;
  $('cash').textContent = fmt(acct.Cash || 0);
  $('crew').textContent = fmt(acct.Crew || 0);
  $('blocks').textContent = st.territory[player];
  const gangs = info.factions.filter((f) => f.gang).length;
  $('rank').textContent = st.alive[player] ? `#${st.rank} of ${gangs}` : 'wiped out';
  const left = st.actions_left ?? 0;
  $('moves').innerHTML = Array.from({ length: me.actions }, (_, i) => `<span class="dot${i < me.actions - left ? ' used' : ''}"></span>`).join('');
  const own = call({ op: 'options' });
  $('own').innerHTML = '';
  own.forEach((o) => $('own').appendChild(optionButton(o, null)));
  legend();
}

function legend() {
  if (view !== 'territory') {
    const max = Math.max(1, ...(st.values || [1]));
    $('legend').innerHTML = `<div><span>0</span><span class="ramp"></span><span>${fmt(max)}</span></div>`;
    return;
  }
  const rows = info.factions.map((f, i) => ({ f, i, n: st.territory[i] })).filter((r) => r.f.gang).sort((a, b) => b.n - a.n);
  $('legend').innerHTML = rows.map((r) => `<div class="${r.i === player ? 'me' : ''}"><i class="chip" style="background:${r.f.color}"></i>${r.f.name}${st.alive[r.i] ? '' : ' <span class="muted">(gone)</span>'}<span class="n">${r.n}</span></div>`).join('')
    + `<div class="muted"><i class="chip" style="background:#cfcfcf"></i>nobody<span class="n">${st.owner.filter((o) => o < 0).length}</span></div>`;
}

function costText(o) {
  return o.costs.map((c) => `${fmt(c.amount)} ${c.resource.toLowerCase()}`).join(', ');
}

function optionButton(o, place) {
  const row = document.createElement('div');
  row.className = 'opt';
  const b = document.createElement('button');
  b.textContent = o.label;
  b.disabled = !o.affordable || busy;
  b.onclick = () => take(o, place);
  row.appendChild(b);
  const c = document.createElement('span');
  c.className = 'cost';
  c.textContent = costText(o);
  row.appendChild(c);
  return row;
}

function select(i) {
  selected = i;
  card();
  paint();
}

function card() {
  if (selected === null) { $('card').hidden = true; return; }
  const c = call({ op: 'place', place: selected });
  $('card').hidden = false;
  $('cardName').textContent = c.name;
  const owner = c.owner !== null ? info.factions[c.owner] : null;
  $('cardOwner').innerHTML = owner ? `Held by <b>${owner.name}</b>${c.owner === player ? ' (you)' : ''}` : 'Nobody holds it';
  const top = Math.max(40, ...c.presence.map((p) => p.presence));
  const bars = c.presence.sort((a, b) => b.presence - a.presence).map((p) => {
    const f = info.factions[p.faction];
    return `<div class="bar"><i class="chip" style="background:${f.color}"></i><span>${f.name}</span><div class="track"><div class="fill" style="width:${100 * p.presence / top}%;background:${f.color}"></div></div><span class="v">${fmt(p.presence)}</span></div>`;
  }).join('') || '<div class="muted">No gang is here.</div>';
  const kv = ['Wealth', 'Police', 'Heat', 'Violence'].filter((k) => k in c.maps).map((k) => `<span>${k}</span><span>${fmt(c.maps[k])}</span>`).join('');
  const units = c.units.length ? c.units.map((u) => `${u.unit}${u.faction !== null ? ' (' + info.factions[u.faction].name + ')' : ''}`).join(', ') : 'none';
  $('cardBody').innerHTML = `<h3>Strength</h3>${bars}<h3>The block</h3><div class="kv">${c.population !== null ? `<span>Residents</span><span>${fmt(c.population)}</span>` : ''}${kv}</div><h3>Built here</h3><div>${units}</div>`;
  $('cardOptions').innerHTML = '';
  if (!c.options.length) $('cardOptions').innerHTML = '<div class="muted">Nothing you can do here now.</div>';
  c.options.forEach((o) => $('cardOptions').appendChild(optionButton(o, selected)));
}

function news(msg) {
  const d = document.createElement('div');
  d.textContent = msg;
  $('news').prepend(d);
  while ($('news').children.length > 6) $('news').lastChild.remove();
  setTimeout(() => d.remove(), 8000);
}

function placeName(i) {
  return info.geojson.features[i].properties.name;
}

// ---------- playing ----------

function viewIndex() {
  return view === 'territory' ? null : info.maps.indexOf(view);
}

function take(o, place) {
  try {
    const before = st.owner.slice();
    st = call({ op: 'take', decision: o.decision, place, map: viewIndex() });
    if (place !== null && before[place] !== player && st.owner[place] === player) news(`You took ${placeName(place)}.`);
    refresh();
  } catch (e) {
    news(e.message);
  }
}

function step(days) {
  if (busy || st.over) return;
  busy = true;
  const before = st.owner.slice();
  const wasAlive = st.alive[player];
  st = call({ op: 'step', days, map: viewIndex() });
  busy = false;
  const gained = [], lost = [];
  st.owner.forEach((o, i) => {
    if (o === player && before[i] !== player) gained.push(i);
    if (before[i] === player && o !== player) lost.push(i);
  });
  const names = (xs) => xs.slice(0, 3).map(placeName).join(', ') + (xs.length > 3 ? ` and ${xs.length - 3} more` : '');
  if (gained.length) news(`Taken: ${names(gained)}.`);
  if (lost.length) news(`Lost: ${names(lost)}.`);
  if (wasAlive && !st.alive[player]) news('Your gang has been wiped out.');
  refresh();
  if (st.over) gameOver();
}

function refresh() {
  hud();
  paint();
  if (selected !== null) card();
}

function gameOver() {
  const rows = info.factions.map((f, i) => ({ f, i, n: st.territory[i] })).filter((r) => r.f.gang).sort((a, b) => b.n - a.n);
  const place = rows.findIndex((r) => r.i === player) + 1;
  $('overText').innerHTML = `<p>You finished <b>#${place}</b> of ${rows.length}, holding <b>${st.territory[player]}</b> blocks.</p>`
    + rows.map((r) => `<div><i class="chip" style="background:${r.f.color}"></i> ${r.f.name}: ${r.n}</div>`).join('')
    + `<p class="muted">Violence over the two years: ${fmt(st.violence)}; blocks changed hands ${fmt(st.captures)} times.</p>`;
  $('over').hidden = false;
}

// ---------- controls ----------

$('bStart').onclick = start;
$('bDay').onclick = () => step(1);
$('bWeek').onclick = () => step(7);
$('bMonth').onclick = () => step(30);
$('cardClose').onclick = () => select(null);
$('view').onchange = (e) => {
  view = e.target.value;
  st = call({ op: 'state', map: viewIndex() });
  paint();
  legend();
};
$('bHelp').onclick = (e) => { e.preventDefault(); $('help').hidden = false; };
$('bFold').onclick = () => { $('hud').classList.toggle('open'); $('bFold').textContent = $('hud').classList.contains('open') ? 'Less ▴' : 'Gangs and views ▾'; };
$('bHelpClose').onclick = () => { $('help').hidden = true; };
$('bAgain').onclick = () => { $('over').hidden = true; $('hud').hidden = true; $('card').hidden = true; $('start').hidden = false; $('seed').value = 1 + Math.floor(Math.random() * 9999); };
document.addEventListener('keydown', (e) => {
  if (e.target.tagName === 'INPUT' || e.target.tagName === 'SELECT') return;
  if (e.code === 'Space' && !$('hud').hidden && $('start').hidden) { e.preventDefault(); step(1); }
  if (e.code === 'Escape') select(null);
});

boot();
