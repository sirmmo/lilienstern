# Lilienstern design

## Aims

1. **A grand-strategy engine, open.** A map of places, factions with goals and an AI, a daily
   clock, and rules entirely in data, as in the engines behind map-based strategy games. It
   takes the architecture of the genre, not any game's files, assets or script format.
2. **Deeply configurable.** Every rule is data, every number in a game can be a parameter, and
   the engine has no built-in notion of what the factions are: Gangland's gangs and police are
   declared by the game, like provinces and countries would be by another.
3. **GlassBox as the economy.** The same rule language as citysim's (SimCity's GlassBox,
   extended for urban digital twins), so rules and knowledge move between the two engines.
4. **Experiments.** Deterministic runs and a runner for parameter sweeps over many seeds, so
   a game is also a model whose behaviour can be measured and compared with data.

## Concepts

| Concept | What it is | In Gangland |
|---|---|---|
| **Place** | A node of the map, with neighbours and an outline | a neighbourhood |
| **Map** | A value per place; decays and spreads to neighbours | Wealth, Heat, Police, Violence |
| **Resource** | `local` (held by a unit), `global` (held by an account), `agent` (carried along the map) | Crew, Cash, Product |
| **Unit** | Something in a place that runs GlassBox rules | safehouse, corner, racket, lab |
| **Faction** | An actor with an account, an AI and a kind (`gang`, `realm`, `police`, `other`) | 6 gangs, the police |
| **Relation** | War, peace or alliance between two factions | (Marches) realms make war and peace |
| **Troop** | Something a faction owns that stands in a place, marches and fights; its kinds are data | (Marches) levies, knights; or militia, hovertanks |
| **Terrain** | What a place is to troops: defence, and what it costs to enter by each movement type | (Marches) plains, forest, hills, mountains |
| **Presence** | How strongly a faction is in a place; sets who holds it | gang strength in a block |
| **Event** | Something that happens to a place, a faction or the world when its conditions hold | an economic shock, arrests |
| **Decision** | Something a faction can do to a target; the AI weighs and takes them | expand, attack, patrol, raid |
| **Modifier** | A timed state of a place | a lockdown after a raid |
| **Metric** | A number counted every day (`log`) | Sales, Raids, HotDaysEast |

## A day

1. **Modifiers** set each place's output factor and apply their daily effects.
2. **GlassBox**: units run their rules (taking and giving resources, emitting into maps),
   agents carry agent resources to the nearest units that need them, map rules run on every
   place, maps decay and diffuse.
3. Factions get their **income**.
4. **Events** fire where their conditions hold.
5. Factions **pay their troops' upkeep** (as much as they have); then each faction's **AI**
   takes up to `actions` decisions (which may raise troops and give them orders).
6. **Troops** march, fight where enemies meet, occupy places, lose health if they went unpaid,
   and heal.
