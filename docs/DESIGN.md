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
| **Faction** | An actor with an account, an AI and a kind (`gang`, `police`, `other`) | 6 gangs, the police |
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
5. Each faction's **AI** takes up to `actions` decisions.
6. **Control**: gangs with presence above `contest` in the same place clash (each loses
   `attrition` times the others' presence, the place gets violence); presence decays, faster
   where the police is; places change hands when a gang has `hold` presence and `margin` times
   the next one's; a gang that loses a place loses its units there (seized or destroyed).
7. **Metrics** are recorded.

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
and fictional maps) work through the import: set `geojson = "file.geojson"` in `[city]`.

## Performance

Gangland, 900 neighbourhoods, 6 gangs, 730 days: about 1.4 s on one core. Experiments run their
runs on all cores; the five shipped experiments (about 200 runs) take under 30 s on 40 cores.

## Next

* **Saves** with stable ids (units, maps, resources by id, not position), so saves survive
  changes to the rules.
* **Events with choices**: options for a player, picked by weight for the AI.
* **Diplomacy**: relations between factions, truces and alliances as decisions and modifiers.
* **A player**: a faction controlled from an interface instead of the AI, then a map view
  (citysim's renderer, or Bevy).
* **Mods**: folders of TOML and scripts laid over a game.
* **Lockstep multiplayer** on top of determinism.
* **Calibration**: runs compared with measured data, as the Extended GlassBox paper proposes
  for digital twins.
