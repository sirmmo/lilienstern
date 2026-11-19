//! Several people in one game: each plays a faction with its own decisions a day, and copies of
//! a game given the same commands stay identical (what lockstep play in several browsers needs).

use std::path::Path;

use lilienstern::{Game, Params, World};

fn marches() -> Game {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/marches");
    let mut p = Params::default();
    p.0.insert("provinces".into(), "120".into());
    Game::load(&dir, &p).unwrap()
}

/// Plays the same commands for two players: each raises a troop at home and sends its troops
/// to a neighbouring place, every few days, for 60 days.
fn play(g: &Game, extra: bool) -> (u64, World<'_>) {
    let mut w = World::new(g, 11).unwrap();
    w.players = vec![0, 2];
    for day in 0..60 {
        for &f in &[0usize, 2] {
            if day % 10 == 0 {
                if let Some(hq) = w.factions[f].hq.filter(|&p| w.owner[p] == Some(f)) {
                    let raise = w
                        .options(f, Some(hq))
                        .into_iter()
                        .find(|o| o.name == "raise_light" && o.affordable);
                    if let Some(o) = raise {
                        w.take(f, o.decision, Some(hq)).unwrap();
                    }
                    let to = w.city.neighbours[hq][0];
                    let ids: Vec<u32> = w
                        .troops
                        .iter()
                        .filter(|t| t.faction == f)
                        .map(|t| t.id)
                        .collect();
                    for id in ids {
                        let _ = w.order(f, id, to);
                    }
                }
            }
        }
        if extra && day == 30 {
            let id = w.troops.iter().find(|t| t.faction == 0).map(|t| t.id);
            if let Some(id) = id {
                let _ = w.order(0, id, w.factions[0].hq.unwrap());
            }
        }
        w.step();
    }
    (w.fingerprint(), w)
}

#[test]
fn copies_with_the_same_commands_stay_in_step() {
    let g = marches();
    let (a, wa) = play(&g, false);
    let (b, wb) = play(&g, false);
    assert_eq!(a, b);
    assert_eq!(wa.summary(), wb.summary());
    let (c, _) = play(&g, true);
    assert_ne!(a, c, "one more order, another game");
}

#[test]
fn each_player_has_its_own_decisions_and_the_ai_leaves_players_alone() {
    let g = marches();
    let mut w = World::new(&g, 4).unwrap();
    w.players = vec![0, 1];
    let actions = g.factions[0].actions;
    for &f in &[0usize, 1] {
        let hq = w.factions[f].hq.unwrap();
        for _ in 0..actions {
            let o = w
                .options(f, Some(hq))
                .into_iter()
                .find(|o| o.affordable && (o.name == "govern" || o.name == "raise_light"))
                .unwrap();
            w.take(f, o.decision, Some(hq)).unwrap();
        }
        let o = w.options(f, Some(hq)).into_iter().next().unwrap();
        assert!(w
            .take(f, o.decision, Some(hq))
            .unwrap_err()
            .contains("no actions left"));
    }
    let before: Vec<usize> = w
        .troops
        .iter()
        .filter(|t| t.faction < 2)
        .map(|t| t.id as usize)
        .collect();
    w.step();
    // Nobody gave the players' troops orders, so the AI did not either.
    assert!(
        w.troops
            .iter()
            .filter(|t| t.faction < 2)
            .all(|t| t.path.is_empty()),
        "{before:?}"
    );
    assert_eq!(
        w.player_actions,
        vec![0; g.factions.len()],
        "a new day, new decisions"
    );
}
