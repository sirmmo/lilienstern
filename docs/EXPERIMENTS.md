# Experiments

An experiment plays a game under several **variants** (sets of parameters), each over several
**seeds**, in parallel, and compares them.

```toml
name = "Policing strategies"
question = "Where should a fixed police budget go?"
description = "..."
game = "../games/gangland"      # relative to this file
days = 730
seeds = 12                       # seeds 0..11 (seed_base = 0)
report = ["violence", "hotspots", "m.Raids"]   # rows of the report (default: the usual ones)
geojson = true                   # a map of the first seed of each variant

[set]                            # parameters for every variant
neighborhoods = 900

[[variant]]
name = "no police"
set = { police_budget = 0 }

[[variant]]
name = "hot spots"               # no `set`: the game's own parameters
```

```sh
lilienstern experiment experiments/policing.toml [--out DIR] [--seeds N] [--days N] [--threads N]
```

It writes to `out/<experiment>/`:

* `report.md`: the variants and a table of mean ± standard deviation over seeds;
* `summary.csv`: one row per run, every summary value;
* `series/<variant>/seed-N.csv`: each run's time series;
* `maps/<variant>/`: a viewer for the first seed (`index.html` replays it over the basemap,
  `style.json`, `map.geojson`).

Variants with the same parameters and seeds give the same numbers: in the results below, the
"hot spots" variant of the policing experiment, the "no shock" variant and the "no arrests"
variant are the same runs, and they match to the last digit.

## Gangland's experiments

The numbers below come from the shipped files (12 seeds, 10 for granularity; 730 days; about
900 neighbourhoods). They are properties of a toy model, useful for what they show about the
model and the engine, not claims about real cities.

### Policing strategies (`experiments/policing.toml`)

Where should the same police budget go? `hotspot_places` counts neighbourhoods that were
violent at some point: if patrols only pushed violence next door, it would go up.

| | no police | random patrols | hot spots | rich districts | hot spots x2 |
|---|---:|---:|---:|---:|---:|
| `violence` | 6631 ± 1250 | 5669 ± 1214 | 3979 ± 455 | 4957 ± 636 | 3592 ± 451 |
| `hotspot_places` | 244 ± 47 | 212 ± 27 | 183 ± 32 | 207 ± 29 | 172 ± 24 |
| `captures` | 225 ± 37 | 186 ± 31 | 131 ± 13 | 155 ± 27 | 117 ± 20 |
| `held` | 0.86 | 0.80 | 0.72 | 0.79 | 0.65 |
| `m.Raids` | 0 | 156 ± 21 | 72 ± 14 | 51 ± 9 | 161 ± 24 |

* Hot-spot policing cuts violence by 40% against no police and 30% against random patrols, and
  violence reaches *fewer* neighbourhoods: here policing does not displace it.
* Doubling the hot-spot budget buys only another 10%: returns diminish.
* Random patrols score low, so the police AI spends its budget on raids more often (156 against
  72), yet violence stays high: raids hit business, while patrols where gangs meet hit the wars.

### How many gangs (`experiments/gangs.toml`)

| | 2 gangs | 4 gangs | 8 gangs | 16 gangs |
|---|---:|---:|---:|---:|
| `violence` | 650 ± 165 | 1973 ± 315 | 7821 ± 1119 | 78350 ± 5436 |
| `captures` | 30 ± 10 | 68 ± 13 | 228 ± 26 | 2258 ± 62 |
| `hotspots` (daily) | 3.0 | 6.8 | 17.0 | 76.1 |
| `held` | 0.17 | 0.45 | 0.95 | 1.00 |
| `top_share` | 0.09 | 0.13 | 0.14 | 0.08 |

Violence grows much faster than the number of gangs: twice the gangs, three to ten times the
violence. With 2 or 4 gangs the city is not full and gangs rarely meet; from 8 on the city is
full and every border is a front.

### Economic shock (`experiments/shock.toml`)

On day 365 the eastern half of the city loses 20% ("mild") or 60% ("severe") of its wealth.