7. **Control**: gangs with presence above `contest` in the same place clash (each loses
   `attrition` times the others' presence, the place gets violence); presence decays, faster
   where the police is; places change hands when a gang has `hold` presence and `margin` times
   the next one's; a gang that loses a place loses its units there (seized or destroyed).
8. **Metrics** are recorded, and the day's **ledger** is closed.

Control is the one built-in mechanic tied to a genre (territory held by presence). Its numbers
are in `[control]`, and a game that does not use presence simply has no gangs.

## The AI

For each decision a faction may take (right kind, due today), the AI finds the best target:
every candidate target (`none`, `owned`, `adjacent`, `present`, `contested`, `any`) that meets
the conditions is scored by the decision's weights, with some noise. Then it picks a decision,
by default **at random in proportion to the scores** (`choice = "weighted"`), or the best
(`"best"`).

Two lessons from tuning Gangland shaped this:

* Always taking the best score makes the factions single-minded: whatever weighs most (hiring,
  expanding) takes every action, and nothing else is ever built. Weighted choice keeps every
  option alive in proportion to how good it is, which is also how the genre's AIs usually
  work.
* A greedy AI never saves: cheap decisions spend every bit of crew before an expensive one is
  affordable. So when the picked decision is unaffordable, the faction **saves**: it only takes
  decisions that do not spend what it is short of, for up to `patience` decisions in a row.

And one lesson for writing weights: a weight on an unbounded quantity (a weight on cash, say)
eventually outweighs everything else. Gate such decisions with conditions instead (`if global
Crew less 12`).

## Troops

Troops come from the tactics genre: unit kinds with a movement type, a speed, a cost and a
damage table against every other kind (in percent of a full unit), terrain with defence stars
and a movement cost per movement type, and defence that weakens with the defender's health.
All of it is data (`[[troop]]`, `[[terrain]]`, `[war]`), so the same engine plays levies and
knights or militia and hovertanks; Marches ships both eras with the same troop ids, so its
scripts work with either.

What changes on a strategy map is time. Turn-based tactics moves one unit at a time; here every
troop moves on the shared daily clock, and two rules keep that fair and deterministic:

* **Simultaneous movement.** Troops advance one border at a time, all together, until each has
  spent its speed for the day; one that walks into enemies stops. Routes are cheapest paths
  (Dijkstra) over the terrain's costs, computed backwards from the destination, so one search
  serves every troop sent there.
* **Simultaneous battle.** In each place, every troop picks the enemy it hurts most and every
  hit is computed from the state before any is dealt. The order troops are stored in changes
  nothing.

Troops hold land through **presence**, the same quantity gangs fight over: a troop alone in a
place adds to its faction's presence and wears down its enemies', and control decides the
holder. So troops and decisions (settling, governing, attacking) are two ways to the same
end, and a game can use either or both. **Relations** decide who fights: allies share places
and never fight or take each other's land, factions at peace stay out of each other's places.

One lesson from tuning Marches: with the default saving rule a faction always ends up buying
its cheapest troop, because a cheap pick in between ends the saving for a dear one. Factions
can now save for a goal (`saving = "goal"`).

## The economy

Every change to a faction's resources is booked in a **ledger**, by where it came from: its
fixed income, each event (taxes, levies), each GlassBox unit rule (Gangland's corners, labs and
rackets, named after their units), each decision, research, troop upkeep, and "other" for the
rest (GlassBox's map and agent rules). GlassBox records what each unit rule added to or took
from each account during its tick (`State::flows`). The day's ledger closes at the end of the day; what no
script booked is found by comparing the accounts with the day before, so the ledger always
adds up to the change. It is bookkeeping only: nothing in the game depends on it except the
`income`, `upkeep` and `balance` that scripts can read (the last finished day's), so adding it
changed no game.

Armies have **no cap**: only their cost. Upkeep is paid every morning, before any decision,
as far as the treasury goes; troops lose health for the share left unpaid. A cap used to keep
the AI from raising troops forever; now the AI's own weights do it (`weight balance Funds`,
`weight army`), and a person may raise as many as they can pay for once, and keep them as long
as the treasury lasts. Two lessons from tuning Marches and Italia:

* With upkeep due after the decisions, the AI spent all its funds every morning and left even
  a rich realm's troops unpaid. Paying first fixed it: unpaid days went from hundreds a game
  to a handful.
* Without the cap, the largest realm snowballed (one family took 95% of Italy). The cap had
  been doing the AI's restraint; `weight army -0.4` on raising (the AI's taste, not a rule)
  brought back the old balance: 2.8 families of 8 left after five years, as with the cap.

**Happiness** (Marches' `people.lil`, shared by Italia) is a map, not engine code: taxes and
levies are multiplied by it (`times map Happiness 0.02`), every day it moves towards 50, and
battles, quartered troops, levies, war taxes, very big realms and, most of all, unpaid troops
push it down; below 15 a province revolts (its holder's presence falls). War alone settles a
province at about 25 (half the taxes, no revolt): when war could push provinces into revolt,
revolts helped attackers (the defender's provinces are where the fighting is) and sped up
conquest.

## Techs

A tech tree is the genre's other long-term choice, and here it is data like the rest: techs
with a cost, prerequisites and effects, and one research resource that scripts produce (so
research can come from provinces, buildings, universities or trade, as a game decides). Each
day a faction's research points go into its current tech. Techs act in two ways, which together
cover what trees usually do:

* **bonuses** the engine applies to troops (damage, defence, speed, occupation, upkeep), per
  troop kind, kept as multipliers per faction and recomputed only when a tech is learnt;
* **conditions** for everything else: a troop that `requires` a tech, and `if tech X` in any
  decision or event (new decisions, more income, a different AI).

In a game without techs every bonus is exactly 1 (or 0 stars), and multiplying by 1 changes no
bit, so games written before techs run exactly as they did.

## GlassBox on a graph

The `glassbox` crate is the GlassBox language and engine with places instead of tiles. Compared
with citysim's version:

* a `radius` is a number of steps along the graph; an emission reaches places within it with
  `amount / (1 + steps)`;
* `global` resources belong to the unit's **account**, which in Lilienstern is its faction's
  treasury (citysim has one city treasury);
* agents search breadth-first along the graph up to the resource's `reach`; `private` resources
  go only to units of the same account (a gang's labs supply its own corners);
* tests are gates (a failed test stops the rule), and a rule with `rate n` fires every *n*
  ticks;
* there are no zones, so `zoneRule` is ignored with a warning.

The crate has no dependencies and knows nothing of factions. The plan is for citysim to use it
too, so both engines run the same rules. That needs the grid as a graph (tiles and their
neighbours) and citysim's averaged rates and soft tests as options of the engine.

## Determinism

The same game, parameters and seed give the same run, bit for bit, on the same platform:

* randomness comes from a seeded SplitMix64, drawn in a fixed order;
* nothing iterates a hash map in an order that matters; places, units, factions and decisions
  are processed by index;
* the simulation uses only `+ - * /` and `sqrt` on `f64`, which IEEE 754 rounds the same way
  everywhere. Trigonometry is used only to write GeoJSON.

That is enough for experiments. Lockstep multiplayer across platforms needs results identical
across compilers and CPUs too; if `f64` turns out not to be (fused multiply-add, x87), the
simulation can move to fixed point behind the same rules.

## Maps

A map is either **generated** (a round city of hexagonal neighbourhoods; fields such as wealth
and density are sampled from continuous fields of the city, so the same city can be cut into
any number of neighbourhoods) or **imported from GeoJSON** (polygons sharing a border are
neighbours; numeric properties become fields maps can start from). The map is exported as
GeoJSON with each place's owner, maps, presence and units.

Real neighbourhoods (from OpenStreetMap, a city's statistical areas, or OHM/OFM for historical
and fictional maps) work through the import: set `geojson = "file.geojson"` in `[city]`. A
MapLibre style works too: its GeoJSON source is the map and the style is drawn under the game.
Gangland ships Bologna's 90 statistical areas and 2,606 census sections, with income and
residents from the city's open data (`tools/bologna.py`); Milan's 88 NIL and 7,095 census
sections (`tools/milano.py`); and Rome's 155 zone urbanistiche and 4,200 blocks of census
sections, with the Tiber crossed at its bridges (`tools/roma.py`). Milan and Rome have their
residents from ISTAT's 2021 census and their income from the IRPEF returns per postcode (see
[the maps](../games/gangland/maps/README.md)).

A run can write a **viewer** (`--map DIR`): `index.html` replays the game over the basemap
from snapshots kept every `sample_every` days, `style.json` is a MapLibre style with the game's
layers, and `map.geojson` the last day.

## Playing

People play factions: `World::players` names them, the AI leaves them alone, and
`World::options(faction, place)` lists what one can do there (with costs, `chance` taken as met)
while `World::take` does it, with the same checks the AI faces and the faction's `actions` a
day (counted per player). Troops take orders with `World::order`. Decisions carry a `label` for
players.

**Several people on one map.** On one device, players take turns and the day runs after the
last. Online, play is lockstep, as in OpenTactics: every browser runs the engine on the same
commands in the order the game server (`lilienstern-server`) gives them (decisions, orders,
research, a player taking over or leaving a faction, the end of a day), and
`World::fingerprint` (FNV-1a over the exact bits of the state, and the random generator's
position) shows after each day that they still agree. A browser that disagrees replays the
log; one that comes back after a dropped connection gets the commands it missed. WebAssembly's
arithmetic is IEEE and identical in every browser, so the same commands give the same bits;
nothing needs sending but commands.

The server keeps rooms, not games: the seats, the log and the clock. It does not run the
engine, on purpose: a refused command is refused by every browser alike and changes nothing,
so the log can hold it; and a server with its own engine would have to be redeployed with every
change to the rules or the engine, or every online game would fall out of step. (A first
version had the host's browser as the room, over PeerJS and WebRTC: games died with the host's
tab, and browsers behind some routers could not reach each other.)

The browser games (`web/gangland-bologna`, `-milano`, `-roma`, and Marches and Italia) run the
engine as WebAssembly. `lilienstern-web`
exposes three functions with no generated glue: the page writes a JSON request into the
module's memory, calls `lil_call`, and reads the JSON answer (`new`, `state`, `place`,
`options`, `take`, `step`, `summary`). Games load from files in memory (`Game::from_files`), so
the page fetches the game's TOML, scripts and map and hands them over. The module is about
1 MB; on 2,606 places a day takes a few milliseconds natively, and a month about a second
in the browser.

## Performance

Gangland, 900 neighbourhoods, 6 gangs, 730 days: about 1.4 s on one core. Marches, about 230
provinces, 5 realms and some 40 troops at a time, 1095 days: about 0.7 s. Experiments run their
runs on all cores: the 64 runs of the Marches experiment take about 2 s on 40 cores.

## Next

* **Saves** with stable ids (units, maps, resources by id, not position), so saves survive
  changes to the rules. Saves would also let a browser join a long online game without
  replaying it from the start.
* **Events with choices**: options for a player, picked by weight for the AI.
* **Diplomacy beyond war, peace and alliance**: offers the other side can refuse, vassals,
  relations that drift.
* **Troops**: supply lines, sieges with stages, transports (troops carrying troops), fog of war,
  retreat, generals.
* **More for players**: events that ask the player, a history of what happened to them, and
  saving a game in the browser.
* **Mods**: folders of TOML and scripts laid over a game.
* **Calibration**: runs compared with measured data, as the Extended GlassBox paper proposes
  for digital twins.
