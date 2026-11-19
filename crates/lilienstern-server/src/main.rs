//! lilienstern-server: online rooms for Lilienstern's browser games (Marches, Italia).
//!
//! Browsers connect over a WebSocket and speak JSON. One opens a room (`create`) and gets a
//! code; friends join with it (`hello`), pick a family (`pick`), and the host begins (`begin`).
//! From then on every browser runs the game itself, with the same engine on the same files: the
//! server only puts the players' commands in one order, keeps them, sends them to everyone, and
//! ends the days (when everyone has, or by the pace's clock). See room.rs for the rules of a
//! room and the messages.
//!
//! Settings, from the environment:
//!
//! * `LIL_ADDR`: where to listen (default `0.0.0.0:8080`);
//! * `LIL_DATA`: where rooms are kept, so that games survive a restart (default `data`; empty:
//!   in memory only);
//! * `LIL_ORIGINS`: the pages allowed to connect, comma-separated (default the game's site and
//!   local addresses; `*` for any).

mod room;
mod store;

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

use room::{Out, Room, PACES};

const MAX_MESSAGE: usize = 64 * 1024;
/// Rooms kept in memory (more wait on disk until someone comes back to them).
const MAX_ROOMS: usize = 5_000;
const MAX_PEERS: usize = 24;
/// Rooms each connection may open.
const MAX_CREATES: u32 = 10;
/// Messages a second a connection may send (and in a burst).
const RATE: f64 = 40.0;
const BURST: f64 = 80.0;
/// A room with nobody in it leaves memory after this (it stays on disk).
const IDLE: Duration = Duration::from_secs(6 * 3600);
const LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

struct Conn {
    tx: UnboundedSender<Message>,
    room: Option<String>,
    token: String,
    created: u32,
}

struct Hub {
    rooms: HashMap<String, Room>,
    conns: HashMap<u64, Conn>,
    data: Option<PathBuf>,
    random: RandomState,
    counter: u64,
}

impl Hub {
    fn deliver(&self, out: Out) {
        for (id, msg) in out {
            if let Some(c) = self.conns.get(&id) {
                let _ = c.tx.send(Message::Text(msg.to_string()));
            }
        }
    }

    fn save(&mut self, code: &str) {
        if let (Some(dir), Some(r)) = (&self.data, self.rooms.get_mut(code)) {
            if let Err(e) = store::save(dir, r) {
                eprintln!("room {code}: could not save: {e}");
            }
        }
    }

    /// The room, from memory or from disk.
    fn find(&mut self, code: &str, now: Instant) -> Option<&mut Room> {
        if !self.rooms.contains_key(code) {
            let r = store::load(self.data.as_ref()?, code, now)?;
            self.rooms.insert(code.to_string(), r);
        }
        self.rooms.get_mut(code)
    }

    fn new_code(&mut self) -> String {
        loop {
            let mut h = self.random.build_hasher();
            self.counter += 1;
            h.write_u64(self.counter);
            let mut x = h.finish();
            let code: String = (0..5)
                .map(|_| {
                    let c = LETTERS[(x % LETTERS.len() as u64) as usize] as char;
                    x /= LETTERS.len() as u64;
                    c
                })
                .collect();
            let on_disk = self
                .data
                .as_ref()
                .is_some_and(|d| d.join(format!("{code}.json")).exists());
            if !self.rooms.contains_key(&code) && !on_disk {
                return code;
            }
        }
    }

    /// Takes connection `id` out of its room, if it is in one.
    fn leave(&mut self, id: u64, now: Instant) {
        let Some(code) = self.conns.get_mut(&id).and_then(|c| c.room.take()) else {
            return;
        };
        let mut out = Out::new();
        if let Some(r) = self.rooms.get_mut(&code) {
            r.leave(id, now, &mut out);
        }
        self.save(&code);
        self.deliver(out);
    }

