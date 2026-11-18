# Maps

Real maps for Gangland, from the open data of the Comune di Bologna (CC BY 4.0): statistical
areas, 2021 census sections, median taxable income per area (2024) and residents per area
(2024). Rebuild them with `python3 tools/bologna.py`.

| File | Places | |
|---|---:|---|
| `bologna-areas.geojson` | 90 | statistical areas, with their residents and income |
| `bologna-sections.geojson` | 2,606 | census sections; income of their area, residents of their area split by surface among its residential sections (an estimate) |
| `bologna.style.json` | 90 | a MapLibre style with the areas as a GeoJSON source over OpenStreetMap: a map can be given as a style too |

Every place has `population`, `income` and `wealth` (income scaled 5 to 95 between the 5th and
95th percentile of the areas).

```sh
lilienstern run games/gangland --set city_file=maps/bologna-sections.geojson --map out/bologna
lilienstern run games/gangland --set city_file=maps/bologna.style.json --set gangs=4 --map out/bologna-areas
```

Any GeoJSON of polygons works: polygons sharing a border are neighbours, numeric properties
become fields (`wealth`, `density`, `population` are used by Gangland's maps; missing ones are
made up from the distance to the centre, as in a generated city).
