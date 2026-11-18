//! The shipped game and experiments: they load, run, and run the same every time.

use std::path::{Path, PathBuf};

use lilienstern::experiment;
use lilienstern::geojson::to_geojson;
use lilienstern::{Game, Params, World};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn small(extra: &[(&str, &str)]) -> Game {
    let mut p = Params::default();
    p.0.insert("neighborhoods".into(), "200".into());
    for (k, v) in extra {
        p.0.insert(k.to_string(), v.to_string());
    }
    Game::load(&root().join("games/gangland"), &p).expect("gangland loads")
}

#[test]
fn runs_are_deterministic() {
    let g = small(&[]);
    let run = |seed| {
        let mut w = World::new(&g, seed).unwrap();
        w.run(120);
        (w.summary(), w.series.clone(), w.owner.clone())
    };
    let a = run(7);
    assert_eq!(a, run(7));
    assert_ne!(a.2, run(8).2);
    assert!(a.0["held"] > 0.0, "gangs hold something after 120 days");
}

#[test]
fn parameters_change_the_game() {
    let g = small(&[("gangs", "3")]);
    assert_eq!(
        g.factions
            .iter()
            .filter(|f| f.id.starts_with("gang"))
            .count(),
        3
    );
    let mut p = Params::default();
    p.0.insert("gnags".into(), "3".into());
    assert!(Game::load(&root().join("games/gangland"), &p)
        .unwrap_err()
        .contains("gnags"));
}

#[test]
fn events_fire_on_their_day() {
    let g = small(&[("shock_day", "5"), ("decap_day", "6")]);
    let mut w = World::new(&g, 1).unwrap();
    let wealth = g.rules.map("Wealth").unwrap();
    let east: Vec<usize> = (0..w.city.len())
        .filter(|&p| w.city.field(p, "x") > 0.0)
        .collect();
    w.run(5);
    let before: f64 = east.iter().map(|&p| w.gb.maps[wealth][p]).sum();
    w.run(2);
    let after: f64 = east.iter().map(|&p| w.gb.maps[wealth][p]).sum();
    assert!(after < before * 0.7, "{before} -> {after}");
    let s = w.summary();
    assert_eq!(s["m.Shocked"], east.len() as f64);
    assert_eq!(s["m.Decapitations"], 1.0);
}

#[test]
fn geojson_has_every_place() {
    let g = small(&[]);
    let mut w = World::new(&g, 3).unwrap();
    w.run(30);
    let j = to_geojson(&w);
    let features = j["features"].as_array().unwrap();
    assert_eq!(features.len(), w.city.len());
    let ring = features[0]["geometry"]["coordinates"][0]
        .as_array()
        .unwrap();
    assert_eq!(ring.first(), ring.last());
    assert!(features
        .iter()
        .any(|f| f["properties"]["owner"].is_string()));
}

#[test]
fn shipped_experiments_load() {
    let dir = root().join("experiments");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let path = e.unwrap().path();
        if path.extension().is_some_and(|x| x == "toml") {
            let (def, games) =
                experiment::load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(def.variants.len(), games.len());
            n += 1;
        }
    }
    assert!(n >= 5);
}

#[test]
fn an_experiment_writes_its_report() {
    let out = std::env::temp_dir().join(format!("lilienstern-test-{}", std::process::id()));
    let path = root().join("experiments/decapitation.toml");
    let o = experiment::run(
        &path,
        &experiment::Options {
            out: out.clone(),
            threads: 2,
            seeds: Some(2),
            days: Some(40),
            progress: false,
        },
    )
    .unwrap();
    assert_eq!(o.runs.len(), 6);
    assert!(o.report.contains("| `violence` |"));
    assert!(
        out.join("summary.csv").exists()
            && out.join("maps/partial.geojson").exists()
            && out.join("series/total/seed-1.csv").exists()
    );
    std::fs::remove_dir_all(out).ok();
}
