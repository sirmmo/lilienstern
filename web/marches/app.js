// Marches (and games built like it, such as Italia). The Lilienstern engine runs as
// WebAssembly (lilienstern.wasm); this page sends it JSON requests (see crates/lilienstern-web)
// and draws the land and its troops with MapLibre.
//
// A page can set `window.GAME` before loading this script to play another game: which files to
// load (paths as the game names them, fetched from game/), the parameters its start screen
// sets, which parameter says how many factions play, and what the game calls things (Gangland:
// gangs, blocks, cash and crew, over a street map).

const radio = (name) => document.querySelector(`input[name=${name}]:checked`).value;
const CONFIG = Object.assign({
  id: 'marches',
  files: () => ['params.toml', 'game.toml', 'realms.lil', 'people.lil', 'eras/medieval.toml', 'eras/medieval.lil', 'eras/future.toml'],
  probe: { provinces: 40 }, // a quick game, to learn the factions' names and colours
  params: () => ({ era: radio('era'), provinces: +radio('size') }),
  count: null, // the parameter with the number of factions that play, if the start screen sets it
  cities: null, // a GeoJSON of points (name, province, big) drawn as labels, fetched from game/
  crests: null, // faction name -> image
  era: null, // the look and the troops' icons (icons/<era>-<troop>.jpg); null: the `era` parameter
  // What the game calls things.
  // What the game calls things: see W below.
  // The global resources shown at the top (the first pays the troops), and the land held.
  resources: [{ id: 'Funds', icon: 'icons/funds.jpg' }, { id: 'Recruits', icon: 'icons/recruits.jpg' }],
  land: { label: 'Provinces', icon: 'icons/provinces.jpg' },
  // Maps the player can show, and views of what each place gave its holder on the last day.
  views: { Wealth: 'Wealth', Population: 'People', Happiness: 'Happiness', Elevation: 'Elevation', Fort: 'Castles', Violence: 'Battles' },
  yields: { Funds: 'Funds paid', Recruits: 'Recruits given' },
  // Decisions with an icon (icons/<name>.jpg; troops have theirs).
  decisionIcons: ['settle', 'govern', 'fortify', 'defend', 'attack', 'occupy', 'make_peace', 'declare_war', 'ally', 'feast', 'war_tax', 'disband'],
  basemap: null, // 'osm': OpenStreetMap under the places (a city); null: sea, and the terrain's colours
  diplomacy: true, // relations change (war, peace, alliances); false: everyone is at war
  maxZoom: 7.5, // how close the map comes when it frames a faction's land
  placeRows: null, // (card, h) -> extra rows of a place's card, as [label, value] pairs
  treasuryNote: 'Taxes and levies follow how happy each province is (see the Happiness view); troops are paid every morning, before anything else.',
}, window.GAME || {});
// A page may set only some words: the others stay Marches'.
const W = { realm: 'realm', realms: 'Realms', province: 'province', provinces: 'provinces', troops: 'troops', land: 'land',
  battles: 'Battles fought', destroyed: 'destroyed', fallen: 'fallen', unpaid: 'they lose health and plunder your provinces',
  treasury: 'Treasury', upkeep: 'Troop upkeep', gives: 'your land pays', emptying: 'the treasury is emptying',
  ...(window.GAME?.words || {}) };
const VIEWS = CONFIG.views;
const YIELD_VIEWS = CONFIG.yields;
const MONEY = CONFIG.resources[0].id;
const TWEEN_MS = 450;
const DECISION_ICONS = CONFIG.decisionIcons;
const cap = (t) => t.charAt(0).toUpperCase() + t.slice(1);

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

// ---------- several players ----------
// mode: 'solo' (one person), 'local' (several, taking turns on this device) or 'online' (through
// the game server: net.js). `mine` are the factions played on this device, `player` the one
// shown now; `humans` every faction a person plays. A day runs when every human has ended it
// (`ready`); online, players whose connection dropped (`away`) do not hold the day up.
let mode = 'solo';
let mine = [];
let humans = [];
const ready = new Set();
const away = new Set();
let net = null; // online: { conn, code, token, owner (opened the room) }
let settings = null; // { params, seed, players, pace }: what every browser starts the game from
let log = []; // every command since the start, in order (online: the server's log)
let started = false;

// The pace of an online game, set by the host when it creates the game: 'wait' (a day runs when
// everyone has ended it), 'limit-N' (... or after N seconds at most) or 'run-N' (a day every N
// seconds, sooner if everyone has ended it). The server's clock decides; pages count down.
const PACES = [
  ['wait', 'When everyone has ended the day'],
  ['limit-30', 'Everyone, or 30 seconds at most'],
  ['limit-60', 'Everyone, or 1 minute at most'],
  ['limit-120', 'Everyone, or 2 minutes at most'],
  ['run-5', 'Run: a day every 5 seconds'],
  ['run-10', 'Run: a day every 10 seconds'],
  ['run-20', 'Run: a day every 20 seconds'],
  ['run-30', 'Run: a day every 30 seconds'],
];
const paceName = (p) => (PACES.find(([v]) => v === p) || PACES[0])[1];
const paceSeconds = (p) => +((p || 'wait').split('-')[1] || 0);
let deadline = null; // when the server's clock runs the day (performance.now() time), or null
let paused = false;
let ticker = null;

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

