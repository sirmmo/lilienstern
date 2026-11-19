//! A room: one online game. The room does not run the game: every browser does, with the same
//! engine on the same files, parameters and seed. The room puts every command (a decision, an
//! order, a person taking over or leaving a faction, the end of a day) in one order, keeps them
//! (the log is the game), and sends each to everyone. It runs the day when every person still
//! connected has ended it, or when the pace's clock says so.
//!
//! Everything here is plain data and functions that return the messages to send, so it can be
//! tested without a network.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

/// How the days go: when everyone has ended the day ("wait"), or that or after N seconds at most
/// ("limit-N"), or a day every N seconds whatever the players do, sooner if everyone is done
/// ("run-N").
pub const PACES: [&str; 8] = [
    "wait",
    "limit-30",
    "limit-60",
    "limit-120",
    "run-5",
    "run-10",
    "run-20",
    "run-30",
];

pub fn pace_seconds(pace: &str) -> u64 {
    pace.split('-')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Messages to send: (connection, message).
pub type Out = Vec<(u64, Value)>;

/// Commands a player may ask for, and the fields each keeps.
const OPS: [(&str, &[&str]); 3] = [
    ("take", &["decision", "place"]),
    ("order", &["troop", "to"]),
    ("research", &["tech"]),
];

/// The most commands a game may have (a long Italia game has about 20,000).
pub const MAX_LOG: usize = 400_000;

#[derive(Clone, Debug)]
pub struct Room {
    pub code: String,
    /// The page's game id ("marches", "italia").
    pub game: String,
    /// The token of whoever opened the room: it sets the pace, begins, pauses, and can hand a
    /// seat to the computer.
    pub owner: String,
    pub params: Value,
    pub seed: u64,
    /// A fingerprint of the game's files, as the page computes it.
    pub version: String,
    /// The factions people may play: {i, name, color}.
    pub families: Vec<Value>,
    pub pace: String,
    pub started: bool,
    pub over: bool,
    /// Faction -> token of the person who plays it.
    pub seats: BTreeMap<u64, String>,
    /// The factions people played at the start.
    pub players: Vec<u64>,
    pub log: Vec<Value>,
    /// Factions whose person has ended the day.
    pub ready: BTreeSet<u64>,
    /// Connections in the room: (connection, token).
    pub peers: Vec<(u64, String)>,
    /// When the clock runs the day.
    pub deadline: Option<Instant>,
    /// While paused: the time that was left.
    pub paused: Option<Duration>,
    /// The first fingerprint reported for the last day: (log length, fingerprint).
    pub fingerprint: Option<(usize, String)>,
    pub last_active: Instant,
    /// For the store: the room's settings changed, and how much of the log is saved.
    pub dirty: bool,
    pub saved: usize,
}

impl Room {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        code: String,
        game: String,
        owner: String,
        params: Value,
        seed: u64,
        version: String,
        families: Vec<Value>,
        seat: u64,
        pace: String,
        now: Instant,
    ) -> Room {
        Room {
            code,
            game,
            seats: BTreeMap::from([(seat, owner.clone())]),
            owner,
            params,
            seed,
            version,
            families,
            pace,
            started: false,
            over: false,
            players: Vec::new(),
            log: Vec::new(),
            ready: BTreeSet::new(),
            peers: Vec::new(),
            deadline: None,
            paused: None,
            fingerprint: None,
            last_active: now,
            dirty: true,
            saved: 0,
        }
    }

    fn name(&self, f: u64) -> String {
        self.families
            .iter()
            .find(|x| x["i"].as_u64() == Some(f))
            .and_then(|x| x["name"].as_str())
            .unwrap_or("A family")
            .to_string()
    }

    fn seat_of(&self, token: &str) -> Option<u64> {
        self.seats
            .iter()
            .find(|(_, t)| t.as_str() == token)
            .map(|(&f, _)| f)
    }

    fn connected(&self, token: &str) -> bool {
        self.peers.iter().any(|(_, t)| t == token)
    }

    /// Factions played by a person who is not connected: they do not hold the day up.
    pub fn away(&self) -> Vec<u64> {
        self.seats
            .iter()
            .filter(|(_, t)| !self.connected(t))
            .map(|(&f, _)| f)
            .collect()
    }

    fn all(&self, out: &mut Out, msg: Value) {
        for (id, _) in &self.peers {
            out.push((*id, msg.clone()));
        }
    }

    fn others(&self, me: u64, out: &mut Out, msg: Value) {
        for (id, _) in &self.peers {
            if *id != me {
                out.push((*id, msg.clone()));
            }
        }
    }

    fn note(&self, out: &mut Out, except: Option<u64>, msg: String, kind: &str) {
        let m = json!({ "t": "note", "msg": msg, "kind": kind });
        match except {
            Some(me) => self.others(me, out, m),
            None => self.all(out, m),
        }
    }

    pub fn lobby_for(&self, token: &str) -> Value {
        let taken: Vec<Value> = self
            .seats
            .iter()
            .map(|(f, t)| {
                let who = if t == token {
                    "you"
                } else if *t == self.owner {
                    "host"
                } else {
                    "friend"
                };
                json!([f, who, !self.connected(t)])
            })
            .collect();
        json!({
            "t": "lobby", "code": self.code, "params": self.params, "seed": self.seed,
            "version": self.version, "families": self.families, "pace": self.pace,
            "started": self.started, "owner": token == self.owner, "taken": taken,
        })
    }

    fn lobby_all(&self, out: &mut Out) {
        for (id, t) in &self.peers {
            out.push((*id, self.lobby_for(t)));
        }
    }

    pub fn settings(&self) -> Value {
        json!({ "params": self.params, "seed": self.seed, "players": self.players, "pace": self.pace })
    }

    /// The game for `token`'s person, from command `have` on (all of it if they have more than
    /// there is).
    fn start_for(&self, token: &str, have: usize) -> Option<Value> {
        let f = self.seat_of(token)?;
        let from = if have <= self.log.len() { have } else { 0 };
        Some(json!({
            "t": "start", "settings": self.settings(), "commands": &self.log[from..], "from": from,
            "faction": f, "owner": token == self.owner, "code": self.code,
        }))
    }

    pub fn clock(&self, now: Instant) -> Value {
        let ms = match (self.paused, self.deadline) {
            (Some(left), _) => Some(left.as_millis() as u64),
            (None, Some(d)) => Some(d.saturating_duration_since(now).as_millis() as u64),
            _ => None,
        };
        json!({ "t": "clock", "ms": ms, "paused": self.paused.is_some() })
    }

    fn status(&self, out: &mut Out) {
        self.all(
            out,
            json!({ "t": "away", "away": self.away(), "ready": self.ready }),
        );
    }

    /// A connection joins (or comes back). `have`: commands it already has of this game.
    pub fn join(&mut self, id: u64, token: &str, have: usize, now: Instant, out: &mut Out) {
        let back = self.started && self.seat_of(token).is_some() && !self.connected(token);
        // A connection that comes back replaces the old one, if the server has not noticed yet
        // that it dropped.
        self.peers.retain(|(_, t)| t != token);
        self.peers.push((id, token.to_string()));
        self.last_active = now;
        match self.start_for(token, have).filter(|_| self.started) {
            Some(start) => {
                out.push((id, start));
                out.push((id, self.clock(now)));
                if back {
                    let f = self.seat_of(token).unwrap_or(0);
                    self.note(out, Some(id), format!("{} is back.", self.name(f)), "good");
                }
                self.status(out);
            }
            None => self.lobby_all(out),
        }
    }

    /// A connection has gone.
    pub fn leave(&mut self, id: u64, now: Instant, out: &mut Out) {
        let Some(i) = self.peers.iter().position(|(p, _)| *p == id) else {
            return;
        };
        let (_, token) = self.peers.remove(i);
        if self.connected(&token) {
            return;
        }
        match self.seat_of(&token) {
            Some(f) if self.started => {
                self.note(
                    out,
                    None,
                    format!(
                        "{} has lost the connection; the days go on without waiting.",
                        self.name(f)
                    ),
                    "bad",
                );
                self.status(out);
                self.maybe_step(now, out);
            }
            _ => self.lobby_all(out),
        }
    }

    fn err(id: u64, out: &mut Out, msg: &str) {
        out.push((id, json!({ "t": "err", "msg": msg })));
    }

    /// A person chooses a faction: before the game, a seat; during it, they take it over from
    /// the computer. A seat whose person is away (a closed tab, another device) can be taken
    /// over too; the host's makes the newcomer the host.
    pub fn pick(&mut self, id: u64, token: &str, f: u64, now: Instant, out: &mut Out) {
        let known = self.families.iter().any(|x| x["i"].as_u64() == Some(f));
        if self.seat_of(token).is_some() {
            Self::err(id, out, "You already play a family.");
            return;
        }
        if let Some(old) = self.seats.get(&f).cloned() {
            if self.connected(&old) {
                Self::err(id, out, "That family is taken.");
                return;
            }
            self.seats.insert(f, token.to_string());
            if old == self.owner {
                self.owner = token.to_string();
            }
            self.dirty = true;
            if !self.started {
                self.lobby_all(out);
                return;
            }
            if let Some(start) = self.start_for(token, 0) {
                out.push((id, start));
                out.push((id, self.clock(now)));
            }
            self.note(out, Some(id), format!("{} is back.", self.name(f)), "good");
            self.status(out);
            return;
        }
        if !known {
            Self::err(id, out, "That family does not play.");
            return;
        }
        if self.over {
            Self::err(id, out, "The game is over.");
            return;
        }
        self.seats.insert(f, token.to_string());
        self.dirty = true;
        if !self.started {
            self.lobby_all(out);
            return;
        }
        let c = json!({ "op": "claim", "faction": f });
        self.log.push(c.clone());
        let seq = self.log.len();
        self.others(id, out, json!({ "t": "cmd", "seq": seq, "cmd": c }));
        if let Some(start) = self.start_for(token, 0) {
            out.push((id, start));
            out.push((id, self.clock(now)));
        }
        self.note(
            out,
            Some(id),
            format!("A friend now plays {}.", self.name(f)),
            "good",
        );
        self.status(out);
    }

    pub fn set_pace(&mut self, id: u64, token: &str, pace: &str, out: &mut Out) {
        if token != self.owner || self.started || !PACES.contains(&pace) {
            Self::err(id, out, "Only the host sets the pace, before the game.");
            return;
        }
        self.pace = pace.to_string();
        self.dirty = true;
        self.lobby_all(out);
    }

    pub fn begin(&mut self, id: u64, token: &str, now: Instant, out: &mut Out) {
        if token != self.owner || self.started {
            Self::err(id, out, "Only the host begins the game.");
            return;
        }
        self.started = true;
        self.players = self.seats.keys().copied().collect();
        self.dirty = true;
        self.start_clock(now);
        for (pid, t) in &self.peers {
            match self.start_for(t, 0) {
                Some(start) => {
                    out.push((*pid, start));
                    out.push((*pid, self.clock(now)));
                }
                None => out.push((*pid, self.lobby_for(t))),
            }
        }
        self.status(out);
    }

    /// A player asks for a command: it goes in the log, for everyone (the engine of every
    /// browser accepts or refuses it the same way).
    pub fn request(&mut self, id: u64, token: &str, cmd: &Value, out: &mut Out) {
        let Some(f) = self.seat_of(token).filter(|_| self.started && !self.over) else {
            return;
        };
        let Some(c) = clean(cmd, f) else {
            Self::err(id, out, "That cannot be done online.");
            return;
        };
        if self.log.len() >= MAX_LOG {
            Self::err(id, out, "This game is too long to go on.");
            return;
        }
        self.log.push(c.clone());
        let seq = self.log.len();
        self.all(out, json!({ "t": "cmd", "seq": seq, "cmd": c }));
    }

    pub fn end_day(&mut self, token: &str, now: Instant, out: &mut Out) {
        let Some(f) = self.seat_of(token).filter(|_| self.started) else {
            return;
        };
        self.ready.insert(f);
        self.status(out);
        self.maybe_step(now, out);
    }

    fn maybe_step(&mut self, now: Instant, out: &mut Out) {
        if !self.started || self.over {
            return;
        }
        let away = self.away();
        let waiting = self
            .seats
            .keys()
            .any(|f| !self.ready.contains(f) && !away.contains(f));
        if !waiting {
            self.step(now, out);
        }
    }

    /// The clock: runs the day when its time is up.
    pub fn tick(&mut self, now: Instant, out: &mut Out) {
        if self.started && !self.over && self.paused.is_none() {
            if let Some(d) = self.deadline {
                if now >= d {
                    self.step(now, out);
                }
            }
        }
    }

    fn start_clock(&mut self, now: Instant) {
        let secs = pace_seconds(&self.pace);
        self.paused = None;
        self.deadline = (secs > 0 && !self.over).then(|| now + Duration::from_secs(secs));
    }

    fn step(&mut self, now: Instant, out: &mut Out) {
        if self.log.len() >= MAX_LOG {
            self.over = true;
            self.deadline = None;
            self.dirty = true;
            return;
        }
        self.log.push(json!({ "op": "step" }));
        self.ready.clear();
        self.fingerprint = None;
        self.last_active = now;
        let seq = self.log.len();
        self.all(out, json!({ "t": "step", "seq": seq }));
        self.start_clock(now);
        let clock = self.clock(now);
        self.all(out, clock);
        self.status(out);
    }

    /// A browser's fingerprint of the game after a day. The first one counts; a browser that
    /// differs (and the first, in case it was the one that went wrong) replays the game from
    /// the log.
    pub fn report(&mut self, seq: usize, fp: &str, out: &mut Out) {
        if seq != self.log.len() {
            return;
        }
        match &self.fingerprint {
            None => self.fingerprint = Some((seq, fp.to_string())),
            Some((_, first)) if first == fp => {}
            Some(_) => {
                eprintln!(
                    "room {}: fingerprints differ after command {seq}: everyone replays",
                    self.code
                );
                for (pid, t) in &self.peers {
                    if let Some(start) = self.start_for(t, 0) {
                        out.push((*pid, start));
                    }
                }
            }
        }
    }

    pub fn finish(&mut self) {
        if self.started && !self.over {
            self.over = true;
            self.deadline = None;
            self.paused = None;
            self.dirty = true;
        }
    }

    pub fn pause(&mut self, id: u64, token: &str, now: Instant, out: &mut Out) {
        if token != self.owner || !self.started || self.over {
            Self::err(id, out, "Only the host pauses the game.");
            return;
        }
        match (self.paused, self.deadline) {
            (Some(left), _) => {
                self.paused = None;
                self.deadline = Some(now + left);
            }
            (None, Some(d)) => {
                self.paused = Some(d.saturating_duration_since(now));
                self.deadline = None;
            }
            _ => return,
        }
        let clock = self.clock(now);
        self.all(out, clock);
    }

    /// The host hands a faction to the computer (someone who left for good).
    pub fn release(&mut self, id: u64, token: &str, f: u64, now: Instant, out: &mut Out) {
        let ok = token == self.owner
            && self.started
            && self.seats.get(&f).is_some_and(|t| *t != self.owner);
        if !ok {
            Self::err(
                id,
                out,
                "Only the host hands a friend's family to the computer.",
            );
            return;
        }
        self.seats.remove(&f);
        self.ready.remove(&f);
        self.dirty = true;
        let c = json!({ "op": "release", "faction": f });
        self.log.push(c.clone());
        let seq = self.log.len();
        self.all(out, json!({ "t": "cmd", "seq": seq, "cmd": c }));
        self.status(out);
        self.maybe_step(now, out);
    }

    /// A browser lost track: the whole game again.
    pub fn resync(&self, id: u64, token: &str, out: &mut Out) {
        if let Some(start) = self.start_for(token, 0).filter(|_| self.started) {
            out.push((id, start));
        }
    }
}

