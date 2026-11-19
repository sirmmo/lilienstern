// Marches (and games built like it, such as Italia). The Lilienstern engine runs as
// WebAssembly (lilienstern.wasm); this page sends it JSON requests (see crates/lilienstern-web)
// and draws the land and its troops with MapLibre.
//
// A page can set `window.GAME` before loading this script to play another game with the same
// troops and decisions: which files to load (paths as the game names them, fetched from
// game/), the parameters its start screen sets, and which parameter says how many factions play.

const radio = (name) => document.querySelector(`input[name=${name}]:checked`).value;
const CONFIG = Object.assign({
  id: 'marches',
  files: () => ['params.toml', 'game.toml', 'realms.lil', 'eras/medieval.toml', 'eras/future.toml'],
  probe: { provinces: 40 }, // a quick game, to learn the factions' names and colours
  params: () => ({ era: radio('era'), provinces: +radio('size') }),
  count: null, // the parameter with the number of factions that play, if the start screen sets it
}, window.GAME || {});
const VIEWS = { Wealth: 'Wealth', Population: 'People', Elevation: 'Elevation', Fort: 'Castles', Violence: 'Battles' };
const TWEEN_MS = 450;
// Icons (web/marches/icons, see the README there): troops by era and id, decisions by name.
const DECISION_ICONS = ['settle', 'govern', 'fortify', 'defend', 'attack', 'occupy', 'make_peace', 'declare_war', 'ally'];

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
// mode: 'solo' (one person), 'local' (several, taking turns on this device), 'host' or 'guest'
// (online). `mine` are the factions played on this device, `player` the one shown now;
// `humans` every faction a person plays. A day runs when every human has ended it (`ready`);
// online, players whose connection dropped (`away`) do not hold the day up.
let mode = 'solo';
let mine = [];
let humans = [];
const ready = new Set();
const away = new Set();
let net = null; // host: { peer, code, guests: Map(conn -> token), seats: Map(faction -> token|'host') }; guest: { peer, conn, faction }
let settings = null; // { params, seed, players }: what every browser starts the game from
let log = []; // every command since the start, in order: what a guest replays to catch up
let started = false;
const PREFIX = () => `lilienstern-${CONFIG.id}-v1-`;

// The pace of an online game, set by the host when it creates the game: 'wait' (a day runs when
// everyone has ended it), 'limit-N' (... or after N seconds at most) or 'run-N' (a day every N
// seconds, sooner if everyone has ended it). The host's clock decides; guests show a countdown.
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
let deadline = null; // when the host's clock runs the day (performance.now() time), or null
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
    b.innerHTML = `<i class="chip" style="background:${r.color}"></i>${r.name}`;
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
      <label><input type="radio" name="mode" value="local"> Several of us, on this device <span class="muted">(take turns; choose every family a person plays)</span></label>
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
    era = s.params.era || 'medieval';
    document.body.classList.toggle('future', era === 'future');
    info = call({ op: 'new', files: await files(s.params), params: s.params, seed: s.seed, players: s.players });
    log = [];
    for (const c of commands) apply(c);
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
// Everything that changes the game is a command: a decision ('take'), an order ('order'), a
// person taking over or leaving a faction ('claim', 'release'), the end of a day ('step').
// Commands run in one order in every browser: the host's.

/** Runs a command on this browser's engine and records it. Throws if the engine refuses it. */
function apply(c) {
  if (c.op === 'step') {
    call({ op: 'step', days: 1 });
  } else {
    call(c);
  }
  log.push(c);
}