    fn handle(&mut self, id: u64, msg: &Value, now: Instant) -> Result<(), String> {
        let t = msg["t"].as_str().unwrap_or("");
        let mut out = Out::new();
        let code = match t {
            "create" => {
                let token = token(msg)?;
                let game = word(&msg["game"], 24).ok_or("which game?")?;
                let params = msg["params"].clone();
                if !params.is_object() || params.to_string().len() > 4096 {
                    return Err("bad parameters".into());
                }
                let families: Vec<Value> = msg["families"]
                    .as_array()
                    .ok_or("which families?")?
                    .iter()
                    .take(32)
                    .filter_map(|f| {
                        Some(json!({
                            "i": f["i"].as_u64()?,
                            "name": short(&f["name"], 40)?,
                            "color": short(&f["color"], 16).unwrap_or_default(),
                        }))
                    })
                    .collect();
                let seat = msg["seat"].as_u64().ok_or("which family?")?;
                if !families.iter().any(|f| f["i"] == seat) {
                    return Err("that family does not play".into());
                }
                let pace = msg["pace"].as_str().unwrap_or("wait");
                if !PACES.contains(&pace) {
                    return Err("no such pace".into());
                }
                let conn = self.conns.get_mut(&id).ok_or("no connection")?;
                if conn.created >= MAX_CREATES {
                    return Err("Too many rooms from here.".into());
                }
                if self.rooms.len() >= MAX_ROOMS {
                    return Err("The server is full: try again later.".into());
                }
                conn.created += 1;
                self.leave(id, now);
                let code = self.new_code();
                let mut r = Room::new(
                    code.clone(),
                    game,
                    token.clone(),
                    params,
                    msg["seed"].as_u64().unwrap_or(1),
                    short(&msg["version"], 32).unwrap_or_default(),
                    families,
                    seat,
                    pace.to_string(),
                    now,
                );
                r.join(id, &token, 0, now, &mut out);
                self.rooms.insert(code.clone(), r);
                let conn = self.conns.get_mut(&id).ok_or("no connection")?;
                conn.room = Some(code.clone());
                conn.token = token;
                eprintln!("room {code} opened ({} rooms)", self.rooms.len());
                code
            }
            "hello" => {
                let token = token(msg)?;
                let code: String = msg["code"]
                    .as_str()
                    .unwrap_or("")
                    .to_ascii_uppercase()
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .take(8)
                    .collect();
                let game = msg["game"].as_str().unwrap_or("");
                let have = msg["have"].as_u64().unwrap_or(0) as usize;
                let already = self.conns.get(&id).and_then(|c| c.room.clone());
                if already.as_deref() != Some(code.as_str()) {
                    self.leave(id, now);
                }
                let r = self
                    .find(&code, now)
                    .filter(|r| r.game == game)
                    .ok_or("No room with that code is open.")?;
                if r.peers.len() >= MAX_PEERS && !r.peers.iter().any(|(_, t)| *t == token) {
                    return Err("The room is full.".into());
                }
                r.join(id, &token, have, now, &mut out);
                let conn = self.conns.get_mut(&id).ok_or("no connection")?;
                conn.room = Some(code.clone());
                conn.token = token;
                code
            }
            _ => {
                let conn = self.conns.get(&id).ok_or("no connection")?;
                let code = conn.room.clone().ok_or("Join a room first.")?;
                let token = conn.token.clone();
                let r = self.rooms.get_mut(&code).ok_or("The room is gone.")?;
                if !r.peers.iter().any(|(p, _)| *p == id) {
                    return Err("You have left the room: join again.".into());
                }
                let faction = || msg["faction"].as_u64().ok_or("which family?");
                match t {
                    "pick" => r.pick(id, &token, faction()?, now, &mut out),
                    "pace" => r.set_pace(id, &token, msg["pace"].as_str().unwrap_or(""), &mut out),
                    "begin" => r.begin(id, &token, now, &mut out),
                    "req" => r.request(id, &token, &msg["cmd"], &mut out),
                    "ready" => r.end_day(&token, now, &mut out),
                    "fp" => {
                        let seq = msg["seq"].as_u64().ok_or("which command?")? as usize;
                        let fp = short(&msg["fp"], 32).ok_or("which fingerprint?")?;
                        r.report(seq, &fp, &mut out);
                    }
                    "over" => r.finish(),
                    "pause" => r.pause(id, &token, now, &mut out),
                    "release" => r.release(id, &token, faction()?, now, &mut out),
                    "resync" => r.resync(id, &token, &mut out),
                    _ => return Err(format!("unknown message {t:?}")),
                }
                code
            }
        };
        self.save(&code);
        self.deliver(out);
        Ok(())
    }

    /// Every quarter of a second: the clocks. Now and then: rooms nobody is in leave memory.
    fn tick(&mut self, now: Instant, sweep: bool) {
        let mut out = Out::new();
        let codes: Vec<String> = self
            .rooms
            .iter()
            .filter(|(_, r)| r.deadline.is_some_and(|d| now >= d))
            .map(|(c, _)| c.clone())
            .collect();
        for code in codes {
            if let Some(r) = self.rooms.get_mut(&code) {
                r.tick(now, &mut out);
            }
            self.save(&code);
        }
        self.deliver(out);
        if sweep {
            let idle: Vec<String> = self
                .rooms
                .iter()
                .filter(|(_, r)| r.peers.is_empty() && now.duration_since(r.last_active) > IDLE)
                .map(|(c, _)| c.clone())
                .collect();
            for code in idle {
                self.save(&code);
                self.rooms.remove(&code);
            }
            let started = self.rooms.values().filter(|r| r.started && !r.over).count();
            eprintln!(
                "{} rooms in memory ({started} games under way), {} connections",
                self.rooms.len(),
                self.conns.len()
            );
        }
    }
}

