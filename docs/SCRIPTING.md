# Writing a game

A game is a folder:

```text
games/gangland/
├── params.toml     its numbers, used as $name everywhere
├── game.toml       maps, resources, units, factions, control, city
├── economy.gbx     GlassBox rules
└── politics.lil    events, decisions, modifiers
```

A game with troops (`games/marches`) also has its troops and terrain in files that `game.toml`
includes, one per era:

```text
games/marches/
├── params.toml     era = "medieval"
├── game.toml       include = ["eras/$era.toml"]
├── realms.lil
└── eras/
    ├── medieval.toml   [war], [[terrain]], [[troop]]: levies, archers, men-at-arms, knights
    └── future.toml     the same ids: militia, drone wings, battle mechs, hovertanks
```

Script files may hold any mix of GlassBox and Lilienstern blocks; the extension is only a hint.
`lilienstern check <folder>` loads a game and lists what it declares; errors give the file and
line.

## Parameters

`params.toml` is a flat list of numbers, strings and booleans:

```toml
gangs = 6
expand_crew = 3
```

`$gangs` is replaced by its value in `game.toml` and in the scripts before they are read, so a
parameter can stand anywhere: a count, a cost, a weight, a file name (`"$city_file"`). `$$` is a
dollar sign; `$name` in a `#` comment is left alone. A run (`--set gangs=8`) or an experiment
overrides parameters; a name the game does not declare is an error.

## game.toml

```toml
name = "Gangland"
description = "..."
scripts = ["economy.gbx", "politics.lil"]
days = 730               # default length of a run
sample_every = 10        # days between rows of the time series
include = ["eras/$era.toml"]   # more of game.toml, merged in
```

An included file is read like `game.toml` (parameters work in it and in its name). Its lists of
tables (`[[troop]]`, `[[map]]`, ...) are added to the game's, its tables merged; a value set in
both is an error. A name that is empty once its parameters are filled in is skipped, so a
parameter can switch a part of the game on: Gangland's `include = ["$crews"]` adds crews and
techs only when `crews = "crews.toml"`.

### [city]

| Key | Default | |
|---|---|---|
| `neighborhoods` | 600 | about how many places the generated city has |
| `radius_km` | 10 | |
| `river`, `bridges` | true, 5 | a river across the city, crossed by this many bridges |
| `enclaves` | 2 | rich districts away from the centre |
| `city_seed` | -1 | -1: a new city for each run's seed; a number: always this city |
| `origin` | [0, 0] | [lon, lat] of the centre, for GeoJSON |
| `geojson` | | import places from this file instead of generating them: a GeoJSON, or a MapLibre style (`"version": 8`) with a GeoJSON source; empty means none |
| `source` | | in a MapLibre style: the GeoJSON source with the places (default: the first) |
| `name_property` | "name" | imported places: the property with the name |
| `fields` | | imported places: fields from other properties, as `{ wealth = "income_index" }` |
| `synthesize` | true | imported places missing `wealth`, `density` or `population` get them as a generated city would |
| `links` | | imported places: extra borders by place name, `[["Messina", "Reggio di Calabria"]]` (straits, sea routes) |
| `connect` | false | imported places: join each group cut off from the largest (islands) to its nearest place, instead of dropping it |
| `basemap` | | what map viewers draw underneath: "none", "osm", a MapLibre style URL or file. Default: the imported style, else "osm" for imported places and "none" for a generated city |

Generated places have the fields `x`, `y` (-1 to 1), `centrality`, `wealth` (0 to 100),
`density` (people per km²), `population`, `area_km2`, and `elevation` and `forest` (0 to 1,
smooth over the map, for terrain). Imported places have `x`, `y`,
`centrality`, `area_km2` and their numeric properties (lowercased); `density` is derived from
`population` if missing, and with `synthesize` they get `elevation` and `forest` too. Imported polygons that share at least two vertices are neighbours. In a MapLibre style, GeoJSON
sources given as file names are read relative to the style; the places' source must be a file
or inline (download a source that is on the web).

### [[map]]

```toml
[[map]]
id = "Heat"
init = "wealth"     # a field of the place, or a number (default 0)
scale = 1.0         # multiplies init
decay = 0.05        # share lost every day
diffuse = 0.05      # share spread evenly to the neighbours every day
min = 0
max = 100
```

