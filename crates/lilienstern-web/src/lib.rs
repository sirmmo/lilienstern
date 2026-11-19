//! Lilienstern in the browser. The page sends JSON requests and gets JSON answers through three
//! functions, with no generated glue:
//!
//! ```js
//! const bytes = new TextEncoder().encode(JSON.stringify(request));
//! const ptr = wasm.lil_alloc(bytes.length);
//! new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
//! const out = wasm.lil_call(ptr, bytes.length);       // frees the request
//! const answer = JSON.parse(new TextDecoder().decode(new Uint8Array(wasm.memory.buffer, out, wasm.lil_out_len())));
//! ```
//!
//! Requests (`op`): `new` (start a game from files in memory, with the factions people play
//! as `players`), `state`, `place`, `options`, `take`, `step`, `summary`, for troops `route`
//! (where a troop would go, and how long it takes) and `order` (send it), `release` (a player's
//! faction goes back to the AI) and `fingerprint` (of the whole state, to check that copies of
//! a game played in several browsers run in step). Requests that act for a player say which with
//! `faction` (default: the first player). Answers carry `error` when something went wrong.

use std::cell::RefCell;
use std::collections::BTreeMap;

use lilienstern::geojson::to_geojson;
use lilienstern::script::Kind;
use lilienstern::{Game, Params, World};
use serde_json::{json, Value};

struct Session {
    world: World<'static>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Room for a request of `len` bytes.
#[no_mangle]
pub extern "C" fn lil_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Answers the request at `ptr` (and frees it); the answer is at the returned pointer, for
/// [`lil_out_len`] bytes, until the next call.
///
/// # Safety
/// `ptr` must come from [`lil_alloc`] with `len` bytes written.
#[no_mangle]
pub unsafe extern "C" fn lil_call(ptr: *mut u8, len: usize) -> *const u8 {
    let bytes = Vec::from_raw_parts(ptr, len, len.max(1));
    let answer = match std::str::from_utf8(&bytes)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
    {
        Some(req) => handle(&req).unwrap_or_else(|e| json!({ "error": e })),
        None => json!({ "error": "the request is not JSON" }),
    };
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        *o = answer.to_string().into_bytes();
        o.as_ptr()
    })
}

#[no_mangle]
pub extern "C" fn lil_out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}

/// Answers one request (also usable natively, for tests).
pub fn handle(req: &Value) -> Result<Value, String> {
    let op = req["op"].as_str().ok_or("no op")?;
    if op == "new" {
        return start(req);
    }
    SESSION.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut().ok_or("no game: send `new` first")?;
        let w = &mut s.world;
        match op {
            "state" => {
                let f = faction_arg(w, req).ok();
                Ok(state(w, req["map"].as_u64().map(|m| m as usize), f))
            }
            "fingerprint" => {
                Ok(json!({ "fingerprint": format!("{:016x}", w.fingerprint()), "day": w.day }))
            }
            "claim" => {
                // A person takes over a faction the AI was playing.
                let f = match &req["faction"] {
                    Value::Number(n) => n.as_u64().map(|i| i as usize),
                    Value::String(id) => w.game.factions.iter().position(|f| &f.id == id),
                    _ => None,
                }
                .filter(|&f| f < w.game.factions.len())
                .ok_or("which faction?")?;
                if !w.players.contains(&f) {
                    w.players.push(f);
                }
                Ok(json!({ "players": w.players }))
            }
            "release" => {
                let f = faction_arg(w, req)?;
                w.players.retain(|&p| p != f);
                Ok(json!({ "players": w.players }))
            }
            "place" => {
                let p = place_arg(w, &req["place"])?.ok_or("which place?")?;
                let f = faction_arg(w, req).ok();
                Ok(place(w, p, f))
            }
            "options" => {
                let f = faction_arg(w, req)?;
                let p = place_arg(w, &req["place"])?;
                Ok(offers(w, f, p))
            }
            "take" => {
                let f = faction_arg(w, req)?;
                let d = req["decision"].as_u64().ok_or("which decision?")? as usize;
                let p = place_arg(w, &req["place"])?;
                w.take(f, d, p)?;
                Ok(state(w, req["map"].as_u64().map(|m| m as usize), Some(f)))
            }
            "step" => {
                let days = req["days"].as_u64().unwrap_or(1).min(3650) as u32;
                for _ in 0..days {
                    if w.day >= w.game.days {
                        break;
                    }
                    w.step();
                }
                let f = faction_arg(w, req).ok();
                Ok(state(w, req["map"].as_u64().map(|m| m as usize), f))
            }
            "summary" => Ok(json!(w.summary())),
            "route" | "order" => {
                let f = faction_arg(w, req)?;
                let id = req["troop"].as_u64().ok_or("which troop?")? as u32;
                let to = place_arg(w, &req["to"])?.ok_or("where to?")?;
                if !w.troop(id).is_some_and(|t| t.faction == f) {
                    return Err("that troop is not yours".into());
                }
                let r = if op == "order" {
                    w.order(f, id, to)?
                } else {
                    w.route(id, to).ok_or("your troop cannot get there")?
                };
                Ok(json!({ "path": r.path, "days": r.days }))
            }
            _ => Err(format!("unknown op {op:?}")),
        }
    })
}