async function files(params) {
  const out = {};
  for (const f of CONFIG.files(params)) {
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

let realms = [];

/** The factions to choose from: those that will play with the start screen's settings. */
function realmChoice() {
  const n = CONFIG.count ? +CONFIG.params()[CONFIG.count] : realms.length;
  const list = realms.slice(0, n);
  if (!list.some((r) => r.i === player)) player = list.length ? list[0].i : 0;
  $('realmChoice').innerHTML = '';
  const several = document.querySelector('input[name=mode]:checked')?.value === 'local';
  if (several && !localPick.size) localPick.add(player);
  list.forEach((r) => {
    const b = document.createElement('button');
    b.innerHTML = `${chipFor(r)}${r.name}`;
    if (several ? localPick.has(r.i) : r.i === player) b.classList.add('on');
    b.onclick = () => {
      if (several) {
        if (localPick.has(r.i) && localPick.size > 1) localPick.delete(r.i); else localPick.add(r.i);
        b.classList.toggle('on', localPick.has(r.i));
        return;
      }
      player = r.i;
      [...$('realmChoice').children].forEach((c) => c.classList.remove('on'));
      b.classList.add('on');
    };
    $('realmChoice').appendChild(b);
  });
  for (const f of [...localPick]) if (!list.some((r) => r.i === f)) localPick.delete(f);
}

function playersSection() {
  const heading = $('realmChoice').previousElementSibling;
  const box = document.createElement('div');
  box.innerHTML = `<h3>Players</h3>
    <div class="choices" id="modeChoice">
      <label><input type="radio" name="mode" value="solo" checked> Just me</label>
      <label><input type="radio" name="mode" value="local"> Several of us, on this device <span class="muted">(take turns; choose every ${W.realm} a person plays)</span></label>
      <label><input type="radio" name="mode" value="host"> Online: open a room <span class="muted">(friends join with its code)</span></label>
      <label><input type="radio" name="mode" value="join"> Online: join a room <input id="joinCode" placeholder="code" size="8" autocomplete="off"></label>
    </div>
    <div id="lobby" class="lobby" hidden></div>`;
  heading.before(box);
  document.querySelectorAll('input[name=mode]').forEach((i) => { i.onchange = modeChanged; });
  $('joinCode').oninput = () => { document.querySelector('input[name=mode][value=join]').checked = true; modeChanged(); };
  const code = new URLSearchParams(location.search).get('join');
  if (code) {
    $('joinCode').value = LilNet.cleanCode(code);
    document.querySelector('input[name=mode][value=join]').checked = true;
  }
}

/** What the start screen shows for each way of playing. */
function modeChanged() {
  const m = radio('mode');
  const joining = m === 'join';
  // A guest plays the host's game: its settings and its family come from the room.
  [...$('start').querySelectorAll('.choices')].forEach((c) => {
    if (c.id !== 'modeChoice') c.closest('.choices').style.display = joining ? 'none' : '';
  });
  [...$('start').querySelectorAll('h3')].forEach((h) => {
    if (h.textContent !== 'Players') h.style.display = joining ? 'none' : '';
  });
  $('realmChoice').style.display = joining ? 'none' : '';
  $('start').querySelector('.seed').style.display = joining ? 'none' : '';
  $('bStart').textContent = joining ? 'Join' : m === 'host' ? 'Open the room' : 'Start';
  if (m !== 'local') localPick.clear();
  realmChoice();
}

const localPick = new Set();

async function boot() {
  $('seed').value = 1 + Math.floor(Math.random() * 9999);
  playersSection();
  try {
    await loadEngine();
    const probe = call({ op: 'new', files: await files({ ...CONFIG.params(), ...CONFIG.probe }), params: CONFIG.probe, seed: 1 });
    realms = probe.factions.map((f, i) => ({ ...f, i })).filter((f) => f.holds);
    realmChoice();
    if (CONFIG.count) document.querySelectorAll(`input[name=${CONFIG.count}]`).forEach((i) => { i.onchange = realmChoice; });
    modeChanged();
    $('bStart').disabled = false;
  } catch (e) {
    $('startError').textContent = 'The engine did not start: ' + e.message;
  }
}

/** A fingerprint of the game's files, so that online players are sure to play the same game. */
function version(files) {
  let h = 0x811c9dc5;
  for (const k of Object.keys(files).sort()) {
    for (const ch of k + files[k]) h = Math.imul(h ^ ch.charCodeAt(0), 0x01000193) >>> 0;
  }
  return h.toString(16);
}

async function start() {
  $('startError').textContent = '';
  const m = radio('mode');
  if (m === 'join') { joinRoom(); return; }
  const params = CONFIG.params();
  const seed = +$('seed').value || 1;
  if (m === 'host') { openRoom(params, seed); return; }
  mode = m;
  mine = m === 'local' ? realms.filter((r) => localPick.has(r.i)).map((r) => r.i) : [player];
  await launch({ params, seed, players: mine }, []);
}

/** Starts the game every browser plays: from `settings`, replaying `commands` (to catch up). */
async function launch(s, commands) {
  $('bStart').disabled = true;
  $('bStart').textContent = 'Drawing the land…';
  try {
    settings = s;
    era = CONFIG.era || s.params.era || 'medieval';
    document.body.classList.toggle('future', era === 'future');
    info = call({ op: 'new', files: await files(s.params), params: s.params, seed: s.seed, players: s.players });
    if (CONFIG.cities && !cities) {
      try {
        const r = await fetch('game/' + CONFIG.cities);
        if (r.ok) cities = await r.json();
      } catch { /* no labels: the game is the same */ }
    }
    log = [];
    for (const c of commands) follow(c);
    humans = s.players.slice();
    for (const c of commands) {
      if (c.op === 'claim' && !humans.includes(c.faction)) humans.push(c.faction);
      if (c.op === 'release') humans = humans.filter((f) => f !== c.faction);
    }
    if (!mine.length || !mine.every((f) => humans.includes(f))) mine = mine.filter((f) => humans.includes(f));
    player = mine[0] ?? humans[0];
    ready.clear();
    st = call({ op: 'state', faction: player });
    info.geojson.features.forEach((f, i) => { f.properties.idx = i; });
    centres = info.geojson.features.map((f) => f.properties.centre);
    $('start').hidden = true;
    $('hud').hidden = false;
    $('over').hidden = true;
    selected = null;
    chosen = [];
    $('card').hidden = true;
    markers.forEach((m) => m.marker.remove());
    markers.clear();
    lastPos = new Map();
    started = true;
    // Days run one at a time when several people play.
    $('bWeek').hidden = mode !== 'solo';
    $('bPlay').hidden = mode !== 'solo';
    setupHud();
    drawMap();
    if (mode === 'local' && mine.length > 1) {
      turn(); // whose turn first; the help is under "How to play"
    } else {
      try {
        if (!localStorage.getItem(`${CONFIG.id}.seenHelp`)) { $('help').hidden = false; localStorage.setItem(`${CONFIG.id}.seenHelp`, '1'); }
      } catch { /* no storage: no reminder */ }
    }
  } catch (e) {
    $('startError').textContent = e.message;
  } finally {
    $('bStart').disabled = false;
    $('bStart').textContent = 'Start';
  }
}

// ---------- commands ----------
// Everything that changes the game is a command: a decision ('take'), an order ('order'),
// research, a person taking over or leaving a faction ('claim', 'release'), the end of a day
// ('step'). Online, commands run in one order in every browser: the server's.

/** Runs a command on this browser's engine. Throws if the engine refuses it. */
function exec(c) {
  if (c.op === 'step') call({ op: 'step', days: 1 });
  else call(c);
}

/** Alone or on one device: runs a command and records it, if the engine takes it. */
function apply(c) {
  exec(c);
  log.push(c);
}

/** Online: a command of the server's log. It is part of the game whether or not the engine
 * takes it (every browser refuses the same ones, and a refused command changes nothing), so it
 * is always recorded. Returns the refusal, if any. */
function follow(c) {
  log.push(c);
  try {
    exec(c);
    return null;
  } catch (e) {
    return e;
  }
}

/** This device wants a command done: alone, now; online, through the server. */
function act(c) {
  if (mode === 'online') {
    if (!send({ t: 'req', cmd: c })) { news('Not connected to the server: wait a moment.', 'bad'); return false; }
    return true;
  }
  try {
    apply(c);
  } catch (e) {
    news(e.message, 'bad');
    return false;
  }
  afterCommand(c);
  return true;
}

/** Brings the page up to date after a command (ours or another player's). */
function afterCommand(c) {
  if (c.op === 'claim' && !humans.includes(c.faction)) humans.push(c.faction);
  if (c.op === 'release') { humans = humans.filter((f) => f !== c.faction); ready.delete(c.faction); }
  st = call({ op: 'state', faction: player, map: viewIndex() });
  refresh(false);
}

/** The end of the day for the player shown: alone it runs; otherwise it waits for the others. */
function endDay() {
  if (busy || st.over) return;
  if (mode === 'solo') { advance(); return; }
  ready.add(player);
  if (mode === 'online') {
    send({ t: 'ready' });
    hud();
    return;
  }
  // On this device: the next person, or the day.
  const next = mine.find((f) => !ready.has(f) && st.alive[f]);
  if (next !== undefined) { player = next; turn(); return; }
  advance();
  player = mine.find((f) => st.alive[f]) ?? mine[0];
  st = call({ op: 'state', faction: player, map: viewIndex() });
  refresh(false);
  if (mine.length > 1) turn();
}

/** Runs one day here (alone, or on one device): every human is done. */
function advance() {
  busy = true;
  const before = st;
  apply({ op: 'step' });
  st = call({ op: 'state', faction: player, map: viewIndex() });
  busy = false;
  ready.clear();
  report(before, st);
  refresh(true);
  if (st.over || (mode === 'solo' && (!st.alive[player] || st.territory[player] === st.owner.length))) gameOver();
}

// ---------- the clock (online, kept by the server) ----------

/** The host pauses or resumes the clock for everyone. */
function togglePause() {
  if (mode === 'online' && net?.owner) send({ t: 'pause' });
}

/** Every quarter of a second: the countdown to the server's next day. */
function tick() {
  if (!ticker) {
    // A worker's timer keeps going in a background tab, where the page's own are slowed down.
    try {
      ticker = new Worker(URL.createObjectURL(new Blob(['setInterval(() => postMessage(0), 250)'], { type: 'text/javascript' })));
      ticker.onmessage = tick;
    } catch {
      ticker = setInterval(tick, 250);
    }
  }
  const el = $('clock');
  if (!el) return;
  if (!started || deadline === null || st?.over) {
    el.textContent = paused && started ? ' · paused' : '';
    return;
  }
  const left = Math.max(0, deadline - performance.now());
  el.textContent = ` · ${Math.ceil(left / 1000)} s`;
}

/** On a shared device: whose turn it is. */
function turn() {
  const f = info.factions[player];
  let el = $('turn');
  if (!el) {
    el = document.createElement('div');
    el.id = 'turn';
    el.className = 'overlay';
    document.body.appendChild(el);
  }
  el.innerHTML = `<div class="sheet"><h2>${chipFor(f)} ${f.name}</h2>
    <p>Day ${st.day}: it is ${f.name}'s turn. The others, look away.</p><button class="primary big" id="bTurn">I am ${f.name}</button></div>`;
  el.hidden = false;
  $('bTurn').onclick = () => { el.hidden = true; selected = null; chosen = []; setupHud(); refresh(false); frameOn(player); };
  st = call({ op: 'state', faction: player, map: viewIndex() });
}

/** Centres the map on a faction's land. */
function frameOn(f) {
  const own = st.owner.map((o, i) => (o === f ? i : -1)).filter((i) => i >= 0);
  if (!own.length || !map) return;
  const xs = own.map((i) => centres[i][0]), ys = own.map((i) => centres[i][1]);
  map.fitBounds([[Math.min(...xs), Math.min(...ys)], [Math.max(...xs), Math.max(...ys)]], { padding: framePadding(), maxZoom: CONFIG.maxZoom, duration: 600 });
}

// ---------- online ----------
// Through the game server (net.js): it orders everyone's commands and keeps them; this browser
// runs the game on them, like every other.

/** The families the start screen's settings let play (for the room's seats). */
function lobbyFamilies() {
  return realms.slice(0, CONFIG.count ? +CONFIG.params()[CONFIG.count] : realms.length).map((r) => ({ i: r.i, name: r.name, color: r.color }));
}

let inbox = Promise.resolve();

/** Connects to the server; messages are handled one at a time, in the order they came. */
async function connectServer() {
  return LilNet.connect((msg) => { inbox = inbox.then(() => serverMessage(msg)).catch((e) => console.error(e)); }, (s) => {
    if (s === 'lost') {
      if (started) news('Lost the connection to the server: trying again…', 'bad');
    } else {
      if (started) news('Connected again.', 'good');
      hello();
    }
  });
}

function send(msg) {
  return !!net?.conn && net.conn.send(msg);
}

/** Into the room (again): the server sends the lobby, or the game from the commands we lack. */
function hello() {
  send({ t: 'hello', game: CONFIG.id, code: net.code, token: net.token, have: started ? log.length : 0 });
}

async function openRoom(params, seed) {
  $('bStart').disabled = true;
  $('bStart').textContent = 'Opening the room…';
  try {
    const fs = await files(params);
    net = { token: LilNet.token(), code: null, owner: true };
    net.conn = await connectServer();
    mode = 'online';
    mine = [player];
    send({ t: 'create', game: CONFIG.id, token: net.token, params, seed, version: version(fs), families: lobbyFamilies(), seat: player, pace: 'wait' });
  } catch (e) {
    $('startError').textContent = e.message;
    $('bStart').disabled = false;
    $('bStart').textContent = 'Open the room';
  }
}

async function joinRoom() {
  const code = LilNet.cleanCode($('joinCode').value);
  if (!code) { $('startError').textContent = 'Type the room code.'; return; }
  $('bStart').disabled = true;
  $('bStart').textContent = 'Joining…';
  try {
    net = { token: LilNet.token(), code, owner: false };
    net.conn = await connectServer();
    mode = 'online';
    hello();
  } catch (e) {
    $('startError').textContent = e.message;
  } finally {
    $('bStart').disabled = false;
    $('bStart').textContent = 'Join';
  }
}

/** The room before the game (or for someone without a family once it is under way). */
function showLobby(msg) {
  const box = $('lobby');
  box.hidden = false;
  const link = `${location.origin}${location.pathname}?join=${msg.code}`;
  const taken = new Map(msg.taken.map(([f, who, gone]) => [f, { who, gone }]));
  const seated = msg.taken.some(([, who]) => who === 'you');
  const seat = (f) => {
    const t = taken.get(f.i);
    if (t) {
      const who = t.who === 'you' ? 'you' : t.who === 'host' ? 'the host' : 'a friend';
      // Someone who left (a closed tab, another device) can take their family back from here.
      return t.gone && !seated ? `${who} (away) <button data-f="${f.i}">Take over</button>` : who + (t.gone ? ' (away)' : '');
    }
    return seated ? 'the computer' : `<button data-f="${f.i}">Play</button>`;
  };
  const head = `<div class="code">Room <b>${msg.code}</b>${msg.started ? ' <span class="muted">(the game is under way)</span>' : ''}</div>`;
  const how = msg.owner
    ? `<div class="muted small">Friends open <a href="${link}" target="_blank">${link}</a>, or choose "join a room" and type the code.</div>`
    : `<div class="muted small">${seated ? 'The host begins the game.' : `Choose your ${W.realm}${msg.started ? ' (you take it over from the computer)' : ''}.`}</div>`;
  const pace = msg.owner && !msg.started
    ? `<div class="pace">Pace <select id="pace">${PACES.map(([v, t]) => `<option value="${v}"${v === msg.pace ? ' selected' : ''}>${t}</option>`).join('')}</select></div>`
    : `<div class="pace">Pace: <b>${paceName(msg.pace)}</b></div>`;
  box.innerHTML = `${head}${how}<div class="seats">${msg.families.map((f) => `<div>${chipFor(f)}${f.name}<span class="n">${seat(f)}</span></div>`).join('')}</div>${pace}`
    + (msg.owner && !msg.started ? '<button id="bBegin" class="primary big">Begin</button>' : '');
  if ($('bBegin')) $('bBegin').onclick = () => { $('bBegin').disabled = true; send({ t: 'begin' }); };
  if ($('pace')) $('pace').onchange = (e) => send({ t: 'pace', pace: e.target.value });
  box.querySelectorAll('button[data-f]').forEach((b) => {
    b.onclick = () => { send({ t: 'pick', faction: +b.dataset.f }); box.querySelectorAll('button[data-f]').forEach((x) => { x.disabled = true; }); };
  });
  $('bStart').hidden = true;
}

/** Commands missed while the line was down: run them, as they came. */
function catchUp(commands) {
  const before = st;
  for (const c of commands) follow(c);
  for (const c of commands) {
    if (c.op === 'claim' && !humans.includes(c.faction)) humans.push(c.faction);
    if (c.op === 'release') humans = humans.filter((f) => f !== c.faction);
  }
  st = call({ op: 'state', faction: player, map: viewIndex() });
  report(before, st);
  refresh(true);
  if (st.over) gameOver();
}

async function serverMessage(msg) {
  if (msg.t === 'lobby') {
    net.code = msg.code;
    net.owner = msg.owner;
    if (started) return;
    if (!msg.owner) {
      const fs = await files(msg.params);
      if (version(fs) !== msg.version) {
        $('startError').textContent = 'This room plays another version of the game: reload the page (both of you).';
        return;
      }
    }
    showLobby(msg);
  } else if (msg.t === 'start') {
    net.code = msg.code;
    net.owner = msg.owner;
    if (started && msg.from > 0 && msg.from === log.length) {
      catchUp(msg.commands);
      return;
    }
    mine = [msg.faction];
    await launch(msg.settings, msg.commands);
    $('lobby').hidden = true;
    $('bStart').hidden = false;
  } else if (msg.t === 'cmd') {
    if (!started) return;
    if (msg.seq !== log.length + 1) { send({ t: 'resync' }); return; }
    const refused = follow(msg.cmd);
    if (refused && mine.includes(msg.cmd.faction)) news(refused.message, 'bad');
    afterCommand(msg.cmd);
  } else if (msg.t === 'step') {
    if (!started) return;
    if (msg.seq !== log.length + 1) { send({ t: 'resync' }); return; }
    const before = st;
    busy = true;
    follow({ op: 'step' });
    st = call({ op: 'state', faction: player, map: viewIndex() });
    busy = false;
    ready.clear();
    send({ t: 'fp', seq: log.length, fp: st.fingerprint });
    report(before, st);
    refresh(true);
    if (st.over) { send({ t: 'over' }); gameOver(); } else if (!st.alive[player]) send({ t: 'ready' }); // the fallen do not hold the day up
  } else if (msg.t === 'away') {
    away.clear();
    msg.away.forEach((f) => away.add(f));
    ready.clear();
    msg.ready.forEach((f) => ready.add(f));
    if (started) hud();
  } else if (msg.t === 'clock') {
    paused = msg.paused;
    deadline = msg.ms === null ? null : performance.now() + msg.ms;
    tick();
    if (started) players();
  } else if (msg.t === 'note') {
    if (started) news(msg.msg, msg.kind);
  } else if (msg.t === 'err') {
    if (started) news(msg.msg, 'bad'); else $('startError').textContent = msg.msg;
    $('lobby').querySelectorAll('button[data-f]').forEach((x) => { x.disabled = false; });
    if ($('bBegin')) $('bBegin').disabled = false;
    if (!net?.code) { $('bStart').disabled = false; $('bStart').textContent = radio('mode') === 'join' ? 'Join' : 'Open the room'; }
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
let cities = null; // the game's named cities, when it has them

/** The cities' labels (a dot and a name; MapLibre leaves out those that would overlap). */
function cityLayers(future) {
  if (!cities) return [];
  const ink = future ? '#e7ecf3' : '#231d16', halo = future ? 'rgba(12, 22, 38, .85)' : 'rgba(250, 246, 236, .9)';
  return [
    { id: 'city-dot', type: 'circle', source: 'cities', paint: { 'circle-radius': ['case', ['get', 'big'], 3.5, 2.5], 'circle-color': ink, 'circle-stroke-color': halo, 'circle-stroke-width': 1.2 } },
    { id: 'city-name', type: 'symbol', source: 'cities',
      layout: { 'text-field': ['get', 'name'], 'text-font': ['case', ['get', 'big'], ['literal', ['Noto Sans Bold']], ['literal', ['Noto Sans Regular']]],
        'text-size': ['interpolate', ['linear'], ['zoom'], 5, ['case', ['get', 'big'], 12, 10], 9, ['case', ['get', 'big'], 16, 13]],
        'text-offset': [0, 0.9], 'text-anchor': 'top', 'symbol-sort-key': ['case', ['get', 'big'], 0, ['==', ['get', 'kind'], 'capital'], 1, 2], 'text-padding': 3 },
      paint: { 'text-color': ink, 'text-halo-color': halo, 'text-halo-width': 1.4 } },
  ];
}

/** The main city of a place, by the cities' `province` (for maps of provinces). */
function cityOf(name) {
  if (!cities) return null;
  const c = cities.features.find((f) => f.properties.province === name && f.properties.kind === 'capital');
  return c && c.properties.name !== name ? c.properties.name : null;
}

/** Under the places: a street map (a city), or the sea and the terrain's colours (a land). */
function groundLayers(future) {
  if (CONFIG.basemap === 'osm') {
    return [
      { id: 'ground', type: 'background', paint: { 'background-color': '#e9e6df' } },
      { id: 'osm', type: 'raster', source: 'osm', paint: { 'raster-saturation': -0.7, 'raster-opacity': 0.9 } },
    ];
  }
  return [
    { id: 'ground', type: 'background', paint: { 'background-color': future ? '#0c1626' : '#9fbfcc' } },
    { id: 'terrain', type: 'fill', source: 'places', paint: { 'fill-color': ['get', 'tfill'], 'fill-opacity': future ? 0.75 : 0.95 } },
  ];
}

function drawMap() {
  const future = era === 'future';
  const osm = CONFIG.basemap === 'osm';
  const style = {
    version: 8,
    sources: {
      ...(osm ? { osm: { type: 'raster', tiles: ['https://tile.openstreetmap.org/{z}/{x}/{y}.png'], tileSize: 256, maxzoom: 19,
        attribution: '© <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors' } } : {}),
      places: { type: 'geojson', data: info.geojson, attribution: ['Lilienstern', info.name, info.credit].filter(Boolean).join(' · ') },
      routes: { type: 'geojson', data: empty() },
      preview: { type: 'geojson', data: empty() },
      ...(cities ? { cities: { type: 'geojson', data: cities } } : {}),
    },
    ...(cities ? { glyphs: 'https://tiles.openfreemap.org/fonts/{fontstack}/{range}.pbf' } : {}),
    layers: [
      // Water: the sea around the land, and the river through it (or a street map).
      ...groundLayers(future),
      { id: 'fill', type: 'fill', source: 'places', paint: { 'fill-color': ['get', 'fill'], 'fill-opacity': ['get', 'fill-opacity'] } },
      { id: 'line', type: 'line', source: 'places', paint: { 'line-color': future ? '#000' : osm ? '#333' : '#5a4b35', 'line-width': osm ? 0.4 : 0.6, 'line-opacity': 0.55 } },
      { id: 'mine', type: 'line', source: 'places', filter: ['==', ['get', 'mine'], 1], paint: { 'line-color': future ? '#fff' : '#1b1209', 'line-width': 1.4, 'line-opacity': 0.8 } },
      { id: 'selected', type: 'line', source: 'places', filter: ['==', ['get', 'idx'], -1], paint: { 'line-color': '#ffd54a', 'line-width': 3.5 } },
      ...cityLayers(future),
      { id: 'routes', type: 'line', source: 'routes', paint: { 'line-color': ['get', 'color'], 'line-width': 2.5, 'line-dasharray': [2, 1.5], 'line-opacity': 0.9 } },
      { id: 'preview', type: 'line', source: 'preview', paint: { 'line-color': '#ffd54a', 'line-width': 3, 'line-dasharray': [1, 1] } },
    ],
  };
  if (map) map.remove();
  map = new maplibregl.Map({ container: 'map', style, bounds: bounds(info.geojson), fitBoundsOptions: { padding: framePadding() }, attributionControl: false });
  map.addControl(new maplibregl.AttributionControl({ compact: true }), 'bottom-right');
  if (!narrow()) map.addControl(new maplibregl.NavigationControl({ showCompass: false }), 'top-right');
  map.on('load', () => {
    seatCrests();
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
  const values = VIEWS[view] ? st.values || [] : yieldView() ? st.yields[yieldView()] || [] : null;
  info.geojson.features.forEach((f, i) => {
    const o = st.owner[i];
    const fac = o >= 0 ? info.factions[o] : null;
    f.properties.tfill = terrainColor(i);
    f.properties.fill = fac ? fac.color : '#000000';
    f.properties['fill-opacity'] = view === 'terrain' || !fac ? 0 : CONFIG.basemap === 'osm' ? (o === player ? 0.7 : 0.5) : o === player ? 0.68 : 0.58;
    f.properties.mine = o === player ? 1 : 0;
    if (values) f.properties.v = values[i] || 0;
  });
  map.getSource('places').setData(info.geojson);
  if (values && view === 'Happiness') {
    // Red where people revolt, yellow when content (50), green when happy.
    map.setPaintProperty('fill', 'fill-color', ['interpolate', ['linear'], ['get', 'v'], 0, '#a50026', 15, '#d73027', 35, '#fdae61', 50, '#ffffbf', 75, '#a6d96a', 100, '#1a9850']);
    map.setPaintProperty('fill', 'fill-opacity', 0.75);
  } else if (values) {
    const max = Math.max(view === 'Fort' ? 3 : yieldView() ? 0.01 : 1, ...values);
    map.setPaintProperty('fill', 'fill-color', ['interpolate', ['linear'], ['get', 'v'], 0, '#ffffcc', max / 2, '#fd8d3c', max, '#bd0026']);
    map.setPaintProperty('fill', 'fill-opacity', ['interpolate', ['linear'], ['get', 'v'], 0, 0.0, max * 0.04, 0.75]);
  } else {
    map.setPaintProperty('fill', 'fill-color', ['get', 'fill']);
    map.setPaintProperty('fill', 'fill-opacity', ['get', 'fill-opacity']);
  }
  map.setFilter('selected', ['==', ['get', 'idx'], selected ?? -1]);
  routes();
  paintSeats();
}

// ---------- icons ----------

const troopIcon = (k) => `icons/${era}-${info.troopKinds[k].id}.jpg`;

function decisionIcon(name) {
  if (name.startsWith('raise_')) {
    const k = info.troopKinds.findIndex((t) => `raise_${t.id}` === name);
    return k >= 0 ? troopIcon(k) : null;
  }
  return DECISION_ICONS.includes(name) ? `icons/${name}.jpg` : null;
}

const img = (src, cls = 'ico') => (src ? `<img class="${cls}" src="${src}" alt="">` : '');

/** A faction's crest, when the page has one (Italia's families), else its colour. */
function chipFor(f) {
  const src = CONFIG.crests && CONFIG.crests[f.name];
  return src ? `<img class="crest" src="${src}" alt="" title="${f.name}">` : `<i class="chip" style="background:${f.color}"></i>`;
}

/** The kind that stands for a stack: the most common, the dearest on a tie. */
function leadKind(kinds) {
  const n = {};
  kinds.forEach((k) => { n[k] = (n[k] || 0) + 1; });
  return +Object.keys(n).sort((a, b) => n[b] - n[a] || b - a)[0];
}

// ---------- crests at the families' seats ----------

let seats = [];
function seatCrests() {
  seats.forEach((s) => s.marker.remove());
  seats = [];
  if (!CONFIG.crests || !info.hqs) return;
  info.hqs.forEach((p, f) => {
    const src = CONFIG.crests[info.factions[f].name];
    if (p == null || !src) return;
    const el = document.createElement('div');
    el.className = 'seat-crest';
    el.style.backgroundImage = `url("${src}")`;
    el.title = `${info.factions[f].name}: ${placeName(p)}`;
    seats.push({ f, p, el, marker: new maplibregl.Marker({ element: el, offset: [0, -30] }).setLngLat(centres[p]).addTo(map) });
  });
}

/** A family that has lost its seat: its crest fades. */
function paintSeats() {
  seats.forEach((s) => { s.el.style.opacity = st.owner[s.p] === s.f ? 1 : 0.35; });
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
      el.className = 'troop';
      el.addEventListener('click', (e) => { e.stopPropagation(); clickStack(m.stack); });
      m = { el, marker: new maplibregl.Marker({ element: el }), at: null };
      markers.set(s.id, m);
    }
    m.stack = s;
    const kinds = s.list.map((t) => t.k);
    const hp = s.list.reduce((a, t) => a + t.hp, 0) / s.list.length;
    const mine = s.f === player;
    // Toggle only our classes: MapLibre's own (maplibregl-marker...) position the marker.
    m.el.classList.toggle('mine', mine);
    m.el.classList.toggle('selected', mine && s.list.some((t) => chosen.includes(t.id)));
    m.el.style.borderColor = info.factions[s.f].color;
    m.el.style.zIndex = mine ? 2 : 1; // your troops stay clickable where stacks overlap
    m.el.style.backgroundImage = `url(${troopIcon(leadKind(kinds))})`;
    m.el.innerHTML = `${s.list.length > 1 ? `<small style="background:${info.factions[s.f].color}">${s.list.length}</small>` : ''}<span class="hpbar"><i style="width:${hp}%"></i></span>`;
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
      const r = call({ op: 'route', troop: id, to, faction: player });
      if (act({ op: 'order', faction: player, troop: id, to })) {
        days = Math.max(days, r.days);
        ok++;
      }
    } catch (e) {
      why = e.message;
    }
  }
  if (ok) news(`${ok > 1 ? `${ok} ${W.troops} are on their way` : `On the way`} to ${placeName(to)}${days ? `: ${days} day${days > 1 ? 's' : ''}` : ''}.`);
  else news(why || 'They cannot get there.', 'bad');
  choose([]);
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
      const r = call({ op: 'route', troop: chosen[0], to: p, faction: player });
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
  const crestSrc = CONFIG.crests && CONFIG.crests[me.name];
  $('realmColor').classList.toggle('has-crest', !!crestSrc);
  $('realmColor').style.backgroundImage = crestSrc ? `url("${crestSrc}")` : '';
  $('realmName').textContent = me.name;
  $('days').textContent = info.days;
  $('view').innerHTML = '';
  $('view').add(new Option(W.realms, 'realms'));
  if (info.terrains.length > 1) $('view').add(new Option('Terrain', 'terrain'));
  Object.entries(VIEWS).filter(([id]) => info.maps.includes(id)).forEach(([id, label]) => $('view').add(new Option(label, id)));
  Object.entries(YIELD_VIEWS).forEach(([id, label]) => $('view').add(new Option(`${label} (a day)`, `yield:${id}`)));
  if (![...$('view').options].some((o) => o.value === view)) view = 'realms';
  $('view').value = view;
  stats();
  hud();
}

/** The resources at the top of the panel (click one: where it comes from), and the land held. */
function stats() {
  const box = document.querySelector('#hud .stats');
  box.innerHTML = CONFIG.resources.map((r) => `<div class="money" data-res="${r.id}" title="Where it comes from: click">${img(r.icon, '')}<span class="lbl">${r.label || r.id}</span><b></b><span class="delta"></span></div>`).join('')
    + `<div>${img(CONFIG.land.icon, '')}<span class="lbl">${CONFIG.land.label}</span><b id="land"></b></div>`;
  box.querySelectorAll('.money').forEach((el) => { el.onclick = treasury; });
}

function fmt(v) {
  return v >= 1000 ? Math.round(v).toLocaleString('en') : (Math.round(v * 10) / 10).toString();
}

/** Like fmt, with two decimals below 1 (a province's taxes, a day). */
function fmt2(v) {
  return Math.abs(v) < 1 ? (Math.round(v * 100) / 100).toString() : fmt(v);
}

function hud() {
  const me = info.factions[player];
  const acct = st.accounts[player] || {};
  $('day').textContent = `Day ${st.day}`;
  const eco = st.economy;
  const net = (id) => {
    const r = eco && eco.resources.find((x) => x.id === id);
    return r ? r.day.reduce((a, x) => a + x.amount, 0) : 0;
  };
  document.querySelectorAll('#hud .stats .money').forEach((el) => {
    const id = el.dataset.res;
    el.querySelector('b').textContent = fmt(acct[id] || 0);
    const v = net(id);
    const d = el.querySelector('.delta');
    d.textContent = `${v >= 0 ? '+' : '−'}${fmt2(Math.abs(v))}`;
    d.className = `delta ${v < -0.05 ? 'bad' : v > 0.05 ? 'good' : ''}`;
  });
  const funds = eco && eco.resources.find((x) => x.id === MONEY);
  const warn = [];
  if (eco && eco.unpaid) warn.push(`Your ${W.troops} are not paid: ${W.unpaid}.`);
  else if (funds && funds.upkeep > funds.income + 0.05) warn.push(`Your ${W.troops} cost ${fmt(funds.upkeep)} a day and ${W.gives} ${fmt(funds.income)}: ${W.emptying}.`);
  $('ecoWarn').hidden = !warn.length;
  $('ecoWarn').textContent = warn.join(' ');
  $('land').textContent = st.territory[player];
  const realms = info.factions.filter((f) => f.holds).length;
  $('rank').textContent = st.alive[player] ? `#${st.rank} of ${realms}` : W.fallen;
  const left = st.actions_left ?? 0;
  $('moves').innerHTML = Array.from({ length: me.actions }, (_, i) => `<span class="dot${i < me.actions - left ? ' used' : ''}"></span>`).join('');
  const mine = st.troops.filter((t) => t.f === player);
  const idle = mine.filter((t) => !t.path || !t.path.length);
  const counts = info.troopKinds.map((k, i) => ({ k, n: mine.filter((t) => t.k === i).length })).filter((x) => x.n);
  $('army').innerHTML = `<span class="lbl">Your ${W.troops}: ${mine.length} (${idle.length} without orders)</span>`
    + `<div class="kinds">${counts.map((x) => `<span title="${x.k.description}">${img(troopIcon(info.troopKinds.indexOf(x.k)))}${x.k.name} × ${x.n}</span>`).join('')}</div>`;
  if (idle.length) {
    const b = document.createElement('button');
    b.textContent = `Select the ${W.troops} without orders`;
    b.onclick = () => choose(idle.map((t) => t.id));
    $('army').appendChild(b);
  }
  ownOptions();
  players();
  researchPanel();
  legend();
}

/** Decisions that are not about a place (Gangland: hire muscle). */
function ownOptions() {
  let box = $('own');
  if (!box) {
    box = document.createElement('div');
    box.id = 'own';
    $('army').before(box);
  }
  const own = st.alive[player] ? call({ op: 'options', faction: player }) : [];
  box.innerHTML = '';
  box.hidden = !own.length;
  own.forEach((o) => box.appendChild(optionButton(o, null)));
}

// ---------- treasury ----------

/** Where the realm's funds and recruits came from, and went: on the last day and since the start. */
function treasury() {
  let el = $('treasury');
  if (!el) {
    el = document.createElement('div');
    el.id = 'treasury';
    el.className = 'overlay';
    document.body.appendChild(el);
    el.onclick = (e) => { if (e.target === el) el.hidden = true; };
  }
  const eco = st.economy;
  if (!eco) return;
  const what = (x) => ({ upkeep: W.troops, event: W.provinces })[x.kind] || '';
  const row = (x, perDay) => {
    const n = !perDay || !x.n ? '' : x.kind === 'decision' || x.kind === 'unit' ? `<span class="muted">× ${x.n}</span>` : what(x) && x.n > 1 ? `<span class="muted">${x.n} ${what(x)}</span>` : '';
    return `<tr><td>${x.kind === 'upkeep' ? W.upkeep : x.label} ${n}</td><td class="num ${x.amount < 0 ? 'bad' : 'good'}">${x.amount > 0 ? '+' : '−'}${fmt2(Math.abs(x.amount))}</td></tr>`;
  };
  const table = (rows, perDay) => rows.length ? `<table class="ledger">${rows.map((x) => row(x, perDay)).join('')}
      <tr class="total"><td>${perDay ? 'Change on the day' : 'Change since the start'}</td><td class="num">${(() => { const t = rows.reduce((a, x) => a + x.amount, 0); return `${t >= 0 ? '+' : '−'}${fmt(Math.abs(t))}`; })()}</td></tr></table>` : '<div class="muted">Nothing yet.</div>';
  const blocks = eco.resources.map((r) => {
    const name = r.id === info.research ? `${r.id} (research)` : r.id;
    const bal = r.upkeep ? `<p class="muted small">${cap(W.gives)} ${fmt(r.income)} a day; your ${W.troops} cost ${fmt(r.upkeep)} a day: <b class="${r.income - r.upkeep < 0 ? 'bad' : 'good'}">${r.income - r.upkeep >= 0 ? '+' : '−'}${fmt(Math.abs(r.income - r.upkeep))}</b> before any decision.</p>` : '';
    return `<div class="res"><h3>${name}</h3>${bal}<div class="cols"><div><h4>Day ${Math.max(0, st.day - 1)}</h4>${table(r.day, true)}</div><div><h4>Since the start</h4>${table(r.game, false)}</div></div></div>`;
  }).join('');
  el.innerHTML = `<div class="sheet wide"><button class="close" id="treasuryClose" aria-label="Close">×</button>
    <h2>${W.treasury}</h2><p class="muted">Every gain and every expense, by where it came from. ${CONFIG.treasuryNote}</p>
    ${eco.unpaid ? `<p class="bad"><b>Your ${W.troops} are not paid.</b> ${CONFIG.unpaidNote || 'They lose health every day, and plunder your provinces, whose people grow angry: taxes fall, and below 15 they revolt. Disband troops, levy a war tax, or win land.'}</p>` : ''}
    ${blocks}</div>`;
  el.hidden = false;
  $('treasuryClose').onclick = () => { el.hidden = true; };
}

// ---------- research ----------

/** What a tech does, in words. */
function techEffects(t) {
  const who = t.troops.length ? t.troops.join(', ') : 'all troops';
  const out = [];
  if (t.damage) out.push(`${who}: ${t.damage > 0 ? '+' : ''}${t.damage}% damage`);
  if (t.defense) out.push(`${who}: +${t.defense} defence`);
  if (t.speed) out.push(`${who}: +${t.speed}% speed`);
  if (t.occupy) out.push(`${who}: +${t.occupy}% occupation`);
  if (t.upkeep) out.push(`${who}: ${t.upkeep}% upkeep`);
  if (t.unlocks.length) out.push(`unlocks ${t.unlocks.join(', ')}`);
  return out.join('; ');
}

function researchPanel() {
  let box = $('research');
  if (!box) {
    box = document.createElement('div');
    box.id = 'research';
    $('army').before(box);
  }
  if (!info.techs || !info.techs.length || st.known == null) { box.hidden = true; return; }
  box.hidden = false;
  const r = st.researching;
  const known = st.known.length;
  if (r == null) {
    box.innerHTML = `<span class="lbl">Research · ${known} of ${info.techs.length} known</span>`
      + (st.researchable.length ? '<button class="primary" id="bTechs">Choose what to research</button>' : '<div class="muted">Everything is known.</div>');
  } else {
    const t = info.techs[r];
    const pct = Math.min(100, (100 * st.progress) / t.cost);
    box.innerHTML = `<span class="lbl">Research · ${known} of ${info.techs.length} known</span>
      <div class="bar"><span>${t.name}</span><div class="track"><div class="fill" style="width:${pct}%"></div></div><span class="v">${fmt(st.progress)}/${fmt(t.cost)}</span></div>
      <button id="bTechs">Tech tree</button>`;
  }
  if ($('bTechs')) $('bTechs').onclick = techTree;
}

/** The tech tree: columns by how many techs lead to each; click one that can be researched. */
function techTree() {
  let el = $('techs');
  if (!el) {
    el = document.createElement('div');
    el.id = 'techs';
    el.className = 'overlay';
    document.body.appendChild(el);
    el.onclick = (e) => { if (e.target === el) el.hidden = true; };
  }
  const depth = [];
  const d = (i) => (depth[i] ??= info.techs[i].requires.length ? 1 + Math.max(...info.techs[i].requires.map(d)) : 0);
  info.techs.forEach((_, i) => d(i));
  const cols = [];
  info.techs.forEach((t, i) => { (cols[depth[i]] ||= []).push(i); });
  const known = new Set(st.known), open = new Set(st.researchable);
  const card = (i) => {
    const t = info.techs[i];
    const state = known.has(i) ? 'known' : st.researching === i ? 'current' : open.has(i) ? 'open' : 'locked';
    const needs = t.requires.length ? `<div class="needs">needs ${t.requires.map((r) => info.techs[r].name).join(', ')}</div>` : '';
    const what = state === 'known' ? 'known' : state === 'current' ? `researching · ${fmt(st.progress)}/${fmt(t.cost)}` : `${fmt(t.cost)} ${(info.research || 'points').toLowerCase()}`;
    return `<button class="tech ${state}" data-t="${i}" ${state === 'open' ? '' : 'disabled'}><b>${t.name}</b><span class="what">${what}</span>
      <span class="fx">${techEffects(t) || t.description}</span>${needs}</button>`;
  };
  el.innerHTML = `<div class="sheet wide"><button class="close" id="techsClose" aria-label="Close">×</button>
    <h2>Tech tree</h2><p class="muted">Every day your ${(info.research || 'research').toLowerCase()} goes into the tech you research; the one you click next keeps what was put in.</p>
    <div class="tree">${cols.map((c) => `<div class="col">${c.map(card).join('')}</div>`).join('')}</div></div>`;
  el.hidden = false;
  $('techsClose').onclick = () => { el.hidden = true; };
  el.querySelectorAll('button.tech.open').forEach((b) => {
    b.onclick = () => {
      if (act({ op: 'research', faction: player, tech: +b.dataset.t })) {
        news(`Researching ${info.techs[+b.dataset.t].name}.`);
        el.hidden = true;
      }
    };
  });
}

/** Who plays, and whom the day waits for. */
function players() {
  let box = $('players');
  if (!box) {
    box = document.createElement('div');
    box.id = 'players';
    $('army').after(box);
  }
  if (mode === 'solo') { box.hidden = true; return; }
  box.hidden = false;
  const rows = humans.map((f) => {
    const fac = info.factions[f];
    const state = !st.alive[f] ? 'fallen' : away.has(f) ? 'away' : ready.has(f) ? 'done' : 'thinking…';
    const who = mine.includes(f) ? (mode === 'local' ? 'here' : 'you') : mode === 'online' ? 'friend' : '';
    const ai = mode === 'online' && net?.owner && away.has(f) ? ` <button data-ai="${f}">to the computer</button>` : '';
    return `<div class="${f === player ? 'me' : ''}">${chipFor(fac)}${fac.name} <span class="muted">${who}</span><span class="n">${state}${ai}</span></div>`;
  }).join('');
  const room = net?.code ? `<span class="muted">· room ${net.code}</span>` : '';
  const pace = settings?.pace && settings.pace !== 'wait' ? `<div class="muted small">${paceName(settings.pace)}${mode === 'online' && net?.owner ? ` <button id="bPause">${paused ? 'Resume' : 'Pause'}</button>` : paused ? ' · paused' : ''}</div>` : '';
  box.innerHTML = `<span class="lbl">Players ${room}</span>${rows}${pace}`;
  if ($('bPause')) $('bPause').onclick = togglePause;
  if (!$('clock')) {
    const c = document.createElement('span');
    c.id = 'clock';
    c.className = 'muted';
    $('days').parentElement.after(c);
  }
  box.querySelectorAll('button[data-ai]').forEach((b) => {
    b.onclick = () => send({ t: 'release', faction: +b.dataset.ai });
  });
  $('bDay').textContent = mode === 'solo' ? 'End day' : ready.has(player) ? 'Waiting…' : 'End day';
  $('bDay').disabled = mode !== 'solo' && mode !== 'local' && ready.has(player);
}

const yieldView = () => (view.startsWith('yield:') ? view.slice(6) : null);

function legend() {
  if (view === 'Happiness') {
    $('legend').innerHTML = '<div><span>revolt</span><span class="ramp mood"></span><span>happy</span></div><div class="muted small">Taxes and levies follow it: a province at 50 pays in full.</div>';
    return;
  }
  if (yieldView()) {
    const max = Math.max(0.01, ...(st.yields[yieldView()] || [0]));
    $('legend').innerHTML = `<div><span>0</span><span class="ramp"></span><span>${fmt(max)}</span></div><div class="muted small">What each province gave its lord yesterday.</div>`;
    return;
  }
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
    const rel = r.i === player || !CONFIG.diplomacy ? '' : ` <span class="rel ${st.relations[r.i]}">${st.relations[r.i]}</span>`;
    return `<div class="${r.i === player ? 'me' : ''}">${chipFor(r.f)}${r.f.name}${st.alive[r.i] ? '' : ` <span class="muted">(${W.fallen})</span>`}${rel}<span class="n">${r.n} · ${army[r.i]}⚑</span></div>`;
  }).join('') + `<div class="muted"><i class="chip" style="background:transparent"></i>nobody<span class="n">${st.owner.filter((o) => o < 0).length}</span></div>`;
}

function costText(o) {
  return o.costs.map((c) => `${fmt(c.amount)} ${c.resource.toLowerCase()}`).join(', ');
}

function optionButton(o, place) {
  const row = document.createElement('div');
  row.className = 'opt';
  const b = document.createElement('button');
  b.innerHTML = img(decisionIcon(o.name));
  b.append(o.raise ? `Raise ${o.raise}` : o.label);
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
  const c = call({ op: 'place', place: selected, faction: player });
  $('card').hidden = false;
  $('cardName').textContent = c.name;
  const owner = c.owner !== null ? info.factions[c.owner] : null;
  const rel = owner && c.owner !== player && CONFIG.diplomacy ? ` <span class="rel ${st.relations[c.owner]}">${st.relations[c.owner]}</span>` : '';
  $('cardOwner').innerHTML = owner ? `Held by <b>${owner.name}</b>${c.owner === player ? ' (you)' : ''}${rel}` : 'Nobody holds it';
  const rows = [
    ...(CONFIG.placeRows ? CONFIG.placeRows(c, { fmt, fmt2 }) : landRows(c)),
    Object.keys(c.yields).length && ['Paid yesterday', Object.entries(c.yields).map(([r, v]) => `${fmt2(v)} ${r.toLowerCase()}`).join(', ')],
    c.upkeep && [`Your ${W.troops}' upkeep`, `${fmt(c.upkeep)} a day`],
  ];
  const kv = rows.filter(Boolean).map(([k, v, cls]) => `<span>${k}</span><span class="${cls || ''}">${v}</span>`).join('');
  const top = Math.max(40, ...c.presence.map((p) => p.presence));
  const bars = c.presence.sort((a, b) => b.presence - a.presence).map((p) => {
    const f = info.factions[p.faction];
    return `<div class="bar">${chipFor(f)}<span>${f.name}</span><div class="track"><div class="fill" style="width:${100 * p.presence / top}%;background:${f.color}"></div></div><span class="v">${fmt(p.presence)}</span></div>`;
  }).join('') || `<div class="muted">No ${W.realm} has a hold here.</div>`;
  $('cardBody').innerHTML = `<div class="kv">${kv}</div><h3>Hold</h3>${bars}<h3>${cap(W.troops)} here</h3><div class="troops" id="cardTroops"></div>`;
  const box = $('cardTroops');
  if (!c.troops.length) box.innerHTML = '<div class="muted">None.</div>';
  c.troops.forEach((t) => {
    const k = info.troopKinds[t.kind];
    const row = document.createElement('div');
    row.innerHTML = `${chipFor(info.factions[t.faction])}${img(troopIcon(t.kind))}${k.name}<span class="hp"><i style="width:${t.hp}%"></i></span>`;
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

/** A province's rows on its card, in Marches and Italia. */
function landRows(c) {
  const ter = c.terrain !== null ? info.terrains[c.terrain] : null;
  return [
    cityOf(c.name) && ['Capital', cityOf(c.name)],
    ter && ['Terrain', `${ter.name}${ter.defense ? ` (+${ter.defense})` : ''}`],
    ['Castles', fmt(c.maps.Fort || 0)],
    ['Wealth', fmt(c.maps.Wealth || 0)],
    'Population' in c.maps && ['People (1871)', Math.round(c.maps.Population * 1000).toLocaleString('en')],
    'Elevation' in c.maps && ['Elevation', `${Math.round(c.maps.Elevation)} m`],
    'Happiness' in c.maps && ['Mood', `${mood(c.maps.Happiness)} (${Math.round(c.maps.Happiness)})`, c.maps.Happiness < 15 ? 'bad' : c.maps.Happiness >= 50 ? 'good' : ''],
  ];
}

/** A province's mood in words (people.lil: 50 is content, below 15 it revolts). */
function mood(h) {
  return h < 15 ? 'in revolt' : h < 30 ? 'angry' : h < 45 ? 'restless' : h < 60 ? 'content' : 'happy';
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
  const before = st.troops.length;
  if (!act({ op: 'take', faction: player, decision: o.decision, place })) return;
  if (o.raise && mode !== 'online' && st.troops.length > before) news(`${o.raise} raised in ${placeName(place)}.`, 'good');
}

/** Alone: several days at once. */
function days(n) {
  for (let i = 0; i < n && !st.over; i++) advance();
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
  if (gone) news(`${gone} of your ${W.troops} ${gone > 1 ? 'were' : 'was'} ${W.destroyed}.`, 'bad');
  const arrived = a.troops.filter((t) => t.f === player && t.path && t.path.length && b.troops.some((u) => u.id === t.id && (!u.path || !u.path.length)));
  if (arrived.length) news(`${arrived.length > 1 ? `${arrived.length} ${W.troops} have` : `One of your ${W.troops} has`} arrived.`);
  if (CONFIG.diplomacy) (b.relations || []).forEach((r, i) => {
    const was = a.relations[i];
    if (i === player || r === was) return;
    const name = info.factions[i].name;
    if (r === 'war') news(`${name} is at war with you.`, 'bad');
    else if (r === 'peace') news(`Peace with ${name}.`, 'good');
    else if (r === 'ally') news(`${name} is your ally.`, 'good');
  });
  if (a.alive[player] && !b.alive[player]) news(`Your ${W.realm} has ${W.fallen}.`, 'bad');
  const learnt = (b.known || []).filter((t) => !(a.known || []).includes(t));
  learnt.forEach((t) => news(`Learnt: ${info.techs[t].name}${info.techs[t].unlocks.length ? ` (${info.techs[t].unlocks.join(', ')})` : ''}.`, 'good'));
  if (learnt.length && b.researching == null && b.researchable.length) news('Choose what to research next.');
}

function refresh(animate) {
  choose(chosen, false);
  hud();
  paint();
  drawTroops(animate);
  battles();
  if (selected !== null) card();
  if ($('treasury') && !$('treasury').hidden) treasury();
}

function run() {
  if (runner) { clearInterval(runner); runner = null; $('bPlay').textContent = '▶ Run'; return; }
  $('bPlay').textContent = '❚❚ Pause';
  runner = setInterval(() => {
    if (st.over || !st.alive[player]) { run(); return; }
    advance();
  }, 650);
}

function gameOver() {
  if (runner) run();
  const rows = info.factions.map((f, i) => ({ f, i, n: st.territory[i] })).filter((r) => r.f.holds).sort((a, b) => b.n - a.n);
  const place = rows.findIndex((r) => r.i === player) + 1;
  const all = st.territory[player] === st.owner.length;
  $('overTitle').textContent = all ? `The ${W.land} is yours` : !st.alive[player] ? `Your ${W.realm} has ${W.fallen}` : `${Math.round(info.days / 365)} years later`;
  const cup = all || place === 1 ? 'cup-gold' : st.alive[player] ? 'cup-laurel' : 'cup-dark';
  $('overText').innerHTML = `<img class="cup" src="icons/${cup}.jpg" alt=""><p>On day ${st.day} you are <b>#${place}</b> of ${rows.length}, holding <b>${st.territory[player]}</b> ${W.provinces}.</p>`
    + rows.map((r) => `<div>${chipFor(r.f)} ${r.f.name}: ${r.n}</div>`).join('')
    + `<p class="muted">${W.battles}: ${fmt(st.battles || 0)}; ${W.troops} ${W.destroyed}: ${fmt(st.destroyed || 0)}.</p>`;
  $('over').hidden = false;
}

// ---------- controls ----------

$('bStart').onclick = start;
$('bDay').onclick = endDay;
$('bWeek').onclick = () => days(7);
$('bTreasury').onclick = treasury;
$('bPlay').onclick = run;
$('cardClose').onclick = () => { selected = null; card(); paint(); };
$('ordersCancel').onclick = () => choose([]);
$('view').onchange = (e) => {
  view = e.target.value;
  st = call({ op: 'state', faction: player, map: viewIndex() });
  paint();
  legend();
};
$('bHelp').onclick = (e) => { e.preventDefault(); $('help').hidden = false; };
$('bFold').onclick = () => { $('hud').classList.toggle('open'); $('bFold').textContent = $('hud').classList.contains('open') ? 'Less ▴' : `${W.realms} and views ▾`; };
$('bHelpClose').onclick = () => { $('help').hidden = true; };
$('bAgain').onclick = () => {
  // Online, a new game is a new room: start from a clean page.
  if (mode === 'online') { location.href = location.pathname + location.search.replace(/[?&]join=[^&]*/, ''); return; }
  started = false; $('over').hidden = true; $('hud').hidden = true; $('card').hidden = true; $('start').hidden = false; $('seed').value = 1 + Math.floor(Math.random() * 9999); };
document.addEventListener('keydown', (e) => {
  if (e.target.tagName === 'INPUT' || e.target.tagName === 'SELECT') return;
  const playing = !$('hud').hidden && $('start').hidden && $('over').hidden;
  if (e.code === 'Space' && playing) { e.preventDefault(); endDay(); }
  if (e.code === 'KeyP' && playing && mode === 'solo') run();
  if (e.code === 'Escape') { if (chosen.length) choose([]); else { selected = null; card(); paint(); } }
});

// For tests and the curious.
window.__marches = { call, get state() { return st; }, get info() { return info; }, get map() { return map; }, get mode() { return mode; },
  get player() { return player; }, get log() { return log; }, get ready() { return [...ready]; }, get net() { return net; }, choose, clickPlace, endDay, step: endDay };

boot();
