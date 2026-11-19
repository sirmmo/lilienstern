//! The economy: the ledger of where every resource came from, armies with no cap but their
//! cost, and the people's happiness (Marches' people.lil).

use std::path::Path;

use lilienstern::script::Econ;
use lilienstern::{Game, Params, World};

fn marches(set: &[(&str, &str)]) -> Game {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/marches");
    let mut p = Params::default();
    p.0.insert("provinces".into(), "120".into());
    for (k, v) in set {
        p.0.insert((*k).into(), (*v).into());
    }
    Game::load(&dir, &p).unwrap()
}

/// The sum of a faction's ledger for resource `r` on the last day.
fn booked(w: &World, f: usize, r: usize) -> f64 {
    let ns = w.source_names().len();
    w.yesterday.amount[f][r * ns..(r + 1) * ns].iter().sum()
}

#[test]
fn the_ledger_explains_every_change() {
    let g = marches(&[]);
    let mut w = World::new(&g, 4).unwrap();
    w.players = vec![0];
    let funds = g.rules.resource("Funds").unwrap();
    let names = w.source_names();
    let ns = names.len();
    let taxes = names.iter().position(|n| n.1 == "Taxes").unwrap();
    let raise = g
        .script
        .decisions
        .iter()
        .position(|d| d.name == "raise_light")
        .unwrap();
    for day in 0..60 {
        let before: Vec<Vec<f64>> = (0..w.factions.len())
            .map(|f| w.gb.accounts[w.factions[f].account].clone())
            .collect();
        if day == 10 {
            // A player's decision counts in the day it was taken.
            let hq = w.factions[0].hq.unwrap();
            w.take(0, raise, Some(hq)).unwrap();
        }
        w.step();
        for (f, was) in before.iter().enumerate() {
            for (r, &had) in was.iter().enumerate() {
                let change = w.gb.accounts[w.factions[f].account][r] - had;
                assert!(
                    (booked(&w, f, r) - change).abs() < 1e-6,
                    "day {day}, faction {f}, {}: booked {} but changed {change}",
                    g.rules.resources[r].id,
                    booked(&w, f, r)
                );
            }
        }
        if day == 10 {
            let src = ns - g.script.decisions.len() + raise;
            assert_eq!(
                w.yesterday.amount[0][funds * ns + src],
                -15.0,
                "the levy's cost"
            );
            assert_eq!(w.yesterday.count[0][funds * ns + src], 1);
        }
    }
    // Taxes come from the provinces held, and the places' yields add up to them.
    let taxed = w.yesterday.amount[0][funds * ns + taxes];
    assert!(taxed > 0.0);
    assert_eq!(
        w.yesterday.count[0][funds * ns + taxes] as usize,
        w.factions[0].territory
    );
    let from_places: f64 = (0..w.city.len())
        .filter(|&p| w.owner[p] == Some(0))
        .map(|p| w.yields[funds][p])
        .sum();
    let events: f64 = names
        .iter()
        .enumerate()
        .filter(|(_, n)| n.0 == "event")
        .map(|(s, _)| w.yesterday.amount[0][funds * ns + s])
        .sum();
    assert!((from_places - events).abs() < 1e-6);
    assert!((w.economy(Econ::Income, 0, funds) - events).abs() < 1e-9);
}