| | no shock | mild | severe |
|---|---:|---:|---:|
| `m.HotDaysEast` | 3579 ± 740 | 2073 ± 654 | 845 ± 281 |
| `m.HotDaysWest` | 3830 ± 707 | 5001 ± 869 | 6232 ± 825 |
| `violence` | 3979 ± 455 | 3864 ± 556 | 3934 ± 651 |
| `m.Extortion` | 235067 | 195869 | 136797 |

Total violence does not change; it **moves west**, into the half that is still worth fighting
over. Gangland's gangs chase money (their weights favour wealth, rackets need it), so a poorer
east is a quieter east. A model where poverty drives violence would need recruitment to drive
aggression too: a one-line change in `politics.lil` (`weight global Crew ...` on `attack`, or a
condition on wealth), and a good next experiment.

### Arresting the leaders (`experiments/decapitation.toml`)

On day 365 the largest gang loses its crew and 60% ("partial") or 90% ("total") of its presence.

| | no arrests | partial | total |
|---|---:|---:|---:|
| `violence` | 3979 ± 455 | 3750 ± 520 | 3712 ± 476 |
| `captures` | 131 ± 13 | 127 ± 20 | 126 ± 22 |
| `top_share` | 0.140 | 0.141 | 0.132 |
| `hhi` | 0.169 | 0.169 | 0.168 |

Almost no effect. Gangland's gangs end up the same size (HHI ≈ 1/6), so the "largest" is barely
larger, and presence comes back within weeks from crew income. A more interesting version needs
unequal gangs (different starting cash, or `hq_spacing` putting one in the centre) and leaders
that matter (income tied to a headquarters unit).

### Neighbourhood size (`experiments/granularity.toml`)

The same city (same wealth and density everywhere) cut into about 225, 900 and 3600
neighbourhoods; the fourth variant scales per-neighbourhood costs down and decisions up by 4.

| | 225 | 900 | 3600 | 3600 scaled |
|---|---:|---:|---:|---:|
| `held` | 0.99 | 0.72 | 0.17 | 0.52 |
| `top_share` | 0.21 | 0.14 | 0.04 | 0.10 |
| `hotspot_spread` | 0.89 | 0.21 | 0.06 | 0.12 |
| `captures_share` | 3.88 | 0.15 | 0.03 | 0.22 |
| `hhi` | 0.170 | 0.169 | 0.171 | 0.171 |

The model is strongly scale-dependent: a gang move takes one neighbourhood, so the finer the
cut, the less of the city a gang can hold, and the less often gangs meet. Scaling costs and
actions recovers part of it, not all: decay, attrition and diffusion are per neighbourhood too.
Only the HHI (how territory is shared between gangs) is stable. This is the modifiable areal
unit problem in an agent model, and the reason to state the spatial unit with any result; for
a real city, the import from GeoJSON makes the unit the city's own (statistical areas,
neighbourhoods, blocks).

### Policing in Bologna (`experiments/bologna.toml`)

The policing question on a real city: Bologna's 2,606 census sections, 8 seeds.

| | no police | hot spots | rich districts |
|---|---:|---:|---:|
| `violence` | 3517 ± 830 | 1172 ± 474 | 3539 ± 870 |
| `hotspot_places` | 263 ± 89 | 166 ± 44 | 259 ± 67 |
| `captures` | 122 ± 39 | 39 ± 13 | 123 ± 36 |
| `held` | 0.26 | 0.24 | 0.25 |
| `m.Raids` | 0 | 107 ± 14 | 51 ± 10 |

Hot-spot policing works even better here (violence down by two thirds), and guarding the rich
districts does nothing: in Bologna the gangs start and fight in the poorer sections, far from
the hill and the centre. With small sections and one section per move, gangs hold only a
quarter of the city after two years.

### Policing in Milano and Roma (`experiments/milano.toml`, `experiments/roma.toml`)

The same question on Milan's 7,095 census sections and on Rome's 4,200 blocks (census sections
grouped within their zona urbanistica, with the Tiber crossed at its bridges), 8 seeds each.

| Milano | no police | hot spots | rich districts |
|---|---:|---:|---:|
| `violence` | 164 ± 283 | 32 ± 75 | 143 ± 334 |
| `hotspot_places` | 5 ± 10 | 4 ± 11 | 7 ± 12 |
| `captures` | 6 ± 11 | 0.5 ± 1.4 | 1.5 ± 2.8 |
| `held` | 0.13 | 0.12 | 0.13 |
| `m.Raids` | 0 | 6 ± 8 | 5 ± 5 |

