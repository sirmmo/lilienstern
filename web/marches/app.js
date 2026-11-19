// Marches. The Lilienstern engine runs as WebAssembly (lilienstern.wasm); this page sends it
// JSON requests (see crates/lilienstern-web) and draws the land and its troops with MapLibre.

const GAME_FILES = ['params.toml', 'game.toml', 'realms.lil', 'eras/medieval.toml', 'eras/future.toml'];
const VIEWS = { Wealth: 'Wealth', Fort: 'Castles', Violence: 'Battles' };
const TWEEN_MS = 450;

const $ = (id) => document.getElementById(id);
const texts = {};
let wasm = null;
let info = null; // what `new` answered
let st = null; // the latest state
let player = 0;
let era = 'medieval';
let selected = null; // the place whose card is open
let chosen = []; // ids of the player's troops waiting for an order
let view = 'realms';
let map = null;
let busy = false;
let runner = null;
let centres = [];
const markers = new Map(); // stack key (lowest troop id) -> { marker, el, at }
let lastPos = new Map(); // troop id -> [lon, lat] where it was drawn last
let battleMarkers = [];

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

async function files() {
  const out = {};
  for (const f of GAME_FILES) {
    if (!texts[f]) {
      const r = await fetch('game/' + f);
      if (!r.ok) throw new Error(`${f}: ${r.status}`);
      texts[f] = await r.text();
    }
    out[f] = texts[f];
  }
  return out;
}

// ---------- start ----------

const radio = (name) => document.querySelector(`input[name=${name}]:checked`).value;

async function boot() {
  $('seed').value = 1 + Math.floor(Math.random() * 9999);
  try {
    await loadEngine();
    // A first, tiny game, to learn the realms' names and colours.
    const probe = call({ op: 'new', files: await files(), params: { provinces: 40 }, seed: 1 });
    const realms = probe.factions.map((f, i) => ({ ...f, i })).filter((f) => f.holds);
    $('realmChoice').innerHTML = '';
    realms.forEach((r, k) => {
      const b = document.createElement('button');
      b.innerHTML = `<i class="chip" style="background:${r.color}"></i>${r.name}`;
      b.onclick = () => { player = r.i; [...$('realmChoice').children].forEach((c) => c.classList.remove('on')); b.classList.add('on'); };
      if (k === 0) { b.classList.add('on'); player = r.i; }
      $('realmChoice').appendChild(b);
    });
    $('bStart').disabled = false;
  } catch (e) {
    $('startError').textContent = 'The engine did not start: ' + e.message;
  }
}

