# Maps

Real maps for Gangland: Bologna, Milano, Roma and New York, each cut two ways, into large areas and small
blocks. Every place has `population`, `income` and `wealth` (income scaled 5 to 95 between the
5th and 95th percentile of the city's areas, clamped to 2..100); sections and blocks get the
income of their area (New York's tracts have their own).

| File | Places | |
|---|---:|---|
| `bologna-areas.geojson` | 90 | statistical areas, with their residents and income |
| `bologna-sections.geojson` | 2,606 | census sections; income of their area, residents of their area split by surface among its residential sections (an estimate) |
| `bologna.style.json` | 90 | a MapLibre style with the areas as a GeoJSON source over OpenStreetMap: a map can be given as a style too |
| `milano-areas.geojson` | 88 | the NIL (Nuclei di Identità Locale), with their residents and income |
| `milano-sections.geojson` | 7,095 | census sections (2021), with their census residents and the income of their NIL |
| `milano.style.json` | 88 | the NIL over OpenStreetMap |
| `roma-areas.geojson` | 155 | the zone urbanistiche, with their census residents and income |
| `roma-sections.geojson` | 4,200 | blocks: the census sections (2021) grouped within their zone, with their census residents and the income of their zone |
| `roma.style.json` | 155 | the zone urbanistiche over OpenStreetMap |
| `newyork-areas.geojson` | 262 | the Neighborhood Tabulation Areas (2020), with their residents and median household income |
| `newyork-sections.geojson` | 2,325 | census tracts (2020), with their own residents and median household income |
| `newyork.style.json` | 262 | the neighbourhoods over OpenStreetMap |

```sh
lilienstern run games/gangland --set city_file=maps/bologna-sections.geojson --map out/bologna
lilienstern run games/gangland --set city_file=maps/bologna.style.json --set gangs=4 --map out/bologna-areas
lilienstern run games/gangland --set city_file=maps/milano-areas.geojson --map out/milano
lilienstern run games/gangland --set city_file=maps/roma-sections.geojson --map out/roma
lilienstern run games/gangland --set city_file=maps/newyork-sections.geojson --map out/newyork
```

Any GeoJSON of polygons works: polygons sharing a border are neighbours, numeric properties
become fields (`wealth`, `density`, `population` are used by Gangland's maps; missing ones are
made up from the distance to the centre, as in a generated city). Gangland's `radius_km`,
`river`, `bridges` and `city_seed` shape only a generated city: on a real map the city is the
map, and a river is in the data (Rome's, below). A map can carry its own crossings beside its
features, `"links": [["id", "id"], ...]` (place ids or names): bridges, tunnels and ferries
between places that share no border (New York's, below).

## Bologna

From the open data of the Comune di Bologna ([opendata.comune.bologna.it](https://opendata.comune.bologna.it),
CC BY 4.0): statistical areas, 2021 census sections, median taxable income per area (2024) and
residents per area (2024). Rebuild them with `python3 tools/bologna.py`.

## Milano

Rebuild them with `python3 tools/milano.py [download-dir]` (standard library only; it downloads
about 75 MB).

| Data | Source | Licence |
|---|---|---|
| The 88 NIL (PGT 2030, in force) | Comune di Milano, [ds964](https://dati.comune.milano.it/dataset/ds964-nil-vigenti-pgt-2030) | CC BY 4.0 |
| Residents per NIL, 1999-2025 (the latest year is used) | Comune di Milano, [ds27](https://dati.comune.milano.it/dataset/ds27-popolazione-residenti-cittadinanza-quartiere-serie-storica) | CC BY 4.0 |
| IRPEF income per postcode (CAP), tax year 2024 | Comune di Milano, [ds3009](https://dati.comune.milano.it/dataset/ds3009-redditi-e-variabili-irpef-su-base-sub-comunale-2025-a-i-2024) (the Ministry of Economy and Finance's figures) | CC BY 4.0 |
| 2021 census sections, their residents (`POP21`) and NIL (`COM_ASC2`) | ISTAT, basi territoriali 2021, [R03_21.zip](https://www.istat.it/storage/cartografia/basi_territoriali/2021/R03_21.zip) (Lombardia) | CC BY 3.0 IT |
| Addresses with a postcode (about 40,000), to place the postcodes | © [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors, through the Overpass API | ODbL 1.0 |

* **Areas**: the NIL's outlines from the city, simplified by 4 m; `population` from the city's
  registry (1,399,079 residents at the end of 2025).
* **Sections**: ISTAT's, which carry their NIL in the city's numbering; `population` from the
  2021 census (1,349,930 in all, ISTAT's count). Milan has no large river through the centre
  (the Navigli are canals, the Lambro runs along the eastern edge): every section is kept.
* **Income**: the city publishes income per postcode, not per NIL. Each section takes the
  postcode most common among the OpenStreetMap addresses inside it (4,929 of 7,095 sections
  have some; the others take their neighbours'), and a NIL's `income` is the mean taxable
  income per taxpayer (`Reddito imponibile`: amount over number of taxpayers) of its sections'
  postcodes, weighted by their residents. It is a mean, not Bologna's median, and only as fine
  as Milan's 38 postcodes: NIL that share a postcode get close values.

## Roma

Rebuild them with `python3 tools/roma.py [download-dir]` (standard library only; it downloads
about 50 MB). Roma Capitale's open data portal ([dati.comune.roma.it](https://dati.comune.roma.it))
has no zone boundaries, residents or income per zone, so the maps come from ISTAT and the
Ministry of Economy and Finance.

| Data | Source | Licence |
|---|---|---|
| 2021 census sections, their residents (`POP21`), municipio (`COM_ASC1`) and zona urbanistica (`COM_ASC3`), and the zones' names | ISTAT, basi territoriali 2021, [R12_21.zip](https://www.istat.it/storage/cartografia/basi_territoriali/2021/R12_21.zip) (Lazio) | CC BY 3.0 IT |
| IRPEF income per postcode (CAP), tax year 2024 | Ministero dell'Economia e delle Finanze, Dipartimento delle Finanze, [open data](https://www1.finanze.gov.it/finanze/analisi_stat/public/index.php?opendata=yes) ("Redditi e principali variabili IRPEF su base subcomunale") | CC BY 3.0 IT |
| Addresses with a postcode (about 58,000), to place the postcodes | © [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors, through the Overpass API | ODbL 1.0 |

* **Areas**: the 155 zone urbanistiche, each the union of its census sections, simplified by
  6 m; `population` is the sum of their 2021 census residents (2,749,031 in all, ISTAT's count).
* **Blocks**: Rome's 23,591 sections are too many for a game in the browser (an 11 MB map, and
  a day ten times slower than Bologna's), so `roma-sections.geojson` groups them within each
  zone into 4,200 blocks: the smallest block joins its smallest neighbour until each has 600
  residents or 0.15 km². A block has its sections' residents and their number (`sections`).
* **The Tiber and the Aniene**: their river sections (ISTAT `COD_ACQUE` 2xxxx) are left out, so
  the rivers are gaps crossed at the bridges; the 8 that stay (around the Isola Tiberina, and
  one at Ostia) would otherwise cut places off the city. The zones are the unions of the
  sections that are left.
* **Income**: as in Milan, from each section's postcode (7,101 of 23,439 sections have
  OpenStreetMap addresses with one; the others take their neighbours'), weighted by residents,
  per zone: 72 postcodes for 155 zones.
* Martignano, Rome's exclave by Lake Martignano, borders nothing else in the city: Lilienstern
  joins it to its two nearest places.

## New York

Rebuild them with `python3 tools/newyork.py [download-dir]` (standard library only; it downloads
about 47 MB).

| Data | Source | Licence |
|---|---|---|
| 2020 census tracts, clipped to the shoreline, with their Neighborhood Tabulation Area | NYC Department of City Planning, [2020 Census Tracts](https://data.cityofnewyork.us/d/63ge-mke6) (NYC Open Data) | NYC Open Data [terms of use](https://www.nyc.gov/home/terms-of-use.page) |
| The NTAs' types (residential, park, cemetery, airport, ...) | NYC Department of City Planning, [2020 Neighborhood Tabulation Areas](https://data.cityofnewyork.us/d/9nt8-h7nd) (NYC Open Data) | as above |
| Residents (B01003) and median household income (B19013) per tract | US Census Bureau, [American Community Survey 2019-2023 5-year estimates](https://www2.census.gov/programs-surveys/acs/summary_file/2023/table-based-SF/), table-based summary file | public domain (a US government work) |

* **Sections**: the 2,325 tracts, simplified by 3 m. `population` is the survey's estimate
  (8,516,202 in all); `income` is the tract's own median household income in 2023 dollars
  (2,192 tracts have one); tracts without one (parks, airports, cemeteries, tracts too small
  for the survey) take their area's.
* **Areas**: the 262 NTAs, each the union of its tracts (NYC's NTAs are built from tracts), so
  both maps agree. An area's `income` is its tracts' medians weighted by their residents; the
  areas nobody lives in (Central Park, JFK and LaGuardia, Rikers Island, the cemeteries) take
  their neighbours'. The wealth scale runs over the residential areas: from the South Bronx
  (about $30,000) to Tribeca (about $220,000). `land_use` and `borough` come with them.
* **The water**: the tracts end at the shore, so the East River, the Harlem River, the Narrows
  and Jamaica Bay are gaps. The boroughs meet on land (Brooklyn and Queens; the Bronx and
  Marble Hill) and where the tracts touch near the East River crossings (the Brooklyn,
  Manhattan, Williamsburg and Queensboro bridges, the tunnels). The other crossings are the
  map's `links`, each between the two tracts nearest its ends (`CROSSINGS` in the script): the
  seven Harlem River bridges, the Triborough's three arms, the Whitestone and Throgs Neck
  bridges, the Verrazzano, the Marine Parkway and Cross Bay bridges to the Rockaways, the City
  Island Bridge and the Staten Island Ferry. Hart Island, Hoffman and Swinburne Islands and the
  Statue of Liberty's islands join their nearest place across the water. The areas' map has the
  same crossings, between the areas of those tracts.

The Milan, Rome and New York scripts share `tools/citymaps.py`: reading ISTAT's shapefiles, the projection
from UTM 32N, a simplification that keeps shared borders shared, unions of polygons, and the
postcodes. A hole is joined to its outer ring by a slit (a keyhole polygon), so that a place
inside another shares vertices with it and is its neighbour. Coordinates are rounded to 5
decimals (about a metre).