#[test]
fn armies_have_no_cap_but_their_cost() {
    // Rich, with many decisions a day: a person raises as many troops as they can pay for.
    let g = marches(&[
        ("start_funds", "3000"),
        ("start_recruits", "1000"),
        ("realm_actions", "40"),
    ]);
    let mut w = World::new(&g, 2).unwrap();
    w.players = vec![0];
    let funds = g.rules.resource("Funds").unwrap();
    let happy = g.rules.map("Happiness").unwrap();
    let hq = w.factions[0].hq.unwrap();
    let raise = g
        .script
        .decisions
        .iter()
        .position(|d| d.name == "raise_heavy")
        .unwrap();
    for _ in 0..40 {
        w.take(0, raise, Some(hq)).unwrap();
    }
    let army = w.troops.iter().filter(|t| t.faction == 0).count();
    assert!(army >= 40, "no cap: {army} troops");
    assert!(w.gb.maps[happy][hq] < 10.0, "a heavy levy angers the place");
    let upkeep = w.economy(Econ::Upkeep, 0, funds);
    let expected: f64 = w
        .troops
        .iter()
        .filter(|t| t.faction == 0)
        .flat_map(|t| &g.war.troops[t.kind].upkeep)
        .filter(|u| u.0 == funds)
        .map(|u| u.1)
        .sum();
    assert!(
        (upkeep - expected).abs() < 1e-6 && upkeep > 0.4 * 40.0,
        "upkeep {upkeep}"
    );
    // Spend the rest: the treasury cannot pay the troops for long.
    let acct = w.factions[0].account;
    w.gb.accounts[acct][funds] = 5.0;
    w.step();
    assert!(w.unpaid[0], "5 funds do not pay {upkeep}");
    assert_eq!(
        w.gb.accounts[acct][funds], 0.0,
        "what there was went to the troops"
    );
    let ns = w.source_names().len();
    let paid = w.yesterday.amount[0][funds * ns + lilienstern::world::SRC_UPKEEP];
    assert!(paid < 0.0 && paid > -upkeep, "paid {paid} of {upkeep}");
    let mood = |w: &World| {
        let mine: Vec<usize> = (0..w.city.len())
            .filter(|&p| w.owner[p] == Some(0))
            .collect();
        mine.iter().map(|&p| w.gb.maps[happy][p]).sum::<f64>() / mine.len() as f64
    };
    let before = mood(&w);
    let hp: f64 = w
        .troops
        .iter()
        .filter(|t| t.faction == 0)
        .map(|t| t.hp)
        .sum();
    for _ in 0..10 {
        w.step();
    }
    assert!(
        mood(&w) < before - 5.0,
        "unpaid troops plunder: {before} -> {}",
        mood(&w)
    );
    let hp_after: f64 = w
        .troops
        .iter()
        .filter(|t| t.faction == 0)
        .map(|t| t.hp)
        .sum();
    assert!(hp_after < hp, "and lose health");
    // Disbanding lowers the upkeep.
    let disband = g
        .script
        .decisions
        .iter()
        .position(|d| d.name == "disband")
        .unwrap();
    let at = w
        .troops
        .iter()
        .find(|t| t.faction == 0 && t.path.is_empty())
        .unwrap()
        .place;
    let n = w.troops.len();
    let due = w.due[0][funds];
    w.take(0, disband, Some(at)).unwrap();
    assert_eq!(w.troops.len(), n - 1);
    assert!(w.due[0][funds] < due);
}

#[test]
fn unhappy_provinces_pay_less() {
    let g = marches(&[]);
    let mut w = World::new(&g, 3).unwrap();
    let funds = g.rules.resource("Funds").unwrap();
    let happy = g.rules.map("Happiness").unwrap();
    w.step();
    let wealth = g.rules.map("Wealth").unwrap();
    let rich = |w: &World, p: usize| w.gb.maps[wealth][p] > 50.0;
    let mine: Vec<usize> = (0..w.city.len())
        .filter(|&p| w.owner[p] == Some(0))
        .collect();
    let a = mine[0];
    let b = *mine[1..]
        .iter()
        .find(|&&p| rich(&w, p) == rich(&w, a))
        .unwrap();
    w.gb.maps[happy][a] = 100.0;
    w.gb.maps[happy][b] = 10.0;
    w.step();
    // Taxes come before the day's change of mood (realms.lil before people.lil).
    let ratio = w.yields[funds][a] / w.yields[funds][b];
    assert!(
        (ratio - 10.0).abs() < 1e-9,
        "taxes follow happiness: {ratio}"
    );
}

#[test]
fn the_ai_keeps_its_armies_within_its_means() {
    let g = marches(&[]);
    let mut w = World::new(&g, 5).unwrap();
    w.run(400);
    let s = w.summary();
    assert!(s["raised"] > 20.0);
    assert!(
        s["unpaid"] < 20.0,
        "the AI rarely leaves its troops unpaid: {}",
        s["unpaid"]
    );
}