async function start() {
  $('bStart').disabled = true;
  $('bStart').textContent = 'Drawing the land…';
  try {
    era = radio('era');
    document.body.classList.toggle('future', era === 'future');
    info = call({ op: 'new', files: await files(), params: { era, provinces: +radio('size') }, seed: +$('seed').value || 1, player });
    st = info.state;
    info.geojson.features.forEach((f, i) => { f.properties.idx = i; });
    centres = info.geojson.features.map((f) => f.properties.centre);
    $('start').hidden = true;
    $('hud').hidden = false;
    selected = null;
    chosen = [];
    $('card').hidden = true;
    markers.forEach((m) => m.marker.remove());
    markers.clear();
    lastPos = new Map();
    setupHud();
    drawMap();
    if (!localStorage.getItem('marches.seenHelp')) { $('help').hidden = false; localStorage.setItem('marches.seenHelp', '1'); }
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

const narrow = () => window.innerWidth < 720;

function framePadding() {
  if (!narrow()) return { top: 30, bottom: 30, left: 320, right: 60 };
  const hud = $('hud').getBoundingClientRect();
  return { top: Math.min(hud.bottom + 10, window.innerHeight * 0.5), bottom: 20, left: 10, right: 10 };
}

const empty = () => ({ type: 'FeatureCollection', features: [] });

function drawMap() {
  const future = era === 'future';
  const style = {
    version: 8,
    sources: {
      places: { type: 'geojson', data: info.geojson, attribution: 'Lilienstern · Marches' },
      routes: { type: 'geojson', data: empty() },
      preview: { type: 'geojson', data: empty() },
    },
    layers: [
      // Water: the sea around the land, and the river through it.
      { id: 'ground', type: 'background', paint: { 'background-color': future ? '#0c1626' : '#9fbfcc' } },
      { id: 'terrain', type: 'fill', source: 'places', paint: { 'fill-color': ['get', 'tfill'], 'fill-opacity': future ? 0.75 : 0.95 } },
      { id: 'fill', type: 'fill', source: 'places', paint: { 'fill-color': ['get', 'fill'], 'fill-opacity': ['get', 'fill-opacity'] } },
      { id: 'line', type: 'line', source: 'places', paint: { 'line-color': future ? '#000' : '#5a4b35', 'line-width': 0.6, 'line-opacity': 0.55 } },
      { id: 'mine', type: 'line', source: 'places', filter: ['==', ['get', 'mine'], 1], paint: { 'line-color': future ? '#fff' : '#1b1209', 'line-width': 1.4, 'line-opacity': 0.8 } },
      { id: 'selected', type: 'line', source: 'places', filter: ['==', ['get', 'idx'], -1], paint: { 'line-color': '#ffd54a', 'line-width': 3.5 } },
      { id: 'routes', type: 'line', source: 'routes', paint: { 'line-color': ['get', 'color'], 'line-width': 2.5, 'line-dasharray': [2, 1.5], 'line-opacity': 0.9 } },
      { id: 'preview', type: 'line', source: 'preview', paint: { 'line-color': '#ffd54a', 'line-width': 3, 'line-dasharray': [1, 1] } },
    ],
  };
  if (map) map.remove();
  map = new maplibregl.Map({ container: 'map', style, bounds: bounds(info.geojson), fitBoundsOptions: { padding: framePadding() }, attributionControl: false });
  map.addControl(new maplibregl.AttributionControl({ compact: true }), 'bottom-right');
  if (!narrow()) map.addControl(new maplibregl.NavigationControl({ showCompass: false }), 'top-right');
  map.on('load', () => {
    paint();
    drawTroops(false);
    map.on('click', 'fill', (e) => clickPlace(e.features[0].properties.idx));
    map.on('mouseenter', 'fill', () => { map.getCanvas().style.cursor = 'pointer'; });
    map.on('mouseleave', 'fill', () => { map.getCanvas().style.cursor = ''; preview(null); });
    map.on('mousemove', 'fill', (e) => preview(e.features[0].properties.idx, e.point));
  });
}

function terrainColor(i) {
  const t = info.terrain[i];
  return t >= 0 && info.terrains[t].color ? info.terrains[t].color : '#cccccc';
}

function paint() {
  if (!map || !map.getSource('places')) return;
  const values = VIEWS[view] ? st.values || [] : null;
  info.geojson.features.forEach((f, i) => {
    const o = st.owner[i];
    const fac = o >= 0 ? info.factions[o] : null;
    f.properties.tfill = terrainColor(i);
    f.properties.fill = fac ? fac.color : '#000000';
    f.properties['fill-opacity'] = view === 'terrain' || !fac ? 0 : o === player ? 0.68 : 0.58;
    f.properties.mine = o === player ? 1 : 0;
    if (values) f.properties.v = values[i] || 0;
  });
  map.getSource('places').setData(info.geojson);
  if (values) {
    const max = Math.max(view === 'Fort' ? 3 : 1, ...values);
    map.setPaintProperty('fill', 'fill-color', ['interpolate', ['linear'], ['get', 'v'], 0, '#ffffcc', max / 2, '#fd8d3c', max, '#bd0026']);
    map.setPaintProperty('fill', 'fill-opacity', ['interpolate', ['linear'], ['get', 'v'], 0, 0.0, max * 0.04, 0.75]);
  } else {
    map.setPaintProperty('fill', 'fill-color', ['get', 'fill']);
    map.setPaintProperty('fill', 'fill-opacity', ['get', 'fill-opacity']);
  }
  map.setFilter('selected', ['==', ['get', 'idx'], selected ?? -1]);
  routes();
}

// ---------- troops on the map ----------

function lerp(a, b, s) {
  return [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s];
}

function troopAt(t) {
  return t.n >= 0 ? lerp(centres[t.p], centres[t.n], t.s) : centres[t.p];
}

/** Troops of one realm at the same spot share a marker. */
function stacks() {
  const out = new Map();
  for (const t of st.troops) {
    const key = `${t.f}:${t.p}:${t.n}:${t.s}`;
    if (!out.has(key)) out.set(key, { f: t.f, p: t.p, at: troopAt(t), list: [] });
    out.get(key).list.push(t);
  }
  return [...out.values()].map((s) => ({ ...s, id: Math.min(...s.list.map((t) => t.id)) }));
}

function drawTroops(animate) {
  const seen = new Set();
  const moves = [];
  for (const s of stacks()) {
    seen.add(s.id);
    let m = markers.get(s.id);
    if (!m) {
      const el = document.createElement('div');
      el.addEventListener('click', (e) => { e.stopPropagation(); clickStack(m.stack); });
      m = { el, marker: new maplibregl.Marker({ element: el }), at: null };
      markers.set(s.id, m);
    }
    m.stack = s;
    const kinds = s.list.map((t) => t.k);
    const one = kinds.every((k) => k === kinds[0]);
    const hp = s.list.reduce((a, t) => a + t.hp, 0) / s.list.length;
    const mine = s.f === player;
    m.el.className = 'troop' + (mine ? ' mine' : '') + (mine && s.list.some((t) => chosen.includes(t.id)) ? ' selected' : '');
    m.el.style.background = info.factions[s.f].color;
    m.el.innerHTML = `${one ? info.troopKinds[kinds[0]].icon : '✦'}${s.list.length > 1 ? `<small>${s.list.length}</small>` : ''}<span class="hpbar"><i style="width:${hp}%"></i></span>`;
    m.el.title = `${info.factions[s.f].name}: ${summary(s.list)}`;
    const from = animate ? (m.at || lastPos.get(s.id) || s.at) : s.at;
    if (!m.at) m.marker.setLngLat(from).addTo(map);
    moves.push({ m, from, to: s.at });
    m.at = s.at;
  }
  for (const [id, m] of markers) if (!seen.has(id)) { m.marker.remove(); markers.delete(id); }
  lastPos = new Map();
  for (const t of st.troops) lastPos.set(t.id, troopAt(t));
  if (!animate) { moves.forEach(({ m, to }) => m.marker.setLngLat(to)); return; }
  const t0 = performance.now();
  const frame = (now) => {
    const k = Math.min(1, (now - t0) / TWEEN_MS);
    const e = k < 0.5 ? 2 * k * k : 1 - (-2 * k + 2) ** 2 / 2;
    moves.forEach(({ m, from, to }) => m.marker.setLngLat(lerp(from, to, e)));
    if (k < 1) requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}

function summary(list) {
  const n = {};
  list.forEach((t) => { n[t.k] = (n[t.k] || 0) + 1; });
  return Object.entries(n).map(([k, c]) => `${c} ${info.troopKinds[k].name}`).join(', ');
}

function battles() {
  battleMarkers.forEach((m) => m.remove());
  battleMarkers = (st.fights || []).map((p) => {
    const el = document.createElement('div');
    el.className = 'battle';
    el.textContent = '⚔';
    return new maplibregl.Marker({ element: el, offset: [0, -22] }).setLngLat(centres[p]).addTo(map);
  });
}

function routes() {
  if (!map.getSource('routes')) return;
  const color = info.factions[player].color;
  const features = st.troops.filter((t) => t.f === player && t.path && t.path.length).map((t) => ({
    type: 'Feature', properties: { color },
    geometry: { type: 'LineString', coordinates: [troopAt(t), ...t.path.map((p) => centres[p])] },
  }));
  map.getSource('routes').setData({ type: 'FeatureCollection', features });
}

// ---------- orders ----------

function clickStack(s) {
  if (s.f !== player) { clickPlace(s.p); return; }
  const ids = s.list.map((t) => t.id);
  const all = ids.every((id) => chosen.includes(id));
  choose(all ? chosen.filter((id) => !ids.includes(id)) : [...new Set([...chosen, ...ids])]);
}

function choose(ids, redraw = true) {
  chosen = ids.filter((id) => st.troops.some((t) => t.id === id && t.f === player));
  $('orders').hidden = !chosen.length;
  if (chosen.length) {
    const list = st.troops.filter((t) => chosen.includes(t.id));
    $('ordersText').innerHTML = `<b>${summary(list)}</b>: click where to march`;
  }
  if (!redraw) return;
  preview(null);
  drawTroops(false);
  if (selected !== null) card();
}

function clickPlace(p) {
  if (chosen.length) { march(p); return; }
  selected = p;
  card();
  paint();
}

function march(to) {
  let days = 0, ok = 0, why = '';
  for (const id of chosen) {
    try {
      const r = call({ op: 'order', troop: id, to });
      days = Math.max(days, r.days);
      ok++;
    } catch (e) {
      why = e.message;
    }
  }
  if (ok) news(`${ok > 1 ? ok + ' troops march' : 'Your troop marches'} to ${placeName(to)}${days ? `: ${days} day${days > 1 ? 's' : ''}` : ''}.`);
  else news(why || 'They cannot get there.', 'bad');
  st = call({ op: 'state', map: viewIndex() });
  choose([]);
  refresh(false);
}

let previewFor = -1;
function preview(p, point) {
  const tip = document.querySelector('.tip');
  if (p === null || !chosen.length) {
    previewFor = -1;
    if (tip) tip.remove();
    if (map && map.getSource('preview')) map.getSource('preview').setData(empty());
    return;
  }
  if (p !== previewFor) {
    previewFor = p;
    const t = st.troops.find((x) => x.id === chosen[0]);
    let text = 'cannot get there';
    let line = empty();
    try {
      const r = call({ op: 'route', troop: chosen[0], to: p });
      text = r.days ? `${r.days} day${r.days > 1 ? 's' : ''}` : 'here';
      line = { type: 'FeatureCollection', features: [{ type: 'Feature', properties: {}, geometry: { type: 'LineString', coordinates: [troopAt(t), ...r.path.map((q) => centres[q])] } }] };
    } catch { /* no way there */ }
    map.getSource('preview').setData(line);
    const el = tip || Object.assign(document.createElement('div'), { className: 'tip' });
    el.textContent = `${placeName(p)}: ${text}`;
    if (!tip) document.body.appendChild(el);
  }
  const el = document.querySelector('.tip');
  if (el && point) { el.style.left = `${point.x + 14}px`; el.style.top = `${point.y + 10}px`; }
}

// ---------- the panels ----------

function setupHud() {
  const me = info.factions[player];
  $('realmColor').style.background = me.color;
  $('realmName').textContent = me.name;
  $('days').textContent = info.days;
  $('view').innerHTML = '';
  $('view').add(new Option('Realms', 'realms'));
  $('view').add(new Option('Terrain', 'terrain'));
  Object.entries(VIEWS).filter(([id]) => info.maps.includes(id)).forEach(([id, label]) => $('view').add(new Option(label, id)));
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
  $('funds').textContent = fmt(acct.Funds || 0);
  $('recruits').textContent = fmt(acct.Recruits || 0);
  $('provinces').textContent = st.territory[player];
  const realms = info.factions.filter((f) => f.holds).length;
  $('rank').textContent = st.alive[player] ? `#${st.rank} of ${realms}` : 'fallen';
  const left = st.actions_left ?? 0;
  $('moves').innerHTML = Array.from({ length: me.actions }, (_, i) => `<span class="dot${i < me.actions - left ? ' used' : ''}"></span>`).join('');
  const mine = st.troops.filter((t) => t.f === player);
  const idle = mine.filter((t) => !t.path || !t.path.length);
  const counts = info.troopKinds.map((k, i) => ({ k, n: mine.filter((t) => t.k === i).length })).filter((x) => x.n);
  $('army').innerHTML = `<span class="lbl">Your troops: ${mine.length} (${idle.length} without orders)</span>`
    + `<div class="kinds">${counts.map((x) => `<span title="${x.k.description}">${x.k.icon} ${x.k.name} × ${x.n}</span>`).join('')}</div>`;
  if (idle.length) {
    const b = document.createElement('button');
    b.textContent = 'Select the troops without orders';
    b.onclick = () => choose(idle.map((t) => t.id));
    $('army').appendChild(b);
  }
  legend();
}

function legend() {
  if (VIEWS[view]) {
    const max = Math.max(view === 'Fort' ? 3 : 1, ...(st.values || [1]));
    $('legend').innerHTML = `<div><span>0</span><span class="ramp"></span><span>${fmt(max)}</span></div>`;
    return;
  }
  if (view === 'terrain') {
    $('legend').innerHTML = info.terrains.map((t) => `<div><i class="chip" style="background:${t.color}"></i>${t.name}<span class="n">${t.defense ? `+${t.defense} defence` : ''}</span></div>`).join('');
    return;
  }
  const army = info.factions.map(() => 0);
  st.troops.forEach((t) => army[t.f]++);
  const rows = info.factions.map((f, i) => ({ f, i, n: st.territory[i] })).filter((r) => r.f.holds).sort((a, b) => b.n - a.n);
  $('legend').innerHTML = rows.map((r) => {
    const rel = r.i === player ? '' : ` <span class="rel ${st.relations[r.i]}">${st.relations[r.i]}</span>`;
    return `<div class="${r.i === player ? 'me' : ''}"><i class="chip" style="background:${r.f.color}"></i>${r.f.name}${st.alive[r.i] ? '' : ' <span class="muted">(fallen)</span>'}${rel}<span class="n">${r.n} · ${army[r.i]}⚑</span></div>`;
  }).join('') + `<div class="muted"><i class="chip" style="background:transparent"></i>nobody<span class="n">${st.owner.filter((o) => o < 0).length}</span></div>`;
}

function costText(o) {
  return o.costs.map((c) => `${fmt(c.amount)} ${c.resource.toLowerCase()}`).join(', ');
}

function optionButton(o, place) {
  const row = document.createElement('div');
  row.className = 'opt';
  const b = document.createElement('button');
  b.textContent = o.raise ? `Raise ${o.raise}` : o.label;
  b.disabled = !o.affordable || busy;
  b.onclick = () => take(o, place);
  row.appendChild(b);
  const c = document.createElement('span');
  c.className = 'cost';
  c.textContent = costText(o);
  row.appendChild(c);
  return row;
}

function card() {
  if (selected === null) { $('card').hidden = true; return; }
  const c = call({ op: 'place', place: selected });
  $('card').hidden = false;
  $('cardName').textContent = c.name;
  const owner = c.owner !== null ? info.factions[c.owner] : null;
  const rel = owner && c.owner !== player ? ` <span class="rel ${st.relations[c.owner]}">${st.relations[c.owner]}</span>` : '';
  $('cardOwner').innerHTML = owner ? `Held by <b>${owner.name}</b>${c.owner === player ? ' (you)' : ''}${rel}` : 'Nobody holds it';
  const ter = c.terrain !== null ? info.terrains[c.terrain] : null;
  const kv = [
    ter ? `<span>Terrain</span><span>${ter.name}${ter.defense ? ` (+${ter.defense})` : ''}</span>` : '',
    `<span>Castles</span><span>${fmt(c.maps.Fort || 0)}</span>`,
    `<span>Wealth</span><span>${fmt(c.maps.Wealth || 0)}</span>`,
  ].join('');
  const top = Math.max(40, ...c.presence.map((p) => p.presence));
  const bars = c.presence.sort((a, b) => b.presence - a.presence).map((p) => {
    const f = info.factions[p.faction];
    return `<div class="bar"><i class="chip" style="background:${f.color}"></i><span>${f.name}</span><div class="track"><div class="fill" style="width:${100 * p.presence / top}%;background:${f.color}"></div></div><span class="v">${fmt(p.presence)}</span></div>`;
  }).join('') || '<div class="muted">No realm has a hold here.</div>';
  $('cardBody').innerHTML = `<div class="kv">${kv}</div><h3>Hold</h3>${bars}<h3>Troops here</h3><div class="troops" id="cardTroops"></div>`;
  const box = $('cardTroops');
  if (!c.troops.length) box.innerHTML = '<div class="muted">None.</div>';
  c.troops.forEach((t) => {
    const k = info.troopKinds[t.kind];
    const row = document.createElement('div');
    row.innerHTML = `<i class="chip" style="background:${info.factions[t.faction].color}"></i>${k.icon} ${k.name}<span class="hp"><i style="width:${t.hp}%"></i></span>`;
    const where = document.createElement('span');
    where.className = 'where';
    where.textContent = t.moving ? `→ ${placeName(t.to)}` : '';
    row.appendChild(where);
    if (t.faction === player) {
      const b = document.createElement('button');
      b.textContent = chosen.includes(t.id) ? 'Chosen' : 'Order';
      b.className = chosen.includes(t.id) ? 'on' : '';
      b.onclick = () => choose(chosen.includes(t.id) ? chosen.filter((x) => x !== t.id) : [...chosen, t.id]);
      row.appendChild(b);
    }
    box.appendChild(row);
  });
  $('cardOptions').innerHTML = '';
  if (!c.options.length) $('cardOptions').innerHTML = '<div class="muted">Nothing you can decide here now.</div>';
  c.options.forEach((o) => $('cardOptions').appendChild(optionButton(o, selected)));
}

function news(msg, kind) {
  const d = document.createElement('div');
  d.textContent = msg;
  if (kind) d.className = kind;
  $('news').prepend(d);
  while ($('news').children.length > 6) $('news').lastChild.remove();
  setTimeout(() => d.remove(), 9000);
}

const placeName = (i) => info.geojson.features[i].properties.name;
const viewIndex = () => (VIEWS[view] ? info.maps.indexOf(view) : null);

// ---------- playing ----------

function take(o, place) {
  try {
    const before = st.troops.length;
    st = call({ op: 'take', decision: o.decision, place, map: viewIndex() });
    if (o.raise && st.troops.length > before) news(`${o.raise} raised in ${placeName(place)}.`, 'good');
    refresh(false);
  } catch (e) {
    news(e.message, 'bad');
  }
}

function step(days) {
  if (busy || st.over) return;
  busy = true;
  const before = st;
  st = call({ op: 'step', days, map: viewIndex() });
  busy = false;
  report(before, st);
  refresh(true);
  if (st.over || !st.alive[player] || st.territory[player] === st.owner.length) gameOver();
}

function report(a, b) {
  const gained = [], lost = [];
  b.owner.forEach((o, i) => {
    if (o === player && a.owner[i] !== player) gained.push(i);
    if (a.owner[i] === player && o !== player) lost.push(i);
  });
  const names = (xs) => xs.slice(0, 3).map(placeName).join(', ') + (xs.length > 3 ? ` and ${xs.length - 3} more` : '');
  if (gained.length) news(`Taken: ${names(gained)}.`, 'good');
  if (lost.length) news(`Lost: ${names(lost)}.`, 'bad');
  const mineBefore = a.troops.filter((t) => t.f === player).map((t) => t.id);
  const gone = mineBefore.filter((id) => !b.troops.some((t) => t.id === id)).length;
  if (gone) news(`${gone} of your troops ${gone > 1 ? 'were' : 'was'} destroyed.`, 'bad');
  const arrived = a.troops.filter((t) => t.f === player && t.path && t.path.length && b.troops.some((u) => u.id === t.id && (!u.path || !u.path.length)));
  if (arrived.length) news(`${arrived.length > 1 ? arrived.length + ' troops have' : 'A troop has'} arrived.`);
  (b.relations || []).forEach((r, i) => {
    const was = a.relations[i];
    if (i === player || r === was) return;
    const name = info.factions[i].name;
    if (r === 'war') news(`${name} is at war with you.`, 'bad');
    else if (r === 'peace') news(`Peace with ${name}.`, 'good');
    else if (r === 'ally') news(`${name} is your ally.`, 'good');
  });
  if (a.alive[player] && !b.alive[player]) news('Your realm has fallen.', 'bad');
}

function refresh(animate) {
  choose(chosen, false);
  hud();
  paint();
  drawTroops(animate);
  battles();
  if (selected !== null) card();
}

function run() {
  if (runner) { clearInterval(runner); runner = null; $('bPlay').textContent = '▶ Run'; return; }
  $('bPlay').textContent = '❚❚ Pause';
  runner = setInterval(() => {
    if (st.over || !st.alive[player]) { run(); return; }
    step(1);
  }, 650);
}

function gameOver() {
  if (runner) run();
  const rows = info.factions.map((f, i) => ({ f, i, n: st.territory[i] })).filter((r) => r.f.holds).sort((a, b) => b.n - a.n);
  const place = rows.findIndex((r) => r.i === player) + 1;
  const all = st.territory[player] === st.owner.length;
  $('overTitle').textContent = all ? 'The land is yours' : !st.alive[player] ? 'Your realm has fallen' : 'Three years later';
  $('overText').innerHTML = `<p>On day ${st.day} you are <b>#${place}</b> of ${rows.length}, holding <b>${st.territory[player]}</b> provinces.</p>`
    + rows.map((r) => `<div><i class="chip" style="background:${r.f.color}"></i> ${r.f.name}: ${r.n}</div>`).join('')
    + `<p class="muted">Battles fought: ${fmt(st.battles || 0)}; troops destroyed: ${fmt(st.destroyed || 0)}.</p>`;
  $('over').hidden = false;
}

// ---------- controls ----------

$('bStart').onclick = start;
$('bDay').onclick = () => step(1);
$('bWeek').onclick = () => step(7);
$('bPlay').onclick = run;
$('cardClose').onclick = () => { selected = null; card(); paint(); };
$('ordersCancel').onclick = () => choose([]);
$('view').onchange = (e) => {
  view = e.target.value;
  st = call({ op: 'state', map: viewIndex() });
  paint();
  legend();
};
$('bHelp').onclick = (e) => { e.preventDefault(); $('help').hidden = false; };
$('bFold').onclick = () => { $('hud').classList.toggle('open'); $('bFold').textContent = $('hud').classList.contains('open') ? 'Less ▴' : 'Realms and views ▾'; };
$('bHelpClose').onclick = () => { $('help').hidden = true; };
$('bAgain').onclick = () => { $('over').hidden = true; $('hud').hidden = true; $('card').hidden = true; $('start').hidden = false; $('seed').value = 1 + Math.floor(Math.random() * 9999); };
document.addEventListener('keydown', (e) => {
  if (e.target.tagName === 'INPUT' || e.target.tagName === 'SELECT') return;
  const playing = !$('hud').hidden && $('start').hidden && $('over').hidden;
  if (e.code === 'Space' && playing) { e.preventDefault(); step(1); }
  if (e.code === 'KeyP' && playing) run();
  if (e.code === 'Escape') { if (chosen.length) choose([]); else { selected = null; card(); paint(); } }
});

// For tests and the curious.
window.__marches = { call, get state() { return st; }, get info() { return info; }, choose, clickPlace, step };

boot();