/** This device wants a command done: alone or hosting, now; as a guest, by the host. */
function act(c) {
  if (mode === 'guest') {
    send({ t: 'req', cmd: c });
    return true;
  }
  try {
    apply(c);
  } catch (e) {
    news(e.message, 'bad');
    return false;
  }
  if (mode === 'host') broadcast({ t: 'cmd', seq: log.length, cmd: c });
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
  if (mode === 'guest') {
    send({ t: 'ready', faction: player });
    hud();
    return;
  }
  if (mode === 'host') {
    broadcast({ t: 'ready', ready: [...ready] });
    hostDay();
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

/** Runs one day here (as the host, or alone): every human is done. */
function advance() {
  busy = true;
  const before = st;
  apply({ op: 'step' });
  st = call({ op: 'state', faction: player, map: viewIndex() });
  busy = false;
  ready.clear();
  if (mode === 'host') {
    broadcast({ t: 'step', seq: log.length, fingerprint: st.fingerprint });
    startClock();
  }
  report(before, st);
  refresh(true);
  if (st.over || (mode === 'solo' && (!st.alive[player] || st.territory[player] === st.owner.length))) gameOver();
}

// ---------- the clock (online, set by the host) ----------

/** As the host: a new day, a new deadline (none when the game waits for everyone). */
function startClock() {
  const secs = paceSeconds(settings?.pace);
  deadline = secs && !st.over ? performance.now() + secs * 1000 : null;
  sendClock();
  tick();
}

/** Tells guests (or one guest) how long the day has left; they count down on their own clock. */
function sendClock(conn) {
  const msg = { t: 'clock', ms: deadline === null ? null : Math.max(0, deadline - performance.now()), paused };
  if (conn) conn.send(msg); else broadcast(msg);
}

/** The host pauses or resumes the clock for everyone. */
function togglePause() {
  if (mode !== 'host' || deadline === null) return;
  if (paused) {
    deadline = performance.now() + net.left;
    paused = false;
  } else {
    net.left = Math.max(0, deadline - performance.now());
    paused = true;
  }
  sendClock();
  hud();
}

/** Every quarter of a second: the countdown, and (as the host) the day when time is up. */
function tick() {
  if (!ticker) {
    // A worker's timer keeps going in a background tab, where the page's own are slowed down
    // (to once a minute after a while): the host's days must not stall when it looks elsewhere.
    try {
      ticker = new Worker(URL.createObjectURL(new Blob(['setInterval(() => postMessage(0), 250)'], { type: 'text/javascript' })));
      ticker.onmessage = tick;
    } catch {
      ticker = setInterval(tick, 250);
    }
  }
  const el = $('clock');
  if (!started || deadline === null || st?.over) {
    if (el) el.textContent = '';
    return;
  }
  const left = paused ? net?.left ?? 0 : Math.max(0, deadline - performance.now());
  if (el) el.textContent = paused ? ' · paused' : ` · ${Math.ceil(left / 1000)} s`;
  if (mode === 'host' && !paused && left <= 0 && !busy) {
    deadline = null;
    advance();
  }
}

/** As the host: the day runs once every person still in the game has ended it. */
function hostDay() {
  const waiting = humans.filter((f) => !ready.has(f) && !away.has(f) && st.alive[f]);
  hud();
  if (!waiting.length) advance();
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
  el.innerHTML = `<div class="sheet"><h2><i class="chip" style="background:${f.color}"></i> ${f.name}</h2>
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
  map.fitBounds([[Math.min(...xs), Math.min(...ys)], [Math.max(...xs), Math.max(...ys)]], { padding: framePadding(), maxZoom: 7.5, duration: 600 });
}

// ---------- online ----------

function send(msg) {
  if (net?.conn?.open) net.conn.send(msg);
}

function broadcast(msg) {
  if (!net?.guests) return;
  for (const conn of net.guests.keys()) if (conn.open) conn.send(msg);
}

function lobbyState() {
  const taken = [...net.seats.entries()].map(([f, who]) => [f, who === 'host' ? 'host' : 'guest']);
  return { t: 'lobby', code: net.code, params: settings?.params || net.params, seed: net.seed, version: net.version, taken, started, pace: settings?.pace || net.pace,
    families: realms.slice(0, CONFIG.count ? +net.params[CONFIG.count] : realms.length).map((r) => ({ i: r.i, name: r.name, color: r.color })) };
}

async function openRoom(params, seed) {
  $('bStart').disabled = true;
  $('bStart').textContent = 'Opening the room…';
  try {
    const code = LilNet.newCode();
    const fs = await files(params);
    net = { code, params, seed, version: version(fs), guests: new Map(), seats: new Map([[player, 'host']]), pace: 'wait' };
    net.peer = await LilNet.host(PREFIX(), code, onGuest);
    mode = 'host';
    mine = [player];
    showLobby();
  } catch (e) {
    $('startError').textContent = e.message;
    $('bStart').disabled = false;
    $('bStart').textContent = 'Open the room';
  }
}

function onGuest(conn) {
  net.guests.set(conn, null);
  conn.on('data', (msg) => hostMessage(conn, msg));
  conn.on('close', () => {
    const token = net.guests.get(conn);
    net.guests.delete(conn);
    const seat = [...net.seats.entries()].find(([, who]) => who === token);
    if (!seat) return;
    if (started) {
      away.add(seat[0]);
      news(`${info.factions[seat[0]].name} has lost the connection; the days go on without waiting.`, 'bad');
      hostDay();
    } else {
      net.seats.delete(seat[0]);
      showLobby();
      broadcast(lobbyState());
    }
  });
}

/** The room as the host sees it before the game begins: the code and who plays what. */
function showLobby() {
  const box = $('lobby');
  box.hidden = false;
  const link = `${location.origin}${location.pathname}?join=${net.code}`;
  const fams = lobbyState().families;
  box.innerHTML = `<div class="code">Room <b>${net.code}</b></div>
    <div class="muted small">Friends open <a href="${link}" target="_blank">${link}</a>, or choose "join a room" and type the code.</div>
    <div class="seats">${fams.map((f) => `<div><i class="chip" style="background:${f.color}"></i>${f.name}<span class="n">${net.seats.get(f.i) === 'host' ? 'you' : net.seats.has(f.i) ? 'a friend' : 'the computer'}</span></div>`).join('')}</div>
    <div class="pace">Pace <select id="pace">${PACES.map(([v, t]) => `<option value="${v}"${v === net.pace ? ' selected' : ''}>${t}</option>`).join('')}</select></div>
    <button id="bBegin" class="primary big">Begin</button>`;
  $('bBegin').onclick = beginRoom;
  $('pace').onchange = (e) => { net.pace = e.target.value; broadcast(lobbyState()); };
  $('bStart').hidden = true;
}

async function beginRoom() {
  const players = [...net.seats.keys()].sort((a, b) => a - b);
  const s = { params: net.params, seed: net.seed, players, pace: net.pace };
  mode = 'host';
  await launch(s, []);
  for (const [conn, token] of net.guests) {
    if (token) conn.send({ t: 'start', settings: s, commands: [], faction: seatOf(token) });
  }
  startClock();
  $('bStart').hidden = false;
}

const seatOf = (token) => [...net.seats.entries()].find(([, who]) => who === token)?.[0];

function hostMessage(conn, msg) {
  if (msg.t === 'hello') {
    net.guests.set(conn, msg.token);
    const f = seatOf(msg.token);
    if (started && f !== undefined) {
      // Back after a dropped connection: the whole game so far, and the same family.
      away.delete(f);
      conn.send({ t: 'start', settings, commands: log, faction: f });
      news(`${info.factions[f].name} is back.`, 'good');
      sendClock(conn);
      hud();
      return;
    }
    conn.send(lobbyState());
    return;
  }
  const token = net.guests.get(conn);
  if (msg.t === 'pick') {
    const free = !net.seats.has(msg.faction) && lobbyState().families.some((f) => f.i === msg.faction);
    if (!free || seatOf(token) !== undefined) { conn.send({ t: 'err', msg: 'That family is taken.' }); return; }
    if (!started) {
      net.seats.set(msg.faction, token);
      showLobby();
      broadcast(lobbyState());
      return;
    }
    // Joining a game under way: a person takes over a family the computer played.
    if (!act({ op: 'claim', faction: msg.faction })) return;
    net.seats.set(msg.faction, token);
    conn.send({ t: 'start', settings, commands: log, faction: msg.faction });
    sendClock(conn);
    news(`A friend now plays ${info.factions[msg.faction].name}.`, 'good');
    return;
  }
  const f = seatOf(token);
  if (f === undefined) return;
  if (msg.t === 'req') {
    // A guest acts only for its own family.
    const c = { ...msg.cmd, faction: f };
    if (!['take', 'order'].includes(c.op)) return;
    try {
      apply(c);
    } catch (e) {
      conn.send({ t: 'err', msg: e.message });
      return;
    }
    broadcast({ t: 'cmd', seq: log.length, cmd: c });
    afterCommand(c);
  } else if (msg.t === 'ready') {
    ready.add(f);
    broadcast({ t: 'ready', ready: [...ready] });
    hostDay();
  } else if (msg.t === 'resync') {
    conn.send({ t: 'start', settings, commands: log, faction: f });
  }
}

async function joinRoom() {
  const code = LilNet.cleanCode($('joinCode').value);
  if (!code) { $('startError').textContent = 'Type the room code.'; return; }
  $('bStart').disabled = true;
  $('bStart').textContent = 'Joining…';
  try {
    const { peer, conn } = await LilNet.join(PREFIX(), code);
    net = { peer, conn, code };
    mode = 'guest';
    conn.on('data', guestMessage);
    conn.on('close', () => news('The host has left: the game cannot go on.', 'bad'));
    conn.send({ t: 'hello', token: LilNet.token() });
  } catch (e) {
    $('startError').textContent = e.message;
  } finally {
    $('bStart').disabled = false;
    $('bStart').textContent = 'Join';
  }
}

async function guestMessage(msg) {
  if (msg.t === 'lobby') {
    const fs = await files(msg.params);
    if (version(fs) !== msg.version) {
      $('startError').textContent = 'The host plays another version of this game: reload both pages.';
      return;
    }
    const box = $('lobby');
    box.hidden = false;
    const taken = new Map(msg.taken);
    box.innerHTML = `<div class="code">Room <b>${msg.code}</b>${msg.started ? ' <span class="muted">(the game is under way)</span>' : ''}</div>
      <div class="muted small">Choose your family${msg.started ? ' (you take it over from the computer)' : ''}; the host begins the game.</div>
      <div class="pace">Pace: <b>${paceName(msg.pace)}</b></div>
      <div class="seats">${msg.families.map((f) => `<div><i class="chip" style="background:${f.color}"></i>${f.name}<span class="n">${taken.has(f.i) ? (taken.get(f.i) === 'host' ? 'the host' : 'a friend') : `<button data-f="${f.i}">Play</button>`}</span></div>`).join('')}</div>`;
    box.querySelectorAll('button[data-f]').forEach((b) => { b.onclick = () => { send({ t: 'pick', faction: +b.dataset.f }); box.querySelectorAll('button[data-f]').forEach((x) => { x.disabled = true; }); }; });
    $('bStart').hidden = true;
  } else if (msg.t === 'start') {
    mine = [msg.faction];
    mode = 'guest';
    await launch(msg.settings, msg.commands);
    $('bStart').hidden = false;
  } else if (msg.t === 'cmd') {
    if (!started) return;
    if (msg.seq !== log.length + 1) { send({ t: 'resync' }); return; }
    try {
      apply(msg.cmd);
    } catch {
      send({ t: 'resync' });
      return;
    }
    afterCommand(msg.cmd);
  } else if (msg.t === 'step') {
    if (!started) return;
    if (msg.seq !== log.length + 1) { send({ t: 'resync' }); return; }
    const before = st;
    apply({ op: 'step' });
    st = call({ op: 'state', faction: player, map: viewIndex() });
    ready.clear();
    if (st.fingerprint !== msg.fingerprint) {
      news('Out of step with the host: catching up.', 'bad');
      send({ t: 'resync' });
      return;
    }
    report(before, st);
    refresh(true);
    if (st.over) gameOver();
  } else if (msg.t === 'ready') {
    ready.clear();
    msg.ready.forEach((f) => ready.add(f));
    hud();
  } else if (msg.t === 'clock') {
    paused = msg.paused;
    deadline = msg.ms === null ? null : performance.now() + msg.ms;
    tick();
  } else if (msg.t === 'err') {
    news(msg.msg, 'bad');
    $('lobby').querySelectorAll('button[data-f]').forEach((x) => { x.disabled = false; });
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
      places: { type: 'geojson', data: info.geojson, attribution: ['Lilienstern', info.name, info.credit].filter(Boolean).join(' · ') },
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

/** The kind that stands for a stack: the most common, the dearest on a tie. */
function leadKind(kinds) {
  const n = {};
  kinds.forEach((k) => { n[k] = (n[k] || 0) + 1; });
  return +Object.keys(n).sort((a, b) => n[b] - n[a] || b - a)[0];
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
  if (ok) news(`${ok > 1 ? ok + ' troops march' : 'Your troop marches'} to ${placeName(to)}${days ? `: ${days} day${days > 1 ? 's' : ''}` : ''}.`);
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
    + `<div class="kinds">${counts.map((x) => `<span title="${x.k.description}">${img(troopIcon(info.troopKinds.indexOf(x.k)))}${x.k.name} × ${x.n}</span>`).join('')}</div>`;
  if (idle.length) {
    const b = document.createElement('button');
    b.textContent = 'Select the troops without orders';
    b.onclick = () => choose(idle.map((t) => t.id));
    $('army').appendChild(b);
  }
  players();
  legend();
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
    const who = mine.includes(f) ? (mode === 'local' ? 'here' : 'you') : mode === 'host' ? 'friend' : '';
    const ai = mode === 'host' && away.has(f) ? ` <button data-ai="${f}">to the computer</button>` : '';
    return `<div class="${f === player ? 'me' : ''}"><i class="chip" style="background:${fac.color}"></i>${fac.name} <span class="muted">${who}</span><span class="n">${state}${ai}</span></div>`;
  }).join('');
  const room = net?.code ? `<span class="muted">· room ${net.code}</span>` : '';
  const pace = settings?.pace && settings.pace !== 'wait' ? `<div class="muted small">${paceName(settings.pace)}${mode === 'host' ? ` <button id="bPause">${paused ? 'Resume' : 'Pause'}</button>` : ''}</div>` : '';
  box.innerHTML = `<span class="lbl">Players ${room}</span>${rows}${pace}`;
  if ($('bPause')) $('bPause').onclick = togglePause;
  if (!$('clock')) {
    const c = document.createElement('span');
    c.id = 'clock';
    c.className = 'muted';
    $('days').parentElement.after(c);
  }
  box.querySelectorAll('button[data-ai]').forEach((b) => {
    b.onclick = () => {
      const f = +b.dataset.ai;
      if (!act({ op: 'release', faction: f })) return;
      net.seats.delete(f);
      away.delete(f);
      hostDay();
    };
  });
  $('bDay').textContent = mode === 'solo' ? 'End day' : ready.has(player) ? 'Waiting…' : 'End day';
  $('bDay').disabled = mode !== 'solo' && mode !== 'local' && ready.has(player);
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
  const rel = owner && c.owner !== player ? ` <span class="rel ${st.relations[c.owner]}">${st.relations[c.owner]}</span>` : '';
  $('cardOwner').innerHTML = owner ? `Held by <b>${owner.name}</b>${c.owner === player ? ' (you)' : ''}${rel}` : 'Nobody holds it';
  const ter = c.terrain !== null ? info.terrains[c.terrain] : null;
  const kv = [
    ter ? `<span>Terrain</span><span>${ter.name}${ter.defense ? ` (+${ter.defense})` : ''}</span>` : '',
    `<span>Castles</span><span>${fmt(c.maps.Fort || 0)}</span>`,
    `<span>Wealth</span><span>${fmt(c.maps.Wealth || 0)}</span>`,
    'Population' in c.maps ? `<span>People (1871)</span><span>${Math.round(c.maps.Population * 1000).toLocaleString('en')}</span>` : '',
    'Elevation' in c.maps ? `<span>Elevation</span><span>${Math.round(c.maps.Elevation)} m</span>` : '',
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
    row.innerHTML = `<i class="chip" style="background:${info.factions[t.faction].color}"></i>${img(troopIcon(t.kind))}${k.name}<span class="hp"><i style="width:${t.hp}%"></i></span>`;
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
  const before = st.troops.length;
  if (!act({ op: 'take', faction: player, decision: o.decision, place })) return;
  if (o.raise && mode !== 'guest' && st.troops.length > before) news(`${o.raise} raised in ${placeName(place)}.`, 'good');
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
    advance();
  }, 650);
}

function gameOver() {
  if (runner) run();
  const rows = info.factions.map((f, i) => ({ f, i, n: st.territory[i] })).filter((r) => r.f.holds).sort((a, b) => b.n - a.n);
  const place = rows.findIndex((r) => r.i === player) + 1;
  const all = st.territory[player] === st.owner.length;
  $('overTitle').textContent = all ? 'The land is yours' : !st.alive[player] ? 'Your realm has fallen' : `${Math.round(info.days / 365)} years later`;
  const cup = all || place === 1 ? 'cup-gold' : st.alive[player] ? 'cup-laurel' : 'cup-dark';
  $('overText').innerHTML = `<img class="cup" src="icons/${cup}.jpg" alt=""><p>On day ${st.day} you are <b>#${place}</b> of ${rows.length}, holding <b>${st.territory[player]}</b> provinces.</p>`
    + rows.map((r) => `<div><i class="chip" style="background:${r.f.color}"></i> ${r.f.name}: ${r.n}</div>`).join('')
    + `<p class="muted">Battles fought: ${fmt(st.battles || 0)}; troops destroyed: ${fmt(st.destroyed || 0)}.</p>`;
  $('over').hidden = false;
}

// ---------- controls ----------

$('bStart').onclick = start;
$('bDay').onclick = endDay;
$('bWeek').onclick = () => days(7);
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
$('bFold').onclick = () => { $('hud').classList.toggle('open'); $('bFold').textContent = $('hud').classList.contains('open') ? 'Less ▴' : 'Realms and views ▾'; };
$('bHelpClose').onclick = () => { $('help').hidden = true; };
$('bAgain').onclick = () => {
  // Online, a new game is a new room: start from a clean page.
  if (mode === 'host' || mode === 'guest') { location.href = location.pathname; return; }
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
  get player() { return player; }, get log() { return log; }, get ready() { return [...ready]; }, choose, clickPlace, endDay, step: endDay };

boot();