| Roma | no police | hot spots | rich districts |
|---|---:|---:|---:|
| `violence` | 3430 ± 964 | 1347 ± 772 | 2411 ± 1104 |
| `hotspot_places` | 189 ± 63 | 100 ± 59 | 140 ± 56 |
| `captures` | 102 ± 46 | 47 ± 28 | 80 ± 43 |
| `held` | 0.18 | 0.17 | 0.18 |
| `m.Raids` | 0 | 72 ± 21 | 39 ± 10 |

In Rome the result holds: hot spots cut violence by 60%, guarding the rich districts by less
and within the spread of the seeds. In Milan the gangs hardly meet: the poorer sections, where
they start, lie far apart at the edges of a city cut into 7,095 small sections, and one section
a move does not bring them together in two years. Violence is a twentieth of Bologna's, too rare
for eight seeds to tell the policies apart: the spatial unit decides again (see Neighbourhood
size): on Milan's 88 NIL the gangs hold the whole city, and fight over it, within four months.

### Policing in New York (`experiments/newyork.toml`)

The same question on New York's 2,325 census tracts, with each tract's own median household
income, and the boroughs joined only at their bridges, tunnels and the Staten Island Ferry; 8
seeds.

| New York | no police | hot spots | rich districts |
|---|---:|---:|---:|
| `violence` | 3955 ± 1643 | 2383 ± 573 | 3725 ± 1118 |
| `hotspot_places` | 179 ± 66 | 146 ± 35 | 168 ± 37 |
| `captures` | 154 ± 61 | 95 ± 26 | 153 ± 43 |
| `held` | 0.33 | 0.30 | 0.31 |
| `m.Raids` | 0 | 81 ± 13 | 46 ± 9 |

The result holds again: hot spots cut violence by 40% and captures by 38%, while guarding the
rich districts changes neither beyond the spread of the seeds. New York's poor tracts lie
close together (the South Bronx, Harlem, central Brooklyn), so the gangs meet early, as in Rome
and unlike Milan, and they hold a third of the city after two years.

## Marches' experiment

### Alliances and eras (`experiments/marches.toml`)

Five realms, about 230 provinces, three years, 16 seeds, with each era's tech tree (knights and
hovertanks need a tech first) and the economy of `people.lil`: no cap on armies, taxes and
levies that follow each province's happiness. In "alliance" the first two realms (Aldmark and
Brennholt) start allied; "future" swaps the medieval troops, terrain and techs for the future
era's (`era = "future"`), with the same land and decisions.

| | medieval | alliance | future | future, alliance |
|---|---:|---:|---:|---:|
| `territory.realm1` | 19.0 ± 29.3 | 64.9 ± 49.5 | 49.9 ± 64.5 | 78.8 ± 58.2 |
| `territory.realm2` | 37.4 ± 48.7 | 89.7 ± 64.5 | 46.4 ± 48.8 | 66.6 ± 40.2 |
| `territory.realm3` | 75.9 ± 71.6 | 29.0 ± 38.7 | 36.2 ± 53.4 | 33.0 ± 48.0 |
| `gangs_alive` (realms left) | 3.50 | 3.00 | 3.88 | 3.88 |
| `battles` | 993 ± 192 | 791 ± 240 | 892 ± 235 | 644 ± 297 |
| `destroyed` | 244 ± 40 | 196 ± 59 | 263 ± 64 | 197 ± 84 |
| `top_share` | 0.61 | 0.57 | 0.54 | 0.57 |
| `m.Truces` | 10.3 | 8.2 | 4.3 | 3.5 |
| `m.Revolts` (province-days) | 212 | 147 | 195 | 190 |
| `unpaid` (realm-days) | 0.9 | 0.3 | 15.4 | 22.2 |

* The alliance pays: the two allies end with 155 provinces between them instead of 56 (145
  instead of 96 in the future era), most of a land of about 230. They never fight each other
  and their troops cross each other's land, so they face one front fewer each.
