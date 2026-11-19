//! Rooms on disk, so that games survive a restart of the server: `<code>.json` holds the
//! room's settings and seats, `<code>.log` its commands, one JSON line each, appended as they
//! come. A room nobody has touched for `KEEP_DAYS` days is deleted.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};

use crate::room::Room;

pub const KEEP_DAYS: u64 = 14;

/// Room codes are letters and digits only (they name files).
pub fn valid_code(code: &str) -> bool {
    (4..=8).contains(&code.len())
        && code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn paths(dir: &Path, code: &str) -> (PathBuf, PathBuf) {
    (
        dir.join(format!("{code}.json")),
        dir.join(format!("{code}.log")),
    )
}

/// Writes what changed: the settings if they did, and the commands not yet written.
pub fn save(dir: &Path, r: &mut Room) -> std::io::Result<()> {
    let (meta, log) = paths(dir, &r.code);
    if r.dirty {
        let m = json!({
            "code": r.code, "game": r.game, "owner": r.owner, "params": r.params, "seed": r.seed,
            "version": r.version, "families": r.families, "pace": r.pace, "started": r.started,
            "over": r.over, "players": r.players,
            "seats": r.seats.iter().map(|(f, t)| json!([f, t])).collect::<Vec<_>>(),
        });
        let tmp = meta.with_extension("json.tmp");
        fs::write(&tmp, m.to_string())?;
        fs::rename(&tmp, &meta)?;
        r.dirty = false;
    }
    if r.saved < r.log.len() {
        let mut f = OpenOptions::new().create(true).append(true).open(&log)?;
        let mut text = String::new();
        for c in &r.log[r.saved..] {
            text.push_str(&c.to_string());
            text.push('\n');
        }
        f.write_all(text.as_bytes())?;
        r.saved = r.log.len();
    }
    Ok(())
}

/// A room from disk, if there is one.
pub fn load(dir: &Path, code: &str, now: Instant) -> Option<Room> {
    if !valid_code(code) {
        return None;
    }
    let (meta, log) = paths(dir, code);
    let m: Value = serde_json::from_str(&fs::read_to_string(meta).ok()?).ok()?;
    let mut r = Room::new(
        code.to_string(),
        m["game"].as_str()?.to_string(),
        m["owner"].as_str()?.to_string(),
        m["params"].clone(),
        m["seed"].as_u64()?,
        m["version"].as_str()?.to_string(),
        m["families"].as_array()?.clone(),
        0,
        m["pace"].as_str()?.to_string(),
        now,
    );
    r.seats.clear();
    for s in m["seats"].as_array()? {
        r.seats.insert(s[0].as_u64()?, s[1].as_str()?.to_string());
    }
    r.started = m["started"].as_bool()?;
    r.over = m["over"].as_bool().unwrap_or(false);
    r.players = m["players"]
        .as_array()?
        .iter()
        .filter_map(Value::as_u64)
        .collect();
    if let Ok(f) = fs::File::open(log) {
        for line in BufReader::new(f).lines() {
            // A line cut short by a crash is the last one: what came before stands.
            match line.ok().and_then(|l| serde_json::from_str(&l).ok()) {
                Some(c) => r.log.push(c),
                None => break,
            }
        }
    }
    r.saved = r.log.len();
    r.dirty = false;
    if r.started && !r.over {
        // The clock starts again from now.
        let secs = crate::room::pace_seconds(&r.pace);
        r.deadline = (secs > 0).then(|| now + Duration::from_secs(secs));
    }
    Some(r)
}

/// Deletes rooms nobody has touched for `KEEP_DAYS` days.
pub fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let limit = Duration::from_secs(KEEP_DAYS * 86_400);
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > limit);
        if old {
            let _ = fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::Out;

    #[test]
    fn a_room_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("lil-store-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let now = Instant::now();
        let mut r = Room::new(
            "QWERT".into(),
            "italia".into(),
            "host".into(),
            json!({ "map": "province" }),
            3,
            "v".into(),
            vec![
                json!({ "i": 0, "name": "Savoia" }),
                json!({ "i": 1, "name": "Visconti" }),
            ],
            0,
            "run-5".into(),
            now,
        );
        let mut out = Out::new();
        r.join(1, "host", 0, now, &mut out);
        r.join(2, "guest", 0, now, &mut out);
        r.pick(2, "guest", 1, now, &mut out);
        r.begin(1, "host", now, &mut out);
        r.request(
            2,
            "guest",
            &json!({ "op": "order", "troop": 4, "to": 9 }),
            &mut out,
        );
        save(&dir, &mut r).unwrap();
        r.end_day("host", now, &mut out);
        r.end_day("guest", now, &mut out);
        save(&dir, &mut r).unwrap();
        let back = load(&dir, "QWERT", now).unwrap();
        assert_eq!(back.log, r.log);
        assert_eq!(back.seats, r.seats);
        assert_eq!((back.started, back.players.clone()), (true, vec![0, 1]));
        assert!(back.deadline.is_some(), "the clock goes on");
        assert!(load(&dir, "../etc", now).is_none());
        fs::remove_dir_all(&dir).unwrap();
    }
}