fn token(msg: &Value) -> Result<String, String> {
    msg["token"]
        .as_str()
        .filter(|t| (8..=64).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map(str::to_string)
        .ok_or_else(|| "who are you?".to_string())
}

fn word(v: &Value, max: usize) -> Option<String> {
    v.as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= max
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .map(str::to_string)
}

fn short(v: &Value, max: usize) -> Option<String> {
    v.as_str().map(|s| s.chars().take(max).collect())
}

fn allowed(origin: &str, origins: &[String]) -> bool {
    origins
        .iter()
        .any(|a| a == "*" || origin == a || origin.starts_with(&format!("{a}:")))
}

// The handshake's refusal is tungstenite's own type, large as it is.
#[allow(clippy::result_large_err)]
async fn connection(stream: TcpStream, hub: Arc<Mutex<Hub>>, id: u64, origins: Arc<Vec<String>>) {
    let check = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        match req.headers().get("origin").and_then(|o| o.to_str().ok()) {
            Some(o) if !allowed(o, &origins) => {
                let mut e = ErrorResponse::new(Some("this page may not connect".into()));
                *e.status_mut() = StatusCode::FORBIDDEN;
                Err(e)
            }
            _ => Ok(resp),
        }
    };
    let config = WebSocketConfig {
        max_message_size: Some(MAX_MESSAGE),
        max_frame_size: Some(MAX_MESSAGE),
        ..Default::default()
    };
    let Ok(ws) = tokio_tungstenite::accept_hdr_async_with_config(stream, check, Some(config)).await
    else {
        return;
    };
    let (mut sink, mut source) = ws.split();
    let (tx, mut rx) = unbounded_channel::<Message>();
    hub.lock().unwrap().conns.insert(
        id,
        Conn {
            tx,
            room: None,
            token: String::new(),
            created: 0,
        },
    );
    // Writes what the rooms send, and pings now and then so that proxies keep the line open.
    let writer = tokio::spawn(async move {
        let mut ping = tokio::time::interval(Duration::from_secs(25));
        loop {
            tokio::select! {
                m = rx.recv() => match m {
                    Some(m) => if sink.send(m).await.is_err() { break },
                    None => break,
                },
                _ = ping.tick() => if sink.send(Message::Ping(Vec::new())).await.is_err() { break },
            }
        }
        let _ = sink.close().await;
    });
    let (mut allowance, mut last) = (BURST, Instant::now());
    while let Some(Ok(m)) = source.next().await {
        let text = match m {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };
        let now = Instant::now();
        allowance = (allowance + now.duration_since(last).as_secs_f64() * RATE).min(BURST);
        last = now;
        if allowance < 1.0 {
            break; // flooding: goodbye
        }
        allowance -= 1.0;
        let mut h = hub.lock().unwrap();
        let result = match serde_json::from_str::<Value>(&text) {
            Ok(msg) => h.handle(id, &msg, now),
            Err(_) => Err("not JSON".into()),
        };
        if let Err(e) = result {
            h.deliver(vec![(id, json!({ "t": "err", "msg": e }))]);
        }
    }
    let mut h = hub.lock().unwrap();
    h.leave(id, Instant::now());
    h.conns.remove(&id);
    drop(h);
    writer.abort();
}

#[tokio::main]
async fn main() {
    let addr = std::env::var("LIL_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let data = std::env::var("LIL_DATA").unwrap_or_else(|_| "data".into());
    let data = (!data.is_empty()).then(|| PathBuf::from(data));
    if let Some(d) = &data {
        std::fs::create_dir_all(d).expect("cannot create the data directory");
        store::prune(d);
    }
    let origins: Arc<Vec<String>> = Arc::new(
        std::env::var("LIL_ORIGINS")
            .unwrap_or_else(|_| "https://ingmmo.com,http://localhost,http://127.0.0.1".into())
            .split(',')
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    );
    let hub = Arc::new(Mutex::new(Hub {
        rooms: HashMap::new(),
        conns: HashMap::new(),
        data: data.clone(),
        random: RandomState::new(),
        counter: 0,
    }));
    let listener = TcpListener::bind(&addr).await.expect("cannot listen");
    eprintln!(
        "lilienstern-server on {addr}; rooms in {}; pages from {}",
        data.as_ref()
            .map_or("memory only".into(), |d| d.display().to_string()),
        origins.join(", ")
    );
    let clock = hub.clone();
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_millis(250));
        let mut n: u64 = 0;
        loop {
            every.tick().await;
            n += 1;
            let sweep = n % 2400 == 0; // every 10 minutes
            clock.lock().unwrap().tick(Instant::now(), sweep);
            if n % 14_400 == 0 {
                if let Some(d) = &data {
                    store::prune(d);
                }
            }
        }
    });
    let next = AtomicU64::new(1);
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("cannot watch for signals");
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                if let Ok((stream, _)) = accepted {
                    let _ = stream.set_nodelay(true);
                    let id = next.fetch_add(1, Ordering::Relaxed);
                    tokio::spawn(connection(stream, hub.clone(), id, origins.clone()));
                }
            }
            _ = term.recv() => break,
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    // Everything is saved as it happens; nothing more to do.
    eprintln!("lilienstern-server: stopping");
}