/// The player a request acts for: `faction` (an index or an id), or the first player.
fn faction_arg(w: &World, req: &Value) -> Result<usize, String> {
    let f = match &req["faction"] {
        Value::Number(n) => n.as_u64().map(|i| i as usize),
        Value::String(id) => w.game.factions.iter().position(|f| &f.id == id),
        _ => {
            return w
                .players
                .first()
                .copied()
                .ok_or_else(|| "nobody plays".to_string())
        }
    }
    .ok_or("no such faction")?;
    if !w.players.contains(&f) {
        return Err(format!(
            "{} is not played by a person",
            w.game.factions.get(f).map_or("?", |d| d.name.as_str())
        ));
    }
    Ok(f)
}

fn place_arg(w: &World, v: &Value) -> Result<Option<usize>, String> {
    match v.as_u64() {
        Some(p) if (p as usize) < w.city.len() => Ok(Some(p as usize)),
        Some(p) => Err(format!("no place {p}")),
        None => Ok(None),
    }
}

fn start(req: &Value) -> Result<Value, String> {
    let files: BTreeMap<String, String> = req["files"]
        .as_object()
        .ok_or("files: an object of file name to text")?
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
        .collect();
    let mut params = Params::default();
    if let Some(p) = req["params"].as_object() {
        for (k, v) in p {
            let text = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            params.0.insert(k.clone(), text);
        }
    }
    let game = Game::from_files(files, &params)?;
    // The page keeps one game at a time; a new one replaces it (the old rules are leaked,
    // a few kilobytes).
    let game: &'static Game = Box::leak(Box::new(game));
    let seed = req["seed"].as_u64().unwrap_or(1);
    let mut world = World::new(game, seed)?;
    let one = |v: &Value| match v {
        Value::Number(n) => n
            .as_u64()
            .map(|i| i as usize)
            .filter(|&i| i < game.factions.len()),
        Value::String(id) => game.factions.iter().position(|f| &f.id == id),
        _ => None,
    };
    world.players = match &req["players"] {
        Value::Array(a) => a.iter().filter_map(one).collect(),
        _ => one(&req["player"]).into_iter().collect(),
    };
    world.players.dedup();
    let info = json!({
        "name": game.name,
        "days": game.days,
        "places": world.city.len(),
        "geojson": to_geojson(&world),
        "factions": game.factions.iter().map(|f| json!({ "id": f.id, "name": f.name, "color": f.color, "gang": f.kind == Kind::Gang, "holds": f.kind.holds(), "actions": f.actions })).collect::<Vec<_>>(),
        "troopKinds": game.war.troops.iter().map(|t| json!({
            "id": t.id, "name": t.name, "icon": t.icon, "speed": t.speed, "move": game.war.moves[t.move_type],
            "occupy": t.occupy, "description": t.description,
            "cost": t.cost.iter().map(|&(r, v)| json!({ "resource": game.rules.resources[r].id, "amount": v })).collect::<Vec<_>>(),
            "damage": game.war.troops.iter().zip(&t.damage).map(|(o, d)| (o.id.clone(), json!(d))).collect::<serde_json::Map<_, _>>(),
        })).collect::<Vec<_>>(),
        "terrains": game.war.terrains.iter().map(|t| json!({ "id": t.id, "name": t.name, "color": t.color, "defense": t.defense })).collect::<Vec<_>>(),
        "terrain": world.terrain.iter().map(|t| t.map_or(-1, |t| t as i64)).collect::<Vec<_>>(),
        "kmDistances": game.war.km,
        "maps": game.rules.maps.iter().map(|m| m.id.clone()).collect::<Vec<_>>(),
        "player": world.players.first(),
        "players": world.players,
        "credit": world.city.attribution,
        "state": state(&world, None, world.players.first().copied()),
    });
    SESSION.with(|s| *s.borrow_mut() = Some(Session { world }));
    Ok(info)
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// The state of the game, seen by faction `me` (its rank, actions left and relations).
fn state(w: &World, map: Option<usize>, me: Option<usize>) -> Value {
    let g = w.game;
    let accounts: Vec<Value> = w
        .factions
        .iter()
        .map(|f| {
            let mut m = serde_json::Map::new();
            for (r, res) in g.rules.resources.iter().enumerate() {
                if res.scope == lilienstern::glassbox::Scope::Global {
                    m.insert(res.id.clone(), json!(round1(w.gb.accounts[f.account][r])));
                }
            }
            Value::Object(m)
        })
        .collect();
    let values = map
        .filter(|&m| m < g.rules.maps.len())
        .map(|m| w.gb.maps[m].iter().map(|&v| round1(v)).collect::<Vec<_>>());
    let s = w.summary();
    json!({
        "day": w.day,
        "over": w.day >= g.days,
        "owner": w.owner.iter().map(|o| o.map_or(-1, |f| f as i64)).collect::<Vec<_>>(),
        "territory": w.factions.iter().map(|f| f.territory).collect::<Vec<_>>(),
        "alive": w.factions.iter().map(|f| f.alive).collect::<Vec<_>>(),
        "rank": me.map(|f| w.rank(f)),
        "accounts": accounts,
        "actions_left": me.map(|f| g.factions[f].actions.saturating_sub(w.player_actions[f])),
        "players": w.players,
        "fingerprint": format!("{:016x}", w.fingerprint()),
        "values": values,
        "violence": s["violence"],
        "captures": s["captures"],
        "hotspots_today": w.hotspots,
        "raids": s.get("m.Raids"),
        "troops": troops(w),
        "fights": fights(w),
        "relations": me.map(|f| w.relation[f].iter().map(|r| r.name()).collect::<Vec<_>>()),
        "battles": s.get("battles"),
        "destroyed": s.get("destroyed"),
    })
}

/// Places where troops of factions at war share the place: battles go on there.
fn fights(w: &World) -> Vec<usize> {
    let mut at: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for t in &w.troops {
        let fs = at.entry(t.place).or_default();
        if !fs.contains(&t.faction) {
            fs.push(t.faction);
        }
    }
    at.into_iter()
        .filter(|(_, fs)| fs.iter().any(|&a| fs.iter().any(|&b| w.hostile(a, b))))
        .map(|(p, _)| p)
        .collect()
}

/// Every troop: id, faction, kind, place, next place (-1: none), share of the way there,
/// health; and for the player's, the places still ahead.
fn troops(w: &World) -> Value {
    Value::Array(
        w.troop_positions()
            .into_iter()
            .map(|(id, f, k, p, n, s, hp)| {
                let mut t = json!({ "id": id, "f": f, "k": k, "p": p, "n": n, "s": (f64::from(s) * 100.0).round() / 100.0, "hp": hp });
                if w.players.contains(&(f as usize)) {
                    if let Some(tr) = w.troop(id) {
                        t["path"] = json!(tr.path);
                    }
                }
                t
            })
            .collect(),
    )
}

fn place(w: &mut World, p: usize, me: Option<usize>) -> Value {
    let g = w.game;
    let pl = &w.city.places[p];
    let presence: Vec<Value> = w
        .holders
        .iter()
        .filter(|&&h| w.presence[h][p] > 0.5)
        .map(|&h| json!({ "faction": h, "presence": round1(w.presence[h][p]) }))
        .collect();
    let units: Vec<Value> = w.units_at[p]
        .iter()
        .map(|&u| {
            let unit = &w.gb.units[u];
            json!({ "unit": g.rules.units[unit.def].name, "faction": unit.account.checked_sub(1) })
        })
        .collect();
    let maps: serde_json::Map<String, Value> = g
        .rules
        .maps
        .iter()
        .enumerate()
        .map(|(m, d)| (d.id.clone(), json!(round1(w.gb.maps[m][p]))))
        .collect();
    let here: Vec<Value> = w
        .troops
        .iter()
        .filter(|t| t.place == p)
        .map(|t| json!({ "id": t.id, "faction": t.faction, "kind": t.kind, "hp": t.hp.round(), "moving": !t.path.is_empty(), "to": t.path.last() }))
        .collect();
    let mut out = json!({
        "place": p,
        "name": pl.name,
        "owner": w.owner[p],
        "troops": here,
        "terrain": w.terrain[p],
        "presence": presence,
        "units": units,
        "maps": maps,
        "population": pl.fields.get("population").map(|v| v.round()),
    });
    if let Some(f) = me {
        out["options"] = offers(w, f, Some(p));
    }
    out
}

fn offers(w: &mut World, f: usize, p: Option<usize>) -> Value {
    let left = w.game.factions[f]
        .actions
        .saturating_sub(w.player_actions[f]);
    json!(w
        .options(f, p)
        .into_iter()
        .map(|o| json!({
            "decision": o.decision,
            "name": o.name,
            "label": o.label,
            "affordable": o.affordable && left > 0,
            "raise": w.game.script.decisions[o.decision].effects.iter().find_map(|e| match e {
                lilienstern::script::Effect::Raise { troop } => Some(w.game.war.troops[*troop].name.clone()),
                _ => None,
            }),
            "costs": o.costs.iter().map(|(r, v)| json!({ "resource": r, "amount": v })).collect::<Vec<_>>(),
        }))
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Value {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/gangland/");
        let mut m = serde_json::Map::new();
        for f in [
            "params.toml",
            "game.toml",
            "economy.gbx",
            "politics.lil",
            "maps/bologna-areas.geojson",
        ] {
            m.insert(
                f.into(),
                json!(std::fs::read_to_string(format!("{dir}{f}")).unwrap()),
            );
        }
        Value::Object(m)
    }

    fn marches() -> Value {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/marches/");
        let mut m = serde_json::Map::new();
        for f in [
            "params.toml",
            "game.toml",
            "realms.lil",
            "eras/medieval.toml",
            "eras/future.toml",
        ] {
            m.insert(
                f.into(),
                json!(std::fs::read_to_string(format!("{dir}{f}")).unwrap()),
            );
        }
        Value::Object(m)
    }

    #[test]
    fn a_player_marches_troops() {
        let info = handle(&json!({ "op": "new", "files": marches(), "params": { "provinces": 120, "era": "future" }, "seed": 5, "player": "realm2" })).unwrap();
        assert_eq!(info["troopKinds"][3]["name"], "Hovertank");
        assert_eq!(
            info["terrain"].as_array().unwrap().len(),
            info["places"].as_u64().unwrap() as usize
        );
        let st = &info["state"];
        let mine: Vec<&Value> = st["troops"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["f"] == 1)
            .collect();
        assert_eq!(mine.len(), 3, "three troops at the start");
        let id = mine[0]["id"].clone();
        let from = mine[0]["p"].as_u64().unwrap() as usize;
        // Somewhere it can reach, a few places away.
        let n = info["places"].as_u64().unwrap();
        let (to, route) = (0..n)
            .filter(|&p| p as usize != from)
            .find_map(|p| {
                handle(&json!({ "op": "route", "troop": id, "to": p }))
                    .ok()
                    .filter(|r| r["path"].as_array().unwrap().len() >= 3)
                    .map(|r| (p, r))
            })
            .unwrap();
        let days = route["days"].as_u64().unwrap();
        let order = handle(&json!({ "op": "order", "troop": id, "to": to })).unwrap();
        assert_eq!(order, route);
        let st = handle(&json!({ "op": "state" })).unwrap();
        let me = st["troops"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == id)
            .unwrap()
            .clone();
        assert!(!me["path"].as_array().unwrap().is_empty());
        assert_eq!(st["relations"][1], "ally", "a realm is its own ally");
        // Others' troops cannot be ordered.
        let theirs = st["troops"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["f"] != 1)
            .unwrap()["id"]
            .clone();
        assert!(handle(&json!({ "op": "order", "troop": theirs, "to": to })).is_err());
        let later = handle(&json!({ "op": "step", "days": days + 1 })).unwrap();
        if let Some(t) = later["troops"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == id)
        {
            assert!(t["p"] != from as u64, "it has left: {t}");
        }
        // Raising: the decision carries the troop's name and cost.
        let held = later["owner"]
            .as_array()
            .unwrap()
            .iter()
            .position(|o| o == 1)
            .unwrap();
        let card = handle(&json!({ "op": "place", "place": held })).unwrap();
        let raise: Vec<&Value> = card["options"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["raise"].is_string())
            .collect();
        assert!(raise.iter().any(|o| o["raise"] == "Battle mech"), "{card}");
    }

    #[test]
    fn two_players_share_a_game() {
        let info = handle(&json!({ "op": "new", "files": marches(), "params": { "provinces": 100 }, "seed": 2, "players": ["realm1", 2] })).unwrap();
        assert_eq!(info["players"], json!([0, 2]));
        let hq = |f: u64| {
            let st = handle(&json!({ "op": "state", "faction": f })).unwrap();
            st["owner"]
                .as_array()
                .unwrap()
                .iter()
                .position(|o| o == f)
                .unwrap()
        };
        let p2 = hq(2);
        let card = handle(&json!({ "op": "place", "place": p2, "faction": 2 })).unwrap();
        let raise = card["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["raise"].is_string())
            .unwrap()
            .clone();
        let after = handle(
            &json!({ "op": "take", "decision": raise["decision"], "place": p2, "faction": 2 }),
        )
        .unwrap();
        assert_eq!(
            after["actions_left"], 1,
            "the second player used one of its two"
        );
        let first = handle(&json!({ "op": "state", "faction": 0 })).unwrap();
        assert_eq!(first["actions_left"], 2, "the first player still has both");
        // Not a player: refused.
        assert!(handle(&json!({ "op": "options", "faction": 1 }))
            .unwrap_err()
            .contains("not played"));
        let fp = handle(&json!({ "op": "fingerprint" })).unwrap();
        assert_eq!(fp["fingerprint"], first["fingerprint"]);
        // A player who leaves: the AI takes the faction back.
        let left = handle(&json!({ "op": "release", "faction": 2 })).unwrap();
        assert_eq!(left["players"], json!([0]));
    }

    #[test]
    fn a_player_plays_a_gang_on_bologna() {
        let info = handle(&json!({ "op": "new", "files": files(), "params": { "city_file": "maps/bologna-areas.geojson", "gangs": 4 }, "seed": 3, "player": "gang2" })).unwrap();
        assert_eq!(info["places"], 90);
        assert_eq!(info["player"], 1);
        let st = &info["state"];
        assert_eq!(st["actions_left"], 3);
        assert!(st["accounts"][1]["Cash"].as_f64().unwrap() > 0.0);
        // A place the player holds offers building; take one decision there.
        let held = st["owner"]
            .as_array()
            .unwrap()
            .iter()
            .position(|o| o == 1)
            .unwrap();
        let card = handle(&json!({ "op": "place", "place": held })).unwrap();
        let opts = card["options"].as_array().unwrap();
        assert!(opts.iter().any(|o| o["name"] == "open_corner"), "{opts:?}");
        let pick = opts.iter().find(|o| o["affordable"] == true).unwrap();
        let after =
            handle(&json!({ "op": "take", "decision": pick["decision"], "place": held })).unwrap();
        assert_eq!(after["actions_left"], 2, "{after}");
        let bad = handle(&json!({ "op": "take", "decision": 999, "place": held })).unwrap_err();
        assert!(bad.contains("no such decision"));
        let later = handle(&json!({ "op": "step", "days": 20, "map": 0 })).unwrap();
        assert_eq!(later["day"], 20);
        assert_eq!(later["actions_left"], 3);
        assert_eq!(later["values"].as_array().unwrap().len(), 90);
        // Decisions on the gang itself: only hiring, offered while it is short of crew.
        let own = handle(&json!({ "op": "options" })).unwrap();
        assert!(
            own.as_array().unwrap().iter().all(|o| o["name"] == "hire"),
            "{own}"
        );
    }
}