Maps a script uses without declaring them start at 0 with no decay or diffusion.

### [[resource]]

```toml
[[resource]]
id = "Product"
scope = "agent"     # local, global or agent
reach = 6           # agent: steps an agent travels
private = true      # agent: only to units of the same faction
```

### [[unit]]

```toml
[[unit]]
id = "corner"
name = "Street corner"
```

### [[faction]]

```toml
[[faction]]
id = "gang"
kind = "gang"                 # gang, realm, police or other
count = 6                     # gang1 ... gang6
name = "Gang"
names = ["Vipers", "Saints"]  # names of the copies, in order
colors = ["#e6194b"]          # cycled; a palette by default
start = { Cash = 120 }        # global resources at the start
income = { Budget = 40 }      # every day
start_units = ["safehouse"]   # built at the headquarters
start_presence = 40           # at the headquarters, 60% of it around
start_radius = 1
hq_field = "wealth"           # headquarters only where this field...
hq_max = 40                   # ...is at most this
hq_spacing = 0.5              # 0: anywhere, 1: as far from the others as possible
actions = 3                   # decisions per day
choice = "weighted"           # weighted or best
noise = 0.25                  # randomness of target scores (score x 1 ± noise)
patience = 20                 # decisions it saves for an unaffordable one
saving = "short"              # or "goal": see below
team = "north"                # factions of the same team start allied; with count > 1, a
                              # list or a comma-separated string, one per copy ("a,a,,")
start_troops = ["light", "heavy"]   # troops at the headquarters
capital = "Milano"            # the headquarters, by place name ("Urbino|Pesaro e Urbino": the
                              # first the map has); with count > 1, a list, one per copy
```

A faction with `start_presence`, `start_units` or `start_troops` gets a headquarters. `gang`
and `realm` factions hold places (they are the same to the engine; the names are for the
reader); `police` and `other` act through decisions and troops.

**Saving.** When the decision the AI picks is unaffordable, the faction saves: for up to
`patience` decisions it takes only decisions that do not spend what it is short of. With
`saving = "short"` (the default) the next pick decides again what it saves for, so a cheap
decision picked in between ends the saving and dear ones are rarely bought. With `"goal"` the
faction keeps saving for what it first picked, which it takes as soon as it can afford it (if
that decision still has a target). Marches uses `"goal"`, or its realms would raise only levies.

### [control]

| Key | |
|---|---|
| `hold` | presence needed to hold a place |
| `margin` | ...and this many times the next gang's presence |
| `contest` | above this presence, gangs in the same place clash |
| `attrition` | share of the others' presence each side loses per day of clash |
| `violence` | violence added per unit of presence of all but the strongest |
| `decay` | share of presence lost every day |
| `police_map`, `police_attrition` | presence is also multiplied by `1 - police_attrition x police / 100` |
| `violence_map` | default "Violence" |
| `on_capture` | "seize" or "destroy" the units of a gang that loses a place |
| `hotspot` | violence above which a place counts as a hotspot (default 5) |

A holder keeps its place while its presence is at least half of `hold`, unless another gang
has `hold` and `margin` times its presence.

### Troops: [war], [[terrain]], [[troop]]

Troops are things a faction owns that stand in a place, march to others and fight. All of
them, and the ground they cross, are declared in data:

