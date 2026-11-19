# Lilienstern

An open-source, deeply configurable grand-strategy simulation engine in Rust, in the spirit of
the engines behind map-based strategy games: a map of places, factions with an AI, a daily
clock, and every rule in data files. Its economy is written in **GlassBox**, the rule language
of SimCity (2013) with the Extended GlassBox additions for urban digital twins; its politics
(events, decisions, modifiers) in Lilienstern's own blocks, in the same files.

It is built for experiments as much as for games: every number is a parameter, runs are
deterministic, and an experiment plays a game under several parameter sets and seeds in
parallel and writes a report. Games run on generated maps or on real ones (GeoJSON or a
MapLibre style), and in the browser.

Factions can raise **troops**, declared in the game's data like everything else (levies and
knights, or militia and hovertanks): they march across the map on the shared daily clock,
slowed by terrain, share places with allies, and fight enemies where they meet.

**[Play Gangland: Bologna](https://ingmmo.com/lilienstern/gangland-bologna/)**: run a gang in
Bologna's 2,606 census sections, in your browser. **[Play Marches](https://ingmmo.com/lilienstern/marches/)**:
rule a realm, raise troops and march them to war, medieval or futuristic. Documentation:
**[ingmmo.com/lilienstern](https://ingmmo.com/lilienstern/)**.

Named after Otto August Rühle von Lilienstern (1780–1847), Prussian officer, military writer
and contemporary of Clausewitz.

## Quick start

```sh
cargo build --release
./target/release/lilienstern check games/gangland
./target/release/lilienstern run games/gangland --seed 1 --geojson out/gangland.geojson
./target/release/lilienstern experiment experiments/policing.toml
./target/release/lilienstern run games/marches --seed 4 --map out/marches        # troops
./target/release/lilienstern run games/marches --set era=future --set teams=a,a  # allies
```

`run` prints the course of the game every tenth of its length and a summary; `--csv` writes
the time series and `--geojson` the final map (open it in QGIS or geojson.io: neighbourhoods
are coloured by gang). `--map DIR` writes a viewer: open `DIR/index.html` to replay the game
over a basemap, or use `DIR/style.json` (a MapLibre style) elsewhere. `--set name=value`
changes any parameter. `experiment` writes `out/<experiment>/report.md`, `summary.csv`, one
series per run and a viewer per variant.

On a real city:

```sh
./target/release/lilienstern run games/gangland --set city_file=maps/bologna-sections.geojson --map out/bologna
```

`city_file` takes any GeoJSON of polygons (neighbours are polygons sharing a border) or a
MapLibre style with a GeoJSON source, which then also becomes the basemap. See
[the maps](games/gangland/maps/README.md).

## Gangland

The example game: gangs fight over a large city cut into small neighbourhoods (about 900 by
default, 10 km across, with a river and five bridges).

![Gangland after two years: six gangs' territories, and where the violence is](docs/gangland.png)

* **Economy** (`games/gangland/economy.gbx`, GlassBox): safehouses recruit crew in poor
  neighbourhoods; labs make product that agents carry along the streets to the gang's corners,
  which sell it; rackets extort rich neighbourhoods. All of it heats up the neighbourhood.
* **Politics** (`games/gangland/politics.lil`): gangs expand, reinforce, attack, regroup, open
  corners, rackets, labs and safehouses, hire muscle and bribe; the police patrols and raids.
  Where two gangs meet they clash, and violence drives wealth away.
* **Parameters** (`games/gangland/params.toml`): about 50 numbers, from the number of gangs to
  what police patrols go after.

Six experiments come with it (`experiments/`), one on Bologna; see
[docs/EXPERIMENTS.md](docs/EXPERIMENTS.md) for what they ask and what they found.

## Marches

The example game with troops: five realms on a generated land of about 240 provinces, with
plains, forests, hills, mountains and a river crossed only at its bridges.

* **Troops and terrain** come from an era file (`games/marches/eras/medieval.toml` or
  `future.toml`): four kinds of troops each, with speed, movement type, cost, upkeep, a damage
  table and how fast they occupy; terrain with defence and movement costs.
* **Realms** (`games/marches/realms.lil`) tax their provinces, settle empty land, govern,
  fortify, raise troops, send them to attack, take and defend, and make war, peace and
  alliances. Troops at war fight where they meet; troops alone in a province occupy it.
* `--set teams=a,a` starts the first two realms allied; `--set era=future` changes the troops
  and nothing else.

See [the experiment](docs/EXPERIMENTS.md#marches-experiment) on alliances and eras.

## Documentation

* [docs/DESIGN.md](docs/DESIGN.md): what the engine is, how a day runs, how GlassBox fits, what
  comes next.
* [docs/SCRIPTING.md](docs/SCRIPTING.md): writing a game: `game.toml`, parameters, GlassBox
  rules, events, decisions, modifiers, the AI, troops and terrain.
* [docs/EXPERIMENTS.md](docs/EXPERIMENTS.md): writing experiments, and Gangland's and Marches'
  results.

## Layout

| Crate | What it is |
|---|---|
| `crates/glassbox` | The GlassBox rule language and resource-flow engine on a graph of places. No dependencies; usable on its own. |
| `crates/lilienstern` | The engine: maps (generated or GeoJSON), games, factions, scripts, the AI, control, troops, metrics, experiments. |
| `crates/lilienstern-cli` | The `lilienstern` command. |
| `crates/lilienstern-web` | The engine as WebAssembly, driven by JSON messages, for the browser. |
| `web/gangland-bologna`, `web/marches` | The browser games. `tools/site.py` builds the site (docs and games). |

## License

GPL-3.0-or-later, except the icons in `web/marches/icons`, which come from commercial icon
packs and are licensed separately (see [their README](web/marches/icons/README.md)).