* With an alliance there are fewer battles, not more: the allies win sooner, and a smaller,
  settled map has fewer fronts.
* The future era makes less peace (4 truces a game against 10): drones and hovertanks reach a
  capital in days, so a realm under threat is lost before it can afford a truce. Its troops
  are dearer to keep, and it is the era where realms run out of money: 15 to 22 days a game
  with troops unpaid, against less than one.
* Since armies have no cap (only their cost: see DESIGN.md, "The economy"), more realms
  survive (3.5 of 5 against 2.6 before) and the largest holds less (61% against 68%): the AI
  keeps fewer troops than the old cap of 14 allowed (about 16 on the whole map at the end),
  and conquered provinces are unhappy and pay little.
* The spread is wide (the standard deviations are as large as the means): which realm wins
  depends on where the capitals fall and who meets whom first, as in the genre.

## Italia's experiment

### Who takes Italy (`experiments/italia.toml`)

Eight families at their seats on ISTAT's 1871 Italy (Savoia in Torino, Visconti in Milano, Della
Scala in Verona, Medici in Firenze, Este in Ferrara, Colonna in Roma, Aragona in Napoli,
Chiaramonte in Palermo), five years, 16 seeds; taxes, recruits and learning follow the 1871
population (and the old universities), taxes and recruits also how happy each area is; terrain
follows the real elevation; the medieval tech tree; no cap on armies. Territory at the end, in
places:

| | circondari (284) | provinces (69) |
|---|---:|---:|
| Savoia | 114.1 ± 80.4 | 12.6 ± 9.6 |
| Visconti | 17.6 ± 43.2 | 8.2 ± 10.9 |
| Della Scala | 6.9 ± 18.0 | 13.8 ± 9.4 |
| Medici | 0 | 5.8 ± 7.2 |
| Este | 1.1 ± 3.4 | 0.9 ± 3.8 |
| Colonna | 1.4 ± 5.8 | 6.3 ± 5.1 |
| Aragona | 4.6 ± 12.8 | 3.6 ± 3.9 |
| Chiaramonte | 137.5 ± 75.6 | 17.8 ± 4.9 |
| families left | 2.6 | 4.8 |
| `top_share` | 0.65 | 0.33 |
| largest at the end | Savoia 8, Chiaramonte 8 | Della Scala 6, Chiaramonte 5, Visconti 3, Savoia 2 |
| `m.Revolts` (area-days) | 615 | 85 |

* **Sicily holds and takes the South.** Chiaramonte is never destroyed and has the steadiest
  result: the island is rich, and the only way in is the strait, which one army can hold. From
  there it takes Calabria and Naples: Aragona, in Napoli, usually falls to it. The
  Kingdom of Sicily, once again, without being told to.
* **In the North, the Po valley decides, and now Savoia takes it.** With the cap, Visconti
  (Milano, the richest seat) was largest 8 times in 16; with armies that cost what they cost
  and levies that anger the people, Savoia is, 8 times, and Visconti never. Why has not been
  traced yet (a guess: Milano raises its troops in one place, and its levies anger the richest
  taxpayers of the family). Este, in Ferrara between Milan, Verona and Florence, still never
  survives.
* **The cut of the land changes the war.** On 69 provinces, each a large and slow conquest,
  almost five families are left after five years and the largest holds 33% of Italy; on 284
  circondari, under three are left and the largest holds 65%. The same areal-unit effect as
  Gangland's granularity experiment, on a real map.
* **Unrest follows the war.** Areas in revolt (about one a day on the circondari) are where
  the fronts were: battles keep a province near 25, half its taxes, and levies or unpaid troops
  on top of that tip it over.

## Ideas for more

* **Poverty and recruitment**: make aggression grow with crew and see whether the shock result
  flips.
* **Unequal gangs**: one rich gang in the centre, five poor ones on the edges.
* **Calibration**: compare Bologna's simulated hotspots with recorded data (the Extended
  GlassBox validation loop).
* **Terrain and defence**: the same war with `defense_per_star = 0`, or without the river's
  bridges, to see how much of the outcome the land decides.
* **Police tactics over time**: switch strategy on a given day with an event, and look at the
  time series (`series/*/seed-N.csv`) around it.
