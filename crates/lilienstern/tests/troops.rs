//! Troops: marching on the daily clock, battles, allies, peace, terrain, costs, and the Marches
//! example in both eras.

use std::collections::BTreeMap;
use std::path::Path;

use lilienstern::war::Relation;
use lilienstern::{Game, Params, World};

/// A small game in memory: a generated land of about 40 places and three realms, the first two
/// allied, with one kind of foot troop and one of horse.
fn tiny(extra: &str) -> Game {
    let game = format!(
        r#"
name = "Tiny"
scripts = ["s.lil"]
days = 50
[city]
neighborhoods = 40
radius_km = 5
river = false
enclaves = 0
city_seed = 7
[[map]]
id = "Violence"
[[resource]]
id = "Gold"
scope = "global"
[[faction]]
id = "r"
kind = "realm"
count = 3
team = "a,a,"
start = {{ Gold = 100 }}
start_presence = 50
start_radius = 1
[control]
hold = 20
margin = 1.3
decay = 0.0
contest = 1e9
attrition = 0
violence = 0
[war]
damage_scale = 0.5
[[troop]]
id = "foot"
speed = 1
cost = {{ Gold = 30 }}
damage = {{ foot = 50, horse = 40 }}
occupy = 5
[[troop]]
id = "horse"
move = "horse"
speed = 3
damage = {{ foot = 60, horse = 50 }}
{extra}
"#
    );
    // Weight 0: the AI never takes it, so only what a test does happens; a player can.
    let script = "decision levy\n for realm\n target owned\n do raise foot\n weight 0\nend\n";
    let files = BTreeMap::from([
        ("game.toml".to_string(), game),
        ("s.lil".to_string(), script.to_string()),
    ]);
    Game::from_files(files, &Params::default()).unwrap()
}

/// A place `steps` borders from `from`, as far from the realms' capitals as can be.
fn place_at(w: &World, from: usize, steps: u32) -> usize {
    let around = lilienstern::glassbox::around(&w.city.neighbours, from, steps);
    around
        .iter()
        .filter(|&&(_, d)| d == steps)
        .map(|&(p, _)| p)
        .next()
        .expect("a place that far")
}

#[test]
fn troops_march_at_their_speed() {
    let g = tiny("");
    let mut w = World::new(&g, 1).unwrap();
    let from = w.factions[0].hq.unwrap();
    let to = place_at(&w, from, 3);
    let id = w.raise(0, 0, from);
    let r = w.order(0, id, to).unwrap();
    assert_eq!(r.path.len(), 3);
    assert_eq!(r.days, 3, "speed 1, one border a day");
    let fast = w.raise(0, 1, from);
    assert_eq!(w.order(0, fast, to).unwrap().days, 1, "speed 3");
    w.step();
    assert_eq!(w.troop(id).unwrap().path.len(), 2);
    assert_eq!(w.troop(fast).unwrap().place, to);
    w.step();
    w.step();
    assert_eq!(w.troop(id).unwrap().place, to);
    assert!(w.troop(id).unwrap().path.is_empty());
}

#[test]
fn enemies_fight_and_allies_share() {
    let g = tiny("");
    let mut w = World::new(&g, 2).unwrap();
    let p = w.factions[0].hq.unwrap();
    let a = w.raise(0, 0, p);
    let b = w.raise(1, 0, p); // an ally
    w.step();
    assert_eq!(w.troop(a).unwrap().hp, 100.0);
    assert_eq!(w.troop(b).unwrap().hp, 100.0);
    let c = w.raise(2, 0, p); // an enemy
    w.step();
    let (ha, hc) = (w.troop(a).unwrap().hp, w.troop(c).unwrap().hp);
    assert!(ha < 100.0, "the enemy hits back");
    assert!(hc < 75.0, "two troops hit the enemy: {hc}");
    for _ in 0..10 {
        w.step();
    }
    assert!(w.troop(c).is_none(), "outnumbered, the enemy is destroyed");
    assert!(w.summary()["destroyed"] >= 1.0);
}

#[test]
fn peace_keeps_troops_out_and_war_lets_them_in() {
    let g = tiny("");
    let mut w = World::new(&g, 3).unwrap();
    let enemy_hq = w.factions[2].hq.unwrap();
    let id = w.raise(0, 0, w.factions[0].hq.unwrap());
    assert!(w.route(id, enemy_hq).is_some());
    w.set_relation(0, 2, Relation::Peace);
    assert!(
        w.route(id, enemy_hq).is_none(),
        "at peace, its places are closed"
    );
    w.set_relation(0, 2, Relation::War);
    assert!(w.order(0, id, enemy_hq).is_ok());
    assert!(w.order(1, id, enemy_hq).is_err(), "not your troop");
}