```toml
[war]
distance = "km"         # "steps" (each border costs 1) or "km" (centre to centre)
damage_scale = 0.35     # share of the damage table dealt per day of battle (default 1)
defense_per_star = 10   # % of damage removed per defence star, times the defender's health
home_defense = 1        # stars for troops in places their faction or an ally holds
fort_map = "Fort"       # a map whose value adds stars, and slows occupation
luck = 10               # a random 0..luck added to every hit
heal = 3                # health regained per day in friendly places without enemies
unpaid = 4              # health lost per day when the faction pays none of the upkeep
violence = 0.2          # added to the violence map per point of damage
relation = "war"        # between factions of different teams at the start: war or peace

[[terrain]]             # a place's terrain is the first that matches it
id = "hills"
field = "elevation"     # a field of the place; with no field, matches every place
above = 0.59            # and/or `below`
defense = 2             # stars
move_cost = { foot = 1.6, horse = 2 }   # per movement type; a type missing cannot enter
color = "#b5a77f"       # for viewers

[[troop]]
id = "fast"
name = "Knights"
move = "horse"          # movement type (default "foot")
speed = 45              # per day, in [war] distance units
cost = { Funds = 70, Recruits = 5 }     # global resources, paid by `do raise`
upkeep = { Funds = 0.6 }                # per day
damage = { light = 85, ranged = 80, heavy = 55, fast = 50 }   # % of a full troop, by target id
occupy = 2              # presence a full troop adds per day where there is no enemy troop
icon = "K"              # a mark for viewers (default: the first letter)
description = "..."
```

Places with no matching terrain (and every place, when no terrain is declared) cost 1 to enter
and give no stars. Troops have health from 0 to 100; strength counts a full troop as 1.

**Upkeep** is paid every morning, after income and events and before any decision: each
faction pays all its troops' upkeep, as much of it as it has. Nothing else limits an army: a
faction may raise more troops than its income can feed, and empty its treasury keeping them.

**A day of troops** (after the decisions, before control):

1. *March.* Troops with orders move along their route, all at the same time, a border at a
   time: entering a place costs its distance (1, or km) times the terrain's cost for the
   troop's movement type, and a troop spends up to `speed` a day. A troop that enters a place
   with enemy troops stops there. Routes are the cheapest way, and avoid terrain the troop
   cannot enter and places held by factions it is at peace with.
2. *Battle.* Where factions at war have troops in the same place, each troop hits the enemy
   it does the most damage to: `(damage + luck roll) x damage_scale x health/100 x (1 - cover)`,
   where cover is `stars x defense_per_star x the target's health/100` percent. Every hit is
   worked out before any is dealt, so the order of troops does not matter. A troop at 0 is
   destroyed.
3. *Occupation.* A troop in a place without enemy troops adds `occupy x health/100` (divided
   by 1 + the fort map) to its faction's presence there, and takes as much from the presence of
   every faction at war with it; control then decides who holds the place. Troops do not
   occupy places held by allies or factions at peace.
4. *Supply.* Troops lose `unpaid` times the share of the morning's upkeep their faction could
   not pay (for the worst of its resources); troops in friendly places with no enemies heal
   `heal`.

**Relations.** Every pair of factions is at `war`, at `peace` (no fighting; neither enters the
other's places) or `ally` (they share places, never fight, never take each other's places).
Factions of the same `team` start allied, the others as `[war] relation` says; decisions and
events change relations with `do relation`. Gangs' presence clashes (`[control]`) also happen
only between factions at war.

A person plays troops with `World::order(faction, troop, place)` (and `World::route` to see
the way and the days first); the AI moves them with `do march`.

### Techs: [research], [[tech]]

A tech tree is data too. Each faction researches one tech at a time: every day, all of its
research resource (made by scripts, like any global resource) goes into it, and when the
points reach the tech's cost it is known (what is left over counts for the next one).

```toml
[research]
resource = "Learning"

[[tech]]
id = "chivalry"
name = "Chivalry"
cost = 240
requires = ["horses"]   # techs it needs (a tech cannot need itself, even through others)
weight = 1              # how much the AI wants it
damage = 10             # bonuses to the faction's troops: damage %, defense (stars), speed %,
troops = ["fast"]       # occupy %, upkeep % (negative: cheaper); for these troops (default: all)
description = "Knights, and the charge."
```

* A troop with `requires = "chivalry"` can only be raised by factions that know it; a decision
  that raises it gets the condition by itself.
* Scripts test techs with `if tech chivalry` (and count them with `techs`), so a tech can open
  decisions or change income: `event study / scope place / if tech universities / do global
  Learning add 0.025`.
* `start_techs = ["horses"]` in a `[[faction]]`: known from the start.
* The AI picks its next tech at random, in proportion to the techs' `weight`; a player picks
  with `World::set_research` (the browser's `research` request). Until a player picks, its
  points wait in its account.

An era file can bring its own decisions: `scripts = ["$eras_dir/medieval.lil"]` in an included
file adds to the game's scripts (Marches' medieval era raises arquebusiers that way).

