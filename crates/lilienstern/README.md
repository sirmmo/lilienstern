# Lilienstern

A deeply configurable grand-strategy simulation engine. A game is data: a map of places
(generated, or imported from GeoJSON), factions, and rules written in script files, in two
layers:

* the **economy** in [GlassBox](https://crates.io/crates/glassbox): units in places taking and
  giving resources, agents carrying them along the map, values per place that spread and fade;
* the **politics**: events, decisions the factions' AI weighs and takes, timed modifiers,
  troops that march and fight, technologies, and a ledger that says where every coin came from.

Every number can be a parameter, experiments run a game under several parameter sets and
seeds, and runs are deterministic.

```rust,no_run
use std::path::Path;
use lilienstern::{Game, Params, World};

// A game folder: game.toml, params.toml and the scripts.
let game = Game::load(Path::new("games/marches"), &Params::default()).unwrap();
let mut world = World::new(&game, 1).unwrap(); // seed 1
for _ in 0..365 {
    world.step(); // a day
}
```

The example games (Marches, Italia 1871, Gangland), the scripting reference and the design
notes are in the [repository](https://github.com/sirmmo/lilienstern):
[SCRIPTING.md](https://github.com/sirmmo/lilienstern/blob/main/docs/SCRIPTING.md),
[DESIGN.md](https://github.com/sirmmo/lilienstern/blob/main/docs/DESIGN.md). The games play in
the browser at [ingmmo.com/lilienstern](https://ingmmo.com/lilienstern/).

Licence: GPL-3.0-or-later.
