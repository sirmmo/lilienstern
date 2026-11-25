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
slowed by terrain, share places with allies, and fight enemies where they meet. They research
**tech trees**, also data: techs unlock troops and decisions and make troops stronger, faster
or cheaper.

**[Play Gangland: Bologna](https://ingmmo.com/lilienstern/gangland-bologna/)**: run a gang in
Bologna's 2,606 census sections (or its 90 areas), raise crews and learn the trade, in your browser;
or in **[Milano](https://ingmmo.com/lilienstern/gangland-milano/)** (7,095 census sections, or its 88
NIL) **[Roma](https://ingmmo.com/lilienstern/gangland-roma/)** (4,200 blocks, or its 155 zone
urbanistiche, with the Tiber crossed at its bridges) and **[New York](https://ingmmo.com/lilienstern/gangland-newyork/)**
(2,325 census tracts, or its 262 neighbourhoods, the boroughs joined by their bridges, tunnels and ferry). **[Play Marches](https://ingmmo.com/lilienstern/marches/)**:
rule a realm, raise troops and march them to war, medieval or futuristic.
**[Play Italia](https://ingmmo.com/lilienstern/italia/)**: lead a noble family (Savoia, Visconti,
Medici, Este, Aragona...) across ISTAT's Italy of 1871, on real mountains and plains, with
medieval troops. All of them can be played by several people on one map: taking turns on one device, or
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
./target/release/lilienstern run games/gangland --set city_file=maps/roma-areas.geojson --map out/roma
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
* **Parameters** (`games/gangland/params.toml`): about 60 numbers, from the number of gangs to
  what police patrols go after.
* **Crews and techs** (`games/gangland/crews.toml`, `crews.lil`), switched on with
  `--set crews=crews.toml` (the browser game does): gangs raise street soldiers, shooters and
  enforcers, and once they know how, scooter riders and hitmen, and send them across the city;
  crews fight rival crews and take blocks over. Contacts from the street, corners and rackets go
  into nine techs (burner phones, kevlar, an armory, informants, money laundering...). Without
  them Gangland is the game of the experiments, where gangs move by presence alone.

Eight experiments come with it (`experiments/`), three on real cities (Bologna, Milano, Roma); see
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
  Armies have no cap but their cost: upkeep is paid every morning, and a realm that keeps more
  troops than its land can feed empties its treasury.
* **The people** (`games/marches/people.lil`): each province's happiness, a map. Taxes and
  levies follow it; battles, quartered troops, levies, war taxes and unpaid troops lower it;
  below 15 a province revolts. Rulers can feed the people, levy a war tax or disband troops.
* **A ledger** books every gain and expense under where it came from (each tax, levy,
  decision, the upkeep), which the browser's treasury shows and scripts read as `income`,
  `upkeep` and `balance`.
* `--set teams=a,a` starts the first two realms allied; `--set era=future` changes the troops
  and nothing else.

See [the experiment](docs/EXPERIMENTS.md#marches-experiment) on alliances and eras.

## Italia

Marches on a real map: Italy as ISTAT drew it for the 1871 census (284 circondari and
distretti, or 69 provinces), largely the old provinces of the Italian states, taken as the
lands fought over in the centuries before. Up to 16 noble families start at their historical
seats, under their coats of arms (from Wikimedia Commons, [credits](web/italia/crests/README.md));
each area's taxes, recruits and learning follow its 1871 population (and its old university, if
it has one), its terrain its real elevation (SRTM), and Sicily and Sardinia are reached by sea.
The map names the provinces' capitals, with their 1871 names (Girgenti, Porto Maurizio). The decisions and troops are
Marches' (`games/italia` includes them); `tools/italia.py` builds the maps
([sources](games/italia/maps/README.md)).

```sh
./target/release/lilienstern run games/italia --seed 1 --map out/italia
./target/release/lilienstern run games/italia --set map=province --set families=16
```

## Playing together

Gangland, Marches and Italia take several people on the same map:

* **On one device**: choose every gang, realm or family a person plays; each takes their turn (decisions and
  orders), and the day runs when the last one ends it.
* **Online**: one player opens a room and gets a five-letter code (or a `?join=CODE` link);
  friends join, pick a family each, and the host begins. Rooms live on Lilienstern's game
  server (`crates/lilienstern-server`, at `wss://lilienstern.fantasymaps.org/ws`), so nobody's
  browser has to stay open for the game to go on.

The host also sets the **pace** when it creates the game: a day runs when everyone has ended
it; or when everyone has, or after a time limit (30 seconds to 2 minutes), whichever comes
first; or the game **runs**, a day every 5 to 30 seconds whatever the players do (sooner if
everyone has ended it). The server's clock decides; the host can pause it, and everyone sees
the countdown.

Online play is **lockstep**. Every browser runs the same engine on the same files, parameters and
seed; every change to the game is a command (a decision, an order, research, the end of a day),
and the server puts the commands in one order, keeps them and sends them to everyone: the log of
commands is the game. The server does not run the game itself, only the room: who plays what,
the log, the clock. A day runs when every person has ended it; a player whose connection
dropped does not hold it up, gets the commands they missed when they are back (the page
reconnects by itself), and the host can hand their family to the computer. Someone who closed
their tab can open the room again and take their family back ("Take over"; the host's seat
makes them the host). Someone can also join a game under way and take over a family the
computer was playing. After each day every browser sends the server a fingerprint of the whole
game; if two differ, everyone replays the log.

This works because the engine is deterministic and WebAssembly's arithmetic is the same in
every browser. Rooms are saved on the server as they change, so games survive a restart; a
room nobody touches for two weeks is deleted.

**Running the server**: `cargo run --release -p lilienstern-server` (it listens on
`LIL_ADDR`, default `0.0.0.0:8080`, keeps rooms in `LIL_DATA`, default `data`, and accepts pages
from `LIL_ORIGINS`). A page uses another server with `?server=ws://localhost:8080/ws`.
`deploy/server/` has a Dockerfile and a Docker Swarm stack behind Traefik, deployed by
`tools/deploy-server.sh`. One server holds thousands of games: a room is a few kilobytes of
commands, and the server does no simulation.

**Stats**: the server's own address shows how it is doing, for the public one
[lilienstern.fantasymaps.org](https://lilienstern.fantasymaps.org/): games stored and under way,
players connected, processor and memory, with the last hour in charts. `/stats.json` has the
same numbers, `/health` answers `ok`. Only totals are shown, never room codes.

**Releasing**: `glassbox` and `lilienstern` are on crates.io. To publish a new version, raise it
in the root `Cargo.toml` (the workspace version and the `version` of the two path
dependencies), commit, and push a tag such as `v0.2.0`: the Publish workflow tests and
publishes both crates, with no stored token (crates.io trusted publishing).

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
| `web/gangland-bologna`, `web/gangland-milano`, `web/gangland-roma`, `web/gangland-newyork`, `web/marches`, `web/italia` | The browser games: one page (`web/marches/app.js`), set up by each game's start screen (`window.GAME`: its files, words, resources, views and map). `tools/site.py` builds the site (docs and games). |

## License

GPL-3.0-or-later, except the icons in `web/marches/icons`, which come from commercial icon
packs and are licensed separately (see [their README](web/marches/icons/README.md)).

## Publishing on Wavedash

Each game is its own game on [Wavedash](https://wavedash.com) ([docs](https://docs.wavedash.com)),
configured in `wavedash/<game>/wavedash.toml`: `gangland-bologna`, `gangland-milano`, `gangland-roma`,
`gangland-newyork`, `marches` and `italia`.

1. **Create the game** in the Wavedash Developer Portal, or with the CLI (`curl -fsSL
   https://wavedash.com/cli/install.sh | sh`, `wavedash auth login`, then `wavedash project create
   --title "Gangland: Bologna" --team-id <TEAM_ID>`).
2. **Set its id**: `game_id` in its `wavedash.toml`, or, to keep it out of the repository,
   `WAVEDASH_GAME_ID_<GAME>` in the environment and as a repository variable (Settings › Secrets and
   variables › Actions › Variables): `WAVEDASH_GAME_ID_GANGLAND_BOLOGNA`,
   `WAVEDASH_GAME_ID_GANGLAND_MILANO`, `WAVEDASH_GAME_ID_GANGLAND_ROMA`,
   `WAVEDASH_GAME_ID_GANGLAND_NEWYORK`, `WAVEDASH_GAME_ID_MARCHES`, `WAVEDASH_GAME_ID_ITALIA`. Gangland: Milano and Roma have `game_id = "SET-ME"` until then;
   pushing all, a game without an id is skipped.
3. **Upload a build**: `sh tools/wavedash.sh [game]` builds the engine and the site as the Pages
   workflow does, copies the game's folder into `wavedash/<game>/build/` (links to the documentation
   point to the website) and, with the CLI on `PATH` and `WAVEDASH_TOKEN` set, runs `wavedash build
   push`. Without the CLI (or with `WAVEDASH_DRY_RUN=1`) it only says what it would push. On GitHub:
   Actions › Wavedash › Run workflow, with the secret `WAVEDASH_TOKEN`.
4. **Publish it**: `wavedash publish <BUILD_ID>` (the id `build push` printed), or in the Developer
   Portal.

On Wavedash the game stays behind Wavedash's loading screen until it calls `Wavedash.init()`:
`web/marches/app.js` does it through `web/marches/wavedash.js` once the engine has started and the
start screen is ready, or says why it is not. Elsewhere (GitHub Pages, a local server) there is no
`Wavedash` object and the calls do nothing.

**Playing together from Wavedash**: Wavedash serves a game from a subdomain of
`builds.wavedashcdn.com` (its docs do not say whether one per game or per build), and the server
only accepts the pages in `LIL_ORIGINS`, matched exactly (with any port). Until that origin
(`https://<subdomain>.builds.wavedashcdn.com`, seen in the browser once the game is up) is in
`LIL_ORIGINS` (`deploy/server/stack.yml`), online games on Wavedash are refused; playing alone and
on one screen work.
