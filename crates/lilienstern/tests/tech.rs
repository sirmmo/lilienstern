//! Tech trees: research points go into one tech at a time; techs unlock troops and give bonuses;
//! the AI picks its research, players pick theirs.

use std::collections::BTreeMap;
use std::path::Path;

use lilienstern::{Game, Params, World};

/// Two realms; Learning 10 a day per place held; knights need riding; roads speed every troop.
fn game() -> Game {
    let toml = r#"
name = "T"
scripts = ["s.lil"]
[city]
neighborhoods = 40
radius_km = 5
river = false
enclaves = 0
city_seed = 7
[[resource]]
id = "Gold"
scope = "global"
[[faction]]
id = "r"
kind = "realm"
count = 2
start = { Gold = 1000 }
start_presence = 50
[control]
hold = 20
margin = 1.3
decay = 0
contest = 1e9
attrition = 0
violence = 0
[[troop]]
id = "foot"
speed = 1
damage = { foot = 50, knight = 30 }
[[troop]]
id = "knight"
speed = 2
cost = { Gold = 10 }
damage = { foot = 80, knight = 50 }
requires = "riding"
[research]
resource = "Learning"
[[tech]]
id = "riding"
cost = 25
[[tech]]
id = "roads"
cost = 20
speed = 100
[[tech]]
id = "drill"
cost = 500
requires = ["roads"]
damage = 50
"#;
    let script = "event study\n scope faction\n do global Learning add 10\nend\n\
                  decision knights\n for realm\n target owned\n do raise knight\n weight 0\nend\n";
    Game::from_files(
        BTreeMap::from([
            ("game.toml".into(), toml.into()),
            ("s.lil".into(), script.into()),
        ]),
        &Params::default(),
    )
    .unwrap()
}

#[test]
fn research_unlocks_troops_and_carries_points_over() {
    let g = game();
    let mut w = World::new(&g, 1).unwrap();
    w.players = vec![0];
    let hq = w.factions[0].hq.unwrap();
    let raise = |w: &mut World| {
        w.options(0, Some(hq))
            .into_iter()
            .any(|o| o.name == "knights")
    };
    assert!(!raise(&mut w), "knights need riding");
    assert!(
        w.set_research(0, 2)
            .unwrap_err()
            .contains("cannot be researched"),
        "drill needs roads"
    );
    // A player who has chosen nothing keeps the points in the account.
    w.step();
    assert_eq!(w.research[0], None);
    w.set_research(0, 0).unwrap();
    w.step(); // 10 + 10 = 20
    w.step(); // 30: riding known, 5 left over
    assert!(w.known[0][0]);
    assert_eq!(w.progress[0], 5.0);
    assert_eq!(w.research[0], None, "a player picks the next");
    assert!(raise(&mut w), "knights can be raised now");
    // The AI researches on its own.
    assert!(w.known[1].iter().any(|&k| k) || w.research[1].is_some());
}

#[test]
fn techs_change_how_troops_move() {
    let g = game();
    let mut w = World::new(&g, 2).unwrap();
    w.players = vec![0];
    let hq = w.factions[0].hq.unwrap();
    let to = lilienstern::glassbox::around(&w.city.neighbours, hq, 4)
        .into_iter()
        .find(|&(_, d)| d == 4)
        .unwrap()
        .0;
    let id = w.raise(0, 0, hq);
    assert_eq!(w.route(id, to).unwrap().days, 4);
    w.set_research(0, 1).unwrap();
    w.step();
    w.step(); // roads: twice the speed
    assert!(w.known[0][1]);
    assert_eq!(w.route(id, to).unwrap().days, 2);
    assert!((w.bonus(0, 0).speed - 2.0).abs() < 1e-9);
}

#[test]
fn both_eras_have_tech_trees_and_marches_learns() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/marches");
    for era in ["medieval", "future"] {
        let mut p = Params::default();
        p.0.insert("era".into(), era.into());
        p.0.insert("provinces".into(), "120".into());
        let g = Game::load(&dir, &p).unwrap();
        assert!(g.war.techs.len() >= 8, "{era}");
        let mut w = World::new(&g, 3).unwrap();
        w.run(400);
        assert!(w.summary()["learnt"] > 0.0, "{era}: realms learn");
    }
}