## GlassBox blocks

```text
unitRule cook                 # also `rule`
    appliesTo lab             # unit ids (default: the rule's name)
    rate 1                    # fires every n days
    global Cash in 2          # from the unit's faction
    local Precursor in 1      # held by the unit
    agent Product out 12      # sent to units that need it
    Waste out 1               # no scope: local
    map Heat out 1 radius 1   # emitted: amount / (1 + steps) within the radius
    map Wealth in 0.02        # taken from the place
    map Police less 40        # a test: atLeast, atMost, greater, less, is
    test global Cash atLeast 10
    log Batches 1             # metric, scaled by how well the rule ran
end

mapRule fear                  # on every place
    map Violence greater 10
    map Wealth in 0.1
    map Heat out 1
end

agentRule trafficking         # wherever an agent passes
    condition resource=Product
    repeatAfter 1             # every n steps, counting the start
    area Traffic 1 0          # map, amount per unit carried, radius
    area -map=Air pm10 1 0    # Extended GlassBox form: map Air.pm10
end
```

* **Tests** are gates: a failed test stops the rule.
* **Inputs** scale the rule: with half of what it needs, it takes that half and gives half.
* **Outputs** are multiplied by the place's output factor (modifiers).
* A unit takes up to twice what its rules need of an agent resource, less what it holds; agents
  go to the nearest units first.
* `successEvent`, `onFail`, `options` are accepted and not simulated; `zoneRule` is ignored.

## Lilienstern blocks

### decision

```text
decision attack
    label Attack the block    # what a player sees (default: the name)
    for gang                  # gang, police, other or any (default)
    target adjacent           # see below
    every 1                   # considered every n days
    cost global Crew 6        # paid when taken; unaffordable decisions are saved for
    if owner other            # conditions on the faction and the target
    do rivals add -25         # effects
    log Attacks
    weight 4                  # score: the sum of the weights
    weight map Wealth 0.06
end
```

