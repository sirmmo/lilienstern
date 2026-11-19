# Italia's maps

Italy as ISTAT drew it for the census of 1871, the first census of the Kingdom of Italy with
Veneto and Rome. Its provinces are, for the most part, those of the Italian states before
unification (the provinces of the Kingdom of the Two Sicilies, the Papal delegations, the
provinces of Lombardy-Venetia and Piedmont), and many go back much further: the game takes them
as the lands that were fought over in the centuries before.

| File | Places | |
|---|---:|---|
| `italia-1871-circondari.geojson` | 284 | circondari and (in Veneto and Mantua) distretti, the units inside provinces |
| `italia-1871-province.geojson` | 69 | provinces |
| `italia-cities.geojson` | 73 | points: the capital of every province, with its 1871 name (Girgenti for Agrigento, Porto Maurizio, Aquila, Reggio di Calabria), and the families' other seats (Taranto, Rimini, Urbino, Oristano) |

Both are made by `tools/italia.py` from:

* **Boundaries**: ISTAT, [Confini statistico-amministrativi: analisi storica](https://www.istat.it/notizia/confini-statistico-amministrativi-analisi-storica/),
  1871 (shapefiles, reprojected to WGS84 and simplified on a 0.004° grid, which keeps shared
  borders shared). ISTAT's data are released under a Creative Commons Attribution licence. ISTAT draws
  the comune of Catanzaro apart from its province (Calabria Ulteriore II) under the same code;
  the two are merged.
* **Population**: the 1871 census by province, and for the 13 largest cities, as tabulated by
  [DEMOPAST](https://demopast.it) from ISTAT's census volumes. A circondario gets its big city's
  people (Napoli, Milano...) and a share of the rest of its province by area. ISTAT's circondari
  carry no province code; each one belongs to the province where most of its area is.
* **Cities**: coordinates of the comuni from [Wikidata](https://www.wikidata.org) (CC0); the
  provinces carry their capital's name as `capital`.
* **Elevation**: SRTM 90 m, sampled at about 16 points inside each place through the
  [OpenTopoData](https://www.opentopodata.org) public API (cached in `tools/italia-elevation.json`).

Properties: `name`, `kind` (provincia, circondario, distretto), `province`, `istat` (ISTAT's
code), `population` (1871), `area_km2`, `density`, `wealth` (density on a log scale between its
5th and 95th percentiles, 10 to 90, plus 15 where one of the 13 largest cities is), `elevation`
(mean, metres), `relief` (highest minus lowest sample) and `university` (universities founded
before 1800 there: Bologna, Modena, Padova, Napoli, Siena, Roma, Perugia, Camerino, Pisa, Firenze,
Pavia, Ferrara, Torino, Catania, Genova, Urbino, Macerata, Messina, Sassari, Parma, Cagliari and
the medical school of Salerno; in the province map, how many each province has).

Not in 1871 Italy, so not on the map: Trentino, Trieste and Istria, Nice and Savoy, Corsica,
Malta. Islands are joined to the nearest place on the mainland (`connect = true`): Sicily at
Messina and Reggio, Sardinia across the Tyrrhenian Sea.
