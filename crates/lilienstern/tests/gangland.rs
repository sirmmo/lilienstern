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
            && out.join("maps/partial/index.html").exists()
            && out.join("series/total/seed-1.csv").exists()
    );
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn plays_on_a_real_map_and_writes_a_viewer() {
    let g = small(&[("city_file", "maps/bologna.style.json"), ("gangs", "3")]);
    let mut w = World::new(&g, 2).unwrap();
    assert_eq!(w.city.len(), 90);
    assert!(w.city.imported && w.city.style.is_some());
    w.keep_history();
    w.run(30);
    assert_eq!(w.history.as_ref().unwrap().len(), 5); // days 0, 1, 10, 20, 30
    let files = lilienstern::viewer::files(&w).unwrap();
    let html = &files.iter().find(|f| f.0 == "index.html").unwrap().1;
    assert!(
        html.contains("maplibre-gl")
            && html.contains("San Luca")
            && !html.contains("</script>\n<script>\n</script>")
    );
    let style: serde_json::Value =
        serde_json::from_str(&files.iter().find(|f| f.0 == "style.json").unwrap().1).unwrap();
    assert_eq!(style["sources"]["lilienstern"]["data"], "map.geojson");
    assert!(
        style["sources"]["osm"].is_object(),
        "the imported style is the basemap"
    );
    let ids: Vec<&str> = style["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.last(), Some(&"lilienstern-line"));
}

#[test]
fn crews_and_techs_are_a_choice() {
    // Without them (the experiments' Gangland), no troops and no techs.
    let plain = small(&[]);
    assert!(plain.war.troops.is_empty() && plain.war.techs.is_empty());
    // With them, gangs raise crews, send them, fight, and learn.
    let g = small(&[("crews", "crews.toml")]);
    assert_eq!(g.war.troops.len(), 5);
    assert_eq!(g.war.techs.len(), 9);
    let mut w = World::new(&g, 3).unwrap();
    w.run(400);
    let s = w.summary();
    assert!(s["raised"] > 10.0, "gangs raise crews: {s:?}");
    assert!(s["battles"] > 0.0, "crews fight: {s:?}");
    let learnt: usize = (0..w.factions.len())
        .map(|f| w.known[f].iter().filter(|&&k| k).count())
        .sum();
    assert!(learnt >= 6, "gangs learn the trade: {learnt}");
    assert!(s["held"] > 0.5, "and still hold most of the city: {s:?}");
}

#[test]
fn the_books_name_the_business() {
    // Cash a gang makes comes from its units (GlassBox rules): the ledger says which.
    let g = small(&[]);
    let mut w = World::new(&g, 2).unwrap();
    w.run(30);
    let names = w.source_names();
    let cash = g
        .rules
        .resources
        .iter()
        .position(|r| r.id == "Cash")
        .unwrap();
    let ns = names.len();
    let gang = (0..w.factions.len())
        .find(|&f| g.factions[f].id.starts_with("gang") && w.factions[f].alive)
        .unwrap();
    let row = |label: &str| -> f64 {
        names
            .iter()
            .enumerate()
            .filter(|(_, (_, l))| l == label)
            .map(|(s, _)| w.ledger_total.amount[gang][cash * ns + s])
            .sum()
    };
    assert!(row("Street corner") > 0.0, "corners sell");
    assert!(row("Lab") < 0.0, "labs cost");
    let other = w.ledger_total.amount[gang][cash * ns + 3];
    assert!(other.abs() < 1e-6, "nothing left unexplained: {other}");
}
