# GlassBox

The rule language Maxis presented for SimCity's GlassBox engine (Andrew Willmott, *Inside
GlassBox*, GDC 2012), with the metrics and agent rules of the Extended GlassBox model for urban
digital twins (Costagliola, Montanari, Bellavista, *Interconnecting Urban Networks: a Novel
Approach to Digital Twins through GlassBox Adaptation*), run on a graph of places instead of a
tile grid: provinces, neighbourhoods, districts.

* **Units** sit in places and run **unit rules**: they take resources in and put them out.
* **Resources** are `local` (held by the unit), `global` (held by the unit's account: a
  treasury, a faction) or `agent` (sent to units that need them along the graph).
* **Maps** are values per place (pollution, land value, heat). Rules read them, emit into them
  and take from them; maps decay and diffuse to neighbouring places.
* **Map rules** run on every place, **agent rules** on every place an agent passes.
* **Metrics** (`log`) are counted every tick.

`compile` turns script text into `Rules`, `State::new` lays out the units, maps and accounts,
and `step` runs one tick on a graph given as each place's neighbours. It has no dependencies.

It is the economy layer of [Lilienstern](https://crates.io/crates/lilienstern), a grand-strategy
engine; the language is described in its
[SCRIPTING.md](https://github.com/sirmmo/lilienstern/blob/main/docs/SCRIPTING.md).

Licence: GPL-3.0-or-later.