/// The command as the room keeps it: its op, the player's faction, and only the fields the op
/// takes (numbers, or short strings).
fn clean(cmd: &Value, f: u64) -> Option<Value> {
    let op = cmd["op"].as_str()?;
    let (_, keys) = OPS.iter().find(|(o, _)| *o == op)?;
    let mut c = Map::new();
    c.insert("op".into(), json!(op));
    c.insert("faction".into(), json!(f));
    for k in keys.iter() {
        match cmd.get(*k) {
            Some(v) if v.is_number() || v.as_str().is_some_and(|s| s.len() <= 64) => {
                c.insert((*k).to_string(), v.clone());
            }
            _ => {}
        }
    }
    Some(Value::Object(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(now: Instant) -> Room {
        Room::new(
            "ABCDE".into(),
            "marches".into(),
            "host".into(),
            json!({ "era": "medieval" }),
            7,
            "v1".into(),
            (0..5)
                .map(|i| json!({ "i": i, "name": format!("Realm{i}"), "color": "#000" }))
                .collect(),
            1,
            "wait".into(),
            now,
        )
    }

    fn of(out: &Out, id: u64, t: &str) -> Vec<Value> {
        out.iter()
            .filter(|(p, m)| *p == id && m["t"] == t)
            .map(|(_, m)| m.clone())
            .collect()
    }

    #[test]
    fn a_game_from_the_lobby_to_the_days() {
        let now = Instant::now();
        let mut r = room(now);
        let mut out = Out::new();
        r.join(1, "host", 0, now, &mut out);
        r.join(2, "guest", 0, now, &mut out);
        assert_eq!(of(&out, 2, "lobby").last().unwrap()["owner"], false);
        out.clear();
        r.pick(2, "guest", 1, now, &mut out);
        assert_eq!(of(&out, 2, "err").len(), 1, "taken by the host");
        r.pick(2, "guest", 3, now, &mut out);
        r.set_pace(2, "guest", "run-5", &mut out);
        assert_eq!(r.pace, "wait", "only the host sets the pace");
        r.set_pace(1, "host", "run-5", &mut out);
        out.clear();
        r.begin(1, "host", now, &mut out);
        let start = &of(&out, 2, "start")[0];
        assert_eq!(start["faction"], 3);
        assert_eq!(start["settings"]["players"], json!([1, 3]));
        assert!(of(&out, 2, "clock")[0]["ms"].as_u64().unwrap() > 4000);
        out.clear();
        // Commands go to everyone, as the player's faction, with only their own fields.
        r.request(
            2,
            "guest",
            &json!({ "op": "take", "decision": 4, "place": 9, "faction": 0, "map": 2 }),
            &mut out,
        );
        assert_eq!(
            of(&out, 1, "cmd")[0]["cmd"],
            json!({ "op": "take", "faction": 3, "decision": 4, "place": 9 })
        );
        r.request(2, "guest", &json!({ "op": "step" }), &mut out);
        assert_eq!(r.log.len(), 1, "a player cannot run the day");
        // The day runs when both have ended it.
        r.end_day("host", now, &mut out);
        assert_eq!(r.log.len(), 1);
        r.end_day("guest", now, &mut out);
        assert_eq!(r.log.last().unwrap()["op"], "step");
        assert!(r.ready.is_empty());
        // ... or when the clock says so.
        out.clear();
        r.tick(now + Duration::from_secs(6), &mut out);
        assert_eq!(of(&out, 1, "step")[0]["seq"], 3);
    }

    #[test]
    fn players_come_and_go() {
        let now = Instant::now();
        let mut r = room(now);
        let mut out = Out::new();
        r.join(1, "host", 0, now, &mut out);
        r.join(2, "guest", 0, now, &mut out);
        r.pick(2, "guest", 2, now, &mut out);
        r.begin(1, "host", now, &mut out);
        r.request(
            1,
            "host",
            &json!({ "op": "research", "tech": "roads" }),
            &mut out,
        );
        // The guest drops: the day no longer waits for them.
        out.clear();
        r.leave(2, now, &mut out);
        assert_eq!(r.away(), vec![2]);
        r.end_day("host", now, &mut out);
        assert_eq!(r.log.len(), 2);
        // Back with one command: only what is missing.
        out.clear();
        r.join(3, "guest", 1, now, &mut out);
        let start = &of(&out, 3, "start")[0];
        assert_eq!(
            (
                start["from"].clone(),
                start["commands"].as_array().unwrap().len()
            ),
            (json!(1), 1)
        );
        assert!(of(&out, 1, "note")[0]["msg"]
            .as_str()
            .unwrap()
            .contains("back"));
        // A latecomer takes over a faction from the computer.
        out.clear();
        r.join(4, "late", 0, now, &mut out);
        assert_eq!(of(&out, 4, "lobby")[0]["started"], true);
        r.pick(4, "late", 4, now, &mut out);
        assert_eq!(
            r.log.last().unwrap(),
            &json!({ "op": "claim", "faction": 4 })
        );
        assert_eq!(
            of(&out, 4, "start")[0]["commands"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        // A seat whose person left for good can be taken over (the host's too).
        out.clear();
        r.leave(1, now, &mut out);
        r.join(5, "newhost", 0, now, &mut out);
        r.pick(5, "newhost", 1, now, &mut out);
        assert_eq!(
            (r.seats[&1].as_str(), r.owner.as_str()),
            ("newhost", "newhost")
        );
        assert_eq!(of(&out, 5, "start")[0]["owner"], true);
        r.pick(4, "late", 1, now, &mut out);
        assert!(of(&out, 4, "err")
            .iter()
            .any(|e| e["msg"] == "You already play a family."));
        // The host hands a family back to the computer.
        let (host, hid) = ("newhost", 5);
        r.release(hid, host, 4, now, &mut out);
        assert_eq!(
            r.log.last().unwrap(),
            &json!({ "op": "release", "faction": 4 })
        );
        assert!(!r.seats.contains_key(&4));
    }

    #[test]
    fn fingerprints_that_differ_replay_the_game() {
        let now = Instant::now();
        let mut r = room(now);
        let mut out = Out::new();
        r.join(1, "host", 0, now, &mut out);
        r.join(2, "guest", 0, now, &mut out);
        r.pick(2, "guest", 2, now, &mut out);
        r.begin(1, "host", now, &mut out);
        r.end_day("host", now, &mut out);
        r.end_day("guest", now, &mut out);
        out.clear();
        r.report(1, "aa", &mut out);
        r.report(1, "aa", &mut out);
        assert!(out.is_empty());
        r.end_day("host", now, &mut out);
        r.end_day("guest", now, &mut out);
        out.clear();
        r.report(2, "bb", &mut out);
        r.report(2, "cc", &mut out);
        assert_eq!(of(&out, 1, "start").len(), 1);
        assert_eq!(of(&out, 2, "start").len(), 1);
    }

    #[test]
    fn the_host_pauses_the_clock() {
        let now = Instant::now();
        let mut r = room(now);
        let mut out = Out::new();
        r.join(1, "host", 0, now, &mut out);
        r.set_pace(1, "host", "run-10", &mut out);
        r.begin(1, "host", now, &mut out);
        r.pause(1, "host", now + Duration::from_secs(4), &mut out);
        r.tick(now + Duration::from_secs(60), &mut out);
        assert!(r.log.is_empty(), "paused");
        r.pause(1, "host", now + Duration::from_secs(60), &mut out);
        r.tick(now + Duration::from_secs(65), &mut out);
        assert!(r.log.is_empty(), "6 seconds were left");
        r.tick(now + Duration::from_secs(67), &mut out);
        assert_eq!(r.log.len(), 1);
    }
}
