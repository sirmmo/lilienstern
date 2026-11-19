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
rule a realm, raise troops and march them to war, medieval or futuristic.
**[Play Italia](https://ingmmo.com/lilienstern/italia/)**: lead a noble family (Savoia, Visconti,
Medici, Este, Aragona...) across ISTAT's Italy of 1871, on real mountains and plains.
Marches and Italia can be played by several people on one map: taking turns on one device, or
online, browser to browser, with a room code. Documentation:
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

## Italia

Marches on a real map: Italy as ISTAT drew it for the 1871 census (284 circondari and
distretti, or 69 provinces), largely the old provinces of the Italian states, taken as the
lands fought over in the centuries before. Up to 16 noble families start at their historical
seats; each area's taxes and recruits follow its 1871 population, its terrain its real
elevation (SRTM), and Sicily and Sardinia are reached by sea. The decisions and troops are
Marches' (`games/italia` includes them); `tools/italia.py` builds the maps
([sources](games/italia/maps/README.md)).

```sh
./target/release/lilienstern run games/italia --seed 1 --map out/italia
./target/release/lilienstern run games/italia --set map=province --set families=16
```

## Playing together

Marches and Italia take several people on the same map:

* **On one device**: choose every family a person plays; each takes their turn (decisions and
  orders), and the day runs when the last one ends it.
* **Online**: one player opens a room and gets a five-letter code (or a `?join=CODE` link);
  friends join, pick a family each, and the host begins. Nothing goes through a game server:
  [PeerJS](https://peerjs.com) (MIT, vendored in `web/marches/vendor`) only introduces the
  browsers, then they talk directly over WebRTC.

The host also sets the **pace** when it creates the game: a day runs when everyone has ended
it; or when everyone has, or after a time limit (30 seconds to 2 minutes), whichever comes
first; or the game **runs**, a day every 5 to 30 seconds whatever the players do (sooner if
everyone has ended it). The host's clock decides and can pause the game; everyone sees the
countdown.

Online play is **lockstep**. Every browser runs the same engine on the same files, parameters and
seed; every change to the game is a command (a decision, an order, the end of a day), and the
host puts the commands in one order and sends them to everyone. A day runs when every person has
ended it (a player whose connection dropped does not hold it up, and the host can hand their
family to the computer). After each day every browser compares a fingerprint of the whole game
with the host's; a browser that differs, or comes back after a dropped connection, replays the
commands from the start, which takes a moment even late in a game. Someone can also join a game
under way and take over a family the computer was playing.

This works because the engine is deterministic and WebAssembly's arithmetic is the same in
every browser. The host is the referee and the room lives in its tab: if the host leaves, the
game stops.

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
| `web/gangland-bologna`, `web/marches`, `web/italia` | The browser games (Italia is the Marches page with its own start screen). `tools/site.py` builds the site (docs and games). |

## License

GPL-3.0-or-later, except the icons in `web/marches/icons`, which come from commercial icon
packs and are licensed separately (see [their README](web/marches/icons/README.md)).