`label` is the name a player sees (by default the decision's name, with spaces).

**Targets:** `none` (the faction itself), `owned`, `adjacent` (bordering its territory, not
its own), `present` (where it has presence), `contested` (where it and a rival both have more
than `contest`), `any`.

### event

```text
event shock
    scope place               # place (each place), faction (each faction) or world
    for gang                  # place events: only places held by this kind; faction: these factions
    every 1
    if day is 365
    if map X greater 0
    do map Wealth mul 0.6
    log Shocked
end
```

In a place event, the faction is the place's holder (none if nobody holds it). `label Taxes`
names the event where a player sees what it gave or took (the treasury); the default is its
name with spaces.

### The ledger

Every change to a faction's global resources is booked under where it came from: its
`income`, each event (by its label), each GlassBox unit rule (by the name of its units: "Street
corner", "Lab"), each decision (its costs and effects, by its label), research, troop upkeep,
and "other" for what no script names (GlassBox map and agent rules), with how many places,
units, troops or decisions made each amount. A unit rule's money counts as `income`. `World::yesterday` is the last
finished day (a player's decisions count in the day they were taken), `World::ledger_total`
the whole game, and `World::yields[resource][place]` what each place's events gave its holder
on the last day. Scripts read it through `income`, `upkeep` and `balance` below; the browser's
treasury shows it.

### modifier

```text
modifier lockdown
    output mul 0.3            # the place's GlassBox output factor
    daily map Heat add -2     # an effect every day it lasts
end
```

Applied with `do modifier lockdown 20` (days). Applying it again extends it.

### Conditions (`if ...`)

`target` may be written before a place condition for clarity; in a decision, place conditions
always read the target.

| Condition | True when |
|---|---|
| `map M <cmp> n` | the map at the place |
| `presence <cmp> n` | the faction's presence at the place |
| `rival <cmp> n` | the strongest other gang's presence at the place |
| `owner is self\|other\|none\|any` | who holds the place |
| `units [U] <cmp> n` | living units (of kind U) at the place |
| `global R <cmp> n` | the faction's resource |
| `territory <cmp> n` | places the faction holds |
| `rank <cmp> n` | its rank by territory among factions of its kind (1: largest) |
| `day <cmp> n` | the day, from 0 |
| `chance p` | with probability p (also a statement of its own) |
| `troops <cmp> n` | the faction's troops at the place (a full troop counts 1) |
| `foes <cmp> n` | troops at the place of factions at war with the faction |
| `threat <cmp> n` | the same, at the place and the places next to it |
| `army <cmp> n` | how many troops the faction has |
| `idle <cmp> n` | how many of them have no orders |
| `relation war\|peace\|ally` | the faction's relation with the place's holder (false if nobody, or the faction itself, holds it) |
| `tech T` | the faction knows tech T |
| `techs <cmp> n` | how many techs it knows |
| `income R <cmp> n` | what the faction's land gave of resource R on the last day (its `income` and every event) |
| `upkeep R <cmp> n` | what its troops cost of R a day |
| `balance R <cmp> n` | income less upkeep |
| `unpaid <cmp> n` | 1 if it could not pay all its troops' upkeep (today, or on the last day before they are paid) |

`<cmp>` is `atLeast`, `atMost`, `greater`, `less` or `is`.

### Effects (`do ...`)

| Effect | |
|---|---|
| `map M add\|mul\|set n [radius r]` | `add` spreads as `n / (1 + steps)`; `mul` and `set` apply to every place in the radius |
| `presence add\|mul\|set n` | the faction's presence at the place |
| `rivals add\|mul\|set n` | every other gang's presence at the place |
| `all presence add\|mul\|set n` | every gang's presence at the place |
| `turf add\|mul\|set n` | the faction's presence in every place it holds |
| `global R add\|mul\|set n` | the faction's resource |
| `global R add\|mul\|set map M k [times map M2 k2]...` | the faction's resource, by k x the map at the place (income that follows the land), times k2 x M2 for each `times` (`global Funds add map Population 0.0017 times map Happiness 0.02`) |
| `global R add\|mul\|set n times map M k...` | n times k x M at the place |
| `modifier NAME days` | |
| `build U` | a unit of the faction at the place |
| `seize` | other factions' units at the place become the faction's |
| `destroy rival\|all` | removes units at the place |
| `log Metric [n]` | adds n (default 1) to a metric |
| `raise T` | a troop of kind T for the faction at the place; in a decision, the troop's cost is added to the decision's |
| `march [n\|all]` | sends up to n (default 1) of the faction's troops that have no orders and are not fighting to the place, those that get there soonest first |
| `relation war\|peace\|ally` | sets the relation between the faction and the place's holder |
| `disband` | removes the faction's weakest troop at the place (its upkeep with it) |

### Weights (`weight ...`)

| Weight | Adds |
|---|---|
| `weight k` | k |
| `weight map M k` | k x the map at the target |
| `weight presence k` | k x the faction's presence there |
| `weight rival k` | k x the strongest rival's presence there |
| `weight unowned k` | k if nobody holds the target |
| `weight units [U] k` | k x the units there |
| `weight global R k` | k x the faction's resource (careful: unbounded quantities end up outweighing everything) |
| `weight troops\|foes\|threat\|army\|idle\|techs\|unpaid k` | k x the count, as in the conditions |
| `weight income\|upkeep\|balance R k` | k x the faction's economy in R, as in the conditions (`weight balance Funds 0.25`: an AI that minds its means) |

A decision whose score is not positive is not taken.

## Metrics

Every run records, per day: violence, clashes and captures (summed between samples), contested
places, hotspots, gangs alive, the HHI of territory, the largest gang's share, the share held,
the mean of every map, every script metric (`m.<name>`), each gang's territory and each
faction's resources. The summary of a run adds totals, daily means (`map.<id>`), the number and
share of places that were ever hotspots, and how often each decision was taken
(`decision.<name>`).

Games with troops add `battles` (places where troops fought, per day), `losses` (health lost
in battle), `troops` (alive) to the series, and `battles`, `losses`, `raised`, `destroyed`,
`troops`, `unpaid` (faction-days with troops not fully paid) and each holder's
`territory.<faction>` to the summary; games with techs add `learnt`
and each faction's `techs.<faction>`.
