//! Italia: ISTAT's 1871 units, families at their seats, islands joined by sea, and the
//! engine features it needs (links, connect, capital, income by map).

use std::collections::BTreeMap;
use std::path::Path;

use lilienstern::{Game, Params, World};

fn italia(map: &str, families: u32) -> Game {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/italia");
    let mut p = Params::default();
    p.0.insert("map".into(), map.into());
    p.0.insert("families".into(), families.to_string());
    Game::load(&dir, &p).unwrap()
}

#[test]
fn every_area_of_1871_italy_is_on_the_map() {
    for (map, places) in [("circondari", 284), ("province", 69)] {
        let g = italia(map, 16);
        let w = World::new(&g, 1).unwrap();
        assert_eq!(
            w.city.len(),
            places,
            "{map}: islands are joined, not dropped"
        );
        let names: Vec<&str> = w.city.places.iter().map(|p| p.name.as_str()).collect();
        assert!(
            names.contains(&"Palermo") && names.contains(&"Cagliari"),
            "{map}"
        );
    }
}

#[test]
fn families_start_at_their_seats() {
    let g = italia("circondari", 16);
    let w = World::new(&g, 3).unwrap();
    let seat = |f: usize| w.city.places[w.factions[f].hq.unwrap()].name.clone();
    assert_eq!(g.factions[1].name, "Visconti");
    assert_eq!(seat(1), "Milano");
    assert_eq!(seat(6), "Napoli");
    assert_eq!(seat(10), "Urbino");
    let g = italia("province", 16);
    let w = World::new(&g, 3).unwrap();
    assert_eq!(
        w.city.places[w.factions[10].hq.unwrap()].name,
        "Pesaro e Urbino",
        "the second name"
    );
}

#[test]
fn italia_plays_and_napoli_pays_more_than_the_alps() {
    let g = italia("circondari", 8);
    let mut w = World::new(&g, 5).unwrap();
    let pop = g.rules.map("Population").unwrap();
    let napoli = w
        .city
        .places
        .iter()
        .position(|p| p.name == "Napoli")
        .unwrap();
    let aosta = w
        .city
        .places
        .iter()
        .position(|p| p.name == "Aosta")
        .unwrap();
    // 574,000 people on 247 km² against 247,000 on 3,472 km² (1871).
    assert!(w.gb.maps[pop][napoli] > 2.0 * w.gb.maps[pop][aosta]);
    w.run(200);
    let s = w.summary();
    assert!(s["battles"] > 0.0 && s["raised"] > 24.0, "{s:?}");
}

/// A tiny imported map: two islands of two places each, a link by name and `connect`.
fn islands(extra: &str) -> Result<Game, String> {
    let sq = |x: f64, y: f64, name: &str| {
        format!(
            r#"{{"type":"Feature","properties":{{"name":"{name}","population":1000}},"geometry":{{"type":"Polygon","coordinates":[[[{x},{y}],[{x1},{y}],[{x1},{y1}],[{x},{y1}],[{x},{y}]]]}}}}"#,
            x1 = x + 0.1,
            y1 = y + 0.1
        )
    };
    let geo = format!(
        r#"{{"type":"FeatureCollection","features":[{},{},{},{}]}}"#,
        sq(10.0, 40.0, "A"),
        sq(10.1, 40.0, "B"),
        sq(11.0, 40.0, "C"),
        sq(11.1, 40.0, "D")
    );
    let game = format!(
        "name = \"I\"\nscripts = [\"s.lil\"]\n[city]\ngeojson = \"m.geojson\"\n{extra}\n[[map]]\nid = \"Population\"\ninit = \"population\"\nscale = 0.001\n[[resource]]\nid = \"Gold\"\nscope = \"global\"\n[[faction]]\nid = \"r\"\nkind = \"realm\"\ncapital = \"D\"\nstart_presence = 50\n[control]\nhold = 20\nmargin = 1.3\ndecay = 0\ncontest = 1e9\nattrition = 0\nviolence = 0\n"
    );
    let script = "event pay\n scope place\n for realm\n do global Gold add map Population 2\nend\n";
    Game::from_files(
        BTreeMap::from([
            ("game.toml".into(), game),
            ("s.lil".into(), script.into()),
            ("m.geojson".into(), geo),
        ]),
        &Params::default(),
    )
}

#[test]
fn islands_are_joined_by_links_or_connect() {
    let g = islands("").unwrap();
    assert_eq!(
        World::new(&g, 1).unwrap().city.len(),
        2,
        "without them, one island is dropped"
    );
    for extra in ["connect = true", "links = [[\"b\", \"C\"]]"] {
        let g = islands(extra).unwrap();
        let w = World::new(&g, 1).unwrap();
        assert_eq!(w.city.len(), 4, "{extra}");
        let (b, c) = (1, 2);
        assert!(
            w.city.neighbours[b].contains(&c),
            "{extra}: B and C are the closest pair"
        );
    }
    // A name the map does not have: an error when the map is loaded.
    let g = islands("links = [[\"A\", \"Atlantis\"]]").unwrap();
    let e = World::new(&g, 1).err().unwrap();
    assert!(e.contains("Atlantis"), "{e}");
}

#[test]
fn capitals_and_income_by_map() {
    let g = islands("connect = true").unwrap();
    let mut w = World::new(&g, 1).unwrap();
    assert_eq!(
        w.city.places[w.factions[0].hq.unwrap()].name,
        "D",
        "the capital, not a random place"
    );
    let held = w.owner.iter().filter(|o| o.is_some()).count() as f64;
    w.step();
    // Each place held pays 2 x its population in thousands (1).
    assert_eq!(w.gb.accounts[w.factions[0].account][0], 2.0 * held);
}
