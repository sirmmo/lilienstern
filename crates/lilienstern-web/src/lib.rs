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
//! Requests (`op`): `new` (start a game from files in memory), `state`, `place`, `options`,
//! `take`, `step`, `summary`. Answers carry `error` when something went wrong.

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
            "state" => Ok(state(w, req["map"].as_u64().map(|m| m as usize))),
            "place" => {
                let p = place_arg(w, &req["place"])?.ok_or("which place?")?;
                Ok(place(w, p))
            }
            "options" => {
                let f = w.player.ok_or("nobody plays")?;
                let p = place_arg(w, &req["place"])?;
                Ok(offers(w, f, p))
            }
            "take" => {
                let f = w.player.ok_or("nobody plays")?;
                let d = req["decision"].as_u64().ok_or("which decision?")? as usize;
                let p = place_arg(w, &req["place"])?;
                w.take(f, d, p)?;
                Ok(state(w, req["map"].as_u64().map(|m| m as usize)))
            }
            "step" => {
                let days = req["days"].as_u64().unwrap_or(1).min(3650) as u32;
                for _ in 0..days {
                    if w.day >= w.game.days {
                        break;
                    }
                    w.step();
                }
                Ok(state(w, req["map"].as_u64().map(|m| m as usize)))
            }
            "summary" => Ok(json!(w.summary())),
            _ => Err(format!("unknown op {op:?}")),
        }
    })
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
    world.player = match &req["player"] {
        Value::Number(n) => n
            .as_u64()
            .map(|i| i as usize)
            .filter(|&i| i < game.factions.len()),
        Value::String(id) => game.factions.iter().position(|f| &f.id == id),
        _ => None,
    };
    let info = json!({
        "name": game.name,
        "days": game.days,
        "places": world.city.len(),
        "geojson": to_geojson(&world),
        "factions": game.factions.iter().map(|f| json!({ "id": f.id, "name": f.name, "color": f.color, "gang": f.kind == Kind::Gang, "actions": f.actions })).collect::<Vec<_>>(),
        "maps": game.rules.maps.iter().map(|m| m.id.clone()).collect::<Vec<_>>(),
        "player": world.player,
        "credit": world.city.attribution,
        "state": state(&world, None),
    });
    SESSION.with(|s| *s.borrow_mut() = Some(Session { world }));
    Ok(info)
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn state(w: &World, map: Option<usize>) -> Value {
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
        "rank": w.player.map(|f| w.rank(f)),
        "accounts": accounts,
        "actions_left": w.player.map(|f| g.factions[f].actions.saturating_sub(w.player_actions)),
        "values": values,
        "violence": s["violence"],
        "captures": s["captures"],
        "hotspots_today": w.hotspots,
        "raids": s.get("m.Raids"),
    })
}

fn place(w: &mut World, p: usize) -> Value {
    let g = w.game;
    let pl = &w.city.places[p];
    let presence: Vec<Value> = w
        .gangs
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
    let mut out = json!({
        "place": p,
        "name": pl.name,
        "owner": w.owner[p],
        "presence": presence,
        "units": units,
        "maps": maps,
        "population": pl.fields.get("population").map(|v| v.round()),
    });
    if let Some(f) = w.player {
        out["options"] = offers(w, f, Some(p));
    }
    out
}

fn offers(w: &mut World, f: usize, p: Option<usize>) -> Value {
    let left = w.game.factions[f].actions.saturating_sub(w.player_actions);
    json!(w
        .options(f, p)
        .into_iter()
        .map(|o| json!({
            "decision": o.decision,
            "name": o.name,
            "label": o.label,
            "affordable": o.affordable && left > 0,
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
