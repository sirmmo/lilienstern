# Writing a game

A game is a folder:

```text
games/gangland/
├── params.toml     its numbers, used as $name everywhere
├── game.toml       maps, resources, units, factions, control, city
├── economy.gbx     GlassBox rules
└── politics.lil    events, decisions, modifiers
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
```

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
| `basemap` | | what map viewers draw underneath: "none", "osm", a MapLibre style URL or file. Default: the imported style, else "osm" for imported places and "none" for a generated city |

Generated places have the fields `x`, `y` (-1 to 1), `centrality`, `wealth` (0 to 100),
`density` (people per km²), `population` and `area_km2`. Imported places have `x`, `y`,
`centrality`, `area_km2` and their numeric properties (lowercased); `density` is derived from
`population` if missing. Imported polygons that share at least two vertices are neighbours. In a MapLibre style, GeoJSON
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
kind = "gang"                 # gang, police or other
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
```

A faction with `start_presence` or `start_units` gets a headquarters. Only `gang` factions
hold places; `police` and `other` act through decisions.

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

In a place event, the faction is the place's holder (none if nobody holds it).

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
| `modifier NAME days` | |
| `build U` | a unit of the faction at the place |
| `seize` | other factions' units at the place become the faction's |
| `destroy rival\|all` | removes units at the place |
| `log Metric [n]` | adds n (default 1) to a metric |

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

A decision whose score is not positive is not taken.

## Metrics

Every run records, per day: violence, clashes and captures (summed between samples), contested
places, hotspots, gangs alive, the HHI of territory, the largest gang's share, the share held,
the mean of every map, every script metric (`m.<name>`), each gang's territory and each
faction's resources. The summary of a run adds totals, daily means (`map.<id>`), the number and
share of places that were ever hotspots, and how often each decision was taken
(`decision.<name>`).