#[test]
fn occupation_takes_places() {
    let g = tiny("");
    let mut w = World::new(&g, 4).unwrap();
    let target = w.factions[2].hq.unwrap();
    assert_eq!(w.owner[target], Some(2));
    for _ in 0..3 {
        w.raise(0, 0, target);
    }
    for _ in 0..12 {
        w.step();
    }
    assert_eq!(w.owner[target], Some(0), "three full troops take the place");
}

#[test]
fn terrain_decides_where_troops_go() {
    // Horses cannot enter high ground; foot pays double there.
    let g = tiny(
        "[[terrain]]\nid = \"high\"\nfield = \"elevation\"\nabove = 0.55\ndefense = 2\nmove_cost = { foot = 2 }\n\
         [[terrain]]\nid = \"low\"\nmove_cost = { foot = 1, horse = 1 }\n",
    );
    let mut w = World::new(&g, 5).unwrap();
    let high: Vec<usize> = (0..w.city.len())
        .filter(|&p| w.terrain[p] == Some(0))
        .collect();
    let low: Vec<usize> = (0..w.city.len())
        .filter(|&p| w.terrain[p] == Some(1))
        .collect();
    assert!(
        !high.is_empty() && !low.is_empty(),
        "both terrains on the map"
    );
    let start = low[0];
    let horse = w.raise(0, 1, start);
    let foot = w.raise(0, 0, start);
    for &p in &high {
        assert!(w.route(horse, p).is_none(), "horses stay off high ground");
        let r = w.route(foot, p).unwrap();
        assert!(r.cost >= 2.0, "entering high ground costs 2");
    }
}

#[test]
fn raising_costs_what_the_troop_costs() {
    let g = tiny("");
    let d = &g.script.decisions[0];
    assert_eq!(d.costs, vec![(g.rules.resource("Gold").unwrap(), 30.0)]);
    let mut w = World::new(&g, 6).unwrap();
    w.player = Some(0);
    let hq = w.factions[0].hq.unwrap();
    let opts = w.options(0, Some(hq));
    assert!(opts[0].affordable);
    w.take(0, 0, Some(hq)).unwrap();
    assert_eq!(w.troops.len(), 1);
    assert_eq!(w.gb.accounts[w.factions[0].account][0], 70.0);
}

#[test]
fn includes_merge_and_refuse_duplicates() {
    let files = |inc: &str| {
        BTreeMap::from([
            (
                "game.toml".to_string(),
                "name = \"I\"\nscripts = []\ninclude = [\"more.toml\"]\n[city]\nneighborhoods = 20\n[[faction]]\nid = \"r\"\nkind = \"realm\"\n[control]\nhold = 1\nmargin = 1\ndecay = 0\ncontest = 1\nattrition = 0\nviolence = 0\n[[troop]]\nid = \"a\"\nspeed = 1\n".to_string(),
            ),
            ("more.toml".to_string(), inc.to_string()),
        ])
    };
    let g = Game::from_files(
        files("[[troop]]\nid = \"b\"\nspeed = 2\n[war]\nluck = 3\n"),
        &Params::default(),
    )
    .unwrap();
    assert_eq!(g.war.troops.len(), 2);
    assert_eq!(g.war.luck, 3.0);
    let e = Game::from_files(files("name = \"other\"\n"), &Params::default()).unwrap_err();
    assert!(e.contains("already set"), "{e}");
}

fn marches(era: &str) -> Game {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/marches");
    let mut p = Params::default();
    p.0.insert("era".into(), era.into());
    p.0.insert("provinces".into(), "120".into());
    Game::load(&dir, &p).unwrap()
}

#[test]
fn marches_runs_in_both_eras_and_is_deterministic() {
    for era in ["medieval", "future"] {
        let g = marches(era);
        assert_eq!(g.war.troops.len(), 4);
        let run = |seed| {
            let mut w = World::new(&g, seed).unwrap();
            w.run(300);
            (w.summary(), w.troops.clone())
        };
        let (a, ta) = run(9);
        let (b, tb) = run(9);
        assert_eq!(a, b, "{era}: same seed, same game");
        assert_eq!(ta, tb);
        assert!(a["raised"] > 15.0, "{era}: realms raise troops");
        assert!(a["battles"] > 0.0, "{era}: and fight");
        assert!(a["destroyed"] > 0.0, "{era}: and lose some");
    }
    assert_eq!(marches("future").war.troops[1].name, "Drone wing");
}
