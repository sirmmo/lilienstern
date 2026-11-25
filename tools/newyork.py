#!/usr/bin/env python3
"""Builds Gangland's New York maps from open data: NYC Department of City Planning's 2020
census tracts, clipped to the shoreline, with their Neighborhood Tabulation Areas (NYC Open
Data), and the US Census Bureau's American Community Survey 2019-2023 (5-year) estimates of
residents and median household income per tract (public domain).

    python3 tools/newyork.py [download-dir]

Downloads (into download-dir, default ./newyork-data) and writes:

  games/gangland/maps/newyork-areas.geojson     the 262 Neighborhood Tabulation Areas (2020)
  games/gangland/maps/newyork-sections.geojson  the 2,325 census tracts (2020)

Every place gets `population` (ACS residents: a tract's own, an area's sum), `income` (a tract's
median household income in dollars; an area's is its tracts' weighted by residents) and `wealth`
(income scaled to 5..95 between the 5th and 95th percentile of the residential areas). Tracts
with no income of their own (parks, airports, cemeteries, tracts too small for the survey) take
their area's; areas with nobody living in them (Central Park, JFK, Rikers Island, the
cemeteries) take their neighbours'.

Unlike Milan's and Rome's, a tract has its own income, not its area's: the survey has it.

The areas are the unions of their tracts (the 2020 NTAs are built from tracts), so the two maps
agree. New York is islands: the tracts are clipped to the shoreline, so the East River, the
Harlem River and the Narrows are gaps, and the boroughs meet on land (Brooklyn and Queens, the
Bronx and Marble Hill) and where the tracts touch at the bridges. The other crossings (CROSSINGS
below: the Harlem River bridges, the Triborough, the Whitestone and Throgs Neck bridges, the
Verrazzano, the Rockaway bridges, the Staten Island Ferry) are written as the map's `links`,
between the places nearest their two ends; a place still cut off joins its nearest place.
"""
import csv
import json
import math
import os
import sys

from citymaps import (Coverage, LocalMetres, check, clean, components, geometry, keyhole, neighbours, polygons_of,
                      wealth_scale, write, fetch)

SODA = "https://data.cityofnewyork.us/resource"
ACS = "https://www2.census.gov/programs-surveys/acs/summary_file/2023/table-based-SF/data/5YRData"
FILES = {
    # 2020 Census Tracts, clipped to the shoreline, with their NTA (NYC DCP, NYC Open Data 63ge-mke6)
    "tracts.geojson": f"{SODA}/63ge-mke6.geojson?$limit=5000",
    # 2020 Neighborhood Tabulation Areas (NYC DCP, NYC Open Data 9nt8-h7nd): their type
    "nta.geojson": f"{SODA}/9nt8-h7nd.geojson?$limit=5000",
    # ACS 2019-2023 5-year, table-based summary file: B01003 total population, B19013 median
    # household income in the past 12 months (2023 dollars)
    "b01003.dat": f"{ACS}/acsdt5y2023-b01003.dat",
    "b19013.dat": f"{ACS}/acsdt5y2023-b19013.dat",
}
CENTRE = (-73.94, 40.70)
COUNTIES = {"36005", "36047", "36061", "36081", "36085"}

# NTA types (NYC DCP): 0 residential, 5 Rikers Island, 6 special-purpose (navy yard, forts, rail
# yards, the UN), 7 cemetery, 8 airport, 9 park.
NTA_LAND_USE = {"0": "residential", "5": "prison", "6": "other", "7": "cemetery", "8": "airport", "9": "park"}

# The crossings the shoreline-clipped tracts do not join: (name, (lon, lat, borough), (lon, lat,
# borough)); each end is the tract of that borough nearest the point.
CROSSINGS = [
    ("Verrazzano-Narrows Bridge", (-74.0590, 40.6025, "Staten Island"), (-74.0330, 40.6100, "Brooklyn")),
    ("Staten Island Ferry", (-74.0730, 40.6435, "Staten Island"), (-74.0130, 40.7010, "Manhattan")),
    ("Marine Parkway Bridge", (-73.8830, 40.5700, "Queens"), (-73.8850, 40.5810, "Brooklyn")),
    ("Cross Bay Bridge", (-73.8195, 40.5860, "Queens"), (-73.8205, 40.6000, "Queens")),
    ("Willis Avenue Bridge", (-73.9285, 40.8045, "Bronx"), (-73.9335, 40.8010, "Manhattan")),
    ("Third Avenue Bridge", (-73.9300, 40.8075, "Bronx"), (-73.9340, 40.8045, "Manhattan")),
    ("Madison Avenue Bridge", (-73.9310, 40.8140, "Bronx"), (-73.9350, 40.8125, "Manhattan")),
    ("145th Street Bridge", (-73.9300, 40.8200, "Bronx"), (-73.9360, 40.8190, "Manhattan")),
    ("Macombs Dam Bridge", (-73.9295, 40.8285, "Bronx"), (-73.9350, 40.8280, "Manhattan")),
    ("Washington Bridge", (-73.9255, 40.8465, "Bronx"), (-73.9300, 40.8455, "Manhattan")),
    ("University Heights Bridge", (-73.9140, 40.8625, "Bronx"), (-73.9180, 40.8640, "Manhattan")),
    ("Triborough Bridge (Manhattan)", (-73.9225, 40.7935, "Manhattan"), (-73.9345, 40.7985, "Manhattan")),
    ("Triborough Bridge (Bronx)", (-73.9225, 40.7935, "Manhattan"), (-73.9190, 40.8020, "Bronx")),
    ("Triborough Bridge (Queens)", (-73.9225, 40.7935, "Manhattan"), (-73.9205, 40.7790, "Queens")),
    ("Whitestone Bridge", (-73.8320, 40.8110, "Bronx"), (-73.8230, 40.7930, "Queens")),
    ("Throgs Neck Bridge", (-73.7950, 40.8130, "Bronx"), (-73.7930, 40.7880, "Queens")),
    ("City Island Bridge", (-73.7890, 40.8560, "Bronx"), (-73.7990, 40.8590, "Bronx")),
]


def acs(path):
    """{tract GEOID: estimate} of an ACS table-based summary file (its first estimate column),
    New York City's tracts only; the survey's missing values (negative) are left out."""
    out = {}
    with open(path, encoding="utf-8") as f:
        rows = csv.reader(f, delimiter="|")
        next(rows)
        for r in rows:
            g = r[0]
            if g.startswith("1400000US") and g[9:14] in COUNTIES:
                v = float(r[1])
                if v >= 0:
                    out[g[9:]] = v
    return out


def nearest(shapes, idx, p):
    """The index in `idx` of the shape with a vertex nearest the point p."""
    return min(idx, key=lambda i: min((v[0] - p[0]) ** 2 + (v[1] - p[1]) ** 2
                                      for poly in shapes[i] for ring in poly for v in ring))


def main():
    d = sys.argv[1] if len(sys.argv) > 1 else "newyork-data"
    os.makedirs(d, exist_ok=True)
    path = {name: fetch(d, name, url) for name, url in FILES.items()}
    pop_t, inc_t = acs(path["b01003.dat"]), acs(path["b19013.dat"])
    with open(path["nta.geojson"]) as f:
        nta_type = {x["properties"]["nta2020"]: x["properties"]["ntatype"] for x in json.load(f)["features"]}
    with open(path["tracts.geojson"]) as f:
        tracts = sorted(json.load(f)["features"], key=lambda x: x["properties"]["geoid"])
    attrs = [t["properties"] for t in tracts]

    lm = LocalMetres(*CENTRE)
    memo = {}

    def fwd(p):
        k = (p[0], p[1])
        if k not in memo:
            memo[k] = lm.fwd(k)
        return memo[k]
    shapes = []
    for t in tracts:
        g = t["geometry"]
        polys = [g["coordinates"]] if g["type"] == "Polygon" else g["coordinates"]
        rings = [clean([fwd(p) for p in ring]) for poly in polys for ring in poly]
        shapes.append(keyhole(polygons_of(rings)))
    pop = [round(pop_t.get(a["geoid"], 0)) for a in attrs]
    nta = [a["nta2020"] for a in attrs]
    print(len(tracts), "tracts,", sum(pop), "residents (ACS 2019-2023),", sum(1 for a in attrs if a["geoid"] in inc_t), "with an income")

    # Crossings, and the groups still cut off joined to their nearest place.
    nb = neighbours(shapes)
    links = []
    for name, a, b in CROSSINGS:
        ends = [nearest(shapes, [i for i, r in enumerate(attrs) if r["boroname"] == e[2]], fwd(e[:2])) for e in (a, b)]
        if ends[0] != ends[1] and ends[1] not in nb[ends[0]]:
            links.append(tuple(ends))
            nb[ends[0]].add(ends[1])
            nb[ends[1]].add(ends[0])
            print(f"  {name}: {attrs[ends[0]]['ntaname']} {attrs[ends[0]]['ctlabel']} - {attrs[ends[1]]['ntaname']} {attrs[ends[1]]['ctlabel']}")
    while True:
        comps = components(nb)
        if len(comps) == 1:
            break
        main_c, other = set(comps[0]), comps[-1]
        verts = lambda i: [v for poly in shapes[i] for ring in poly for v in ring]
        best = min(((math.dist(p, q), a, b) for a in other for b in main_c
                    if math.dist(shapes[a][0][0][0], shapes[b][0][0][0]) < 20000
                    for p in verts(a)[::4] for q in verts(b)[::4]), default=None)
        _, a, b = best
        links.append((a, b))
        nb[a].add(b)
        nb[b].add(a)
        print(f"  across the water: {attrs[a]['ntaname']} {attrs[a]['ctlabel']} - {attrs[b]['ntaname']} {attrs[b]['ctlabel']} ({best[0]:.0f} m)")

    # Income: a tract's own; an area's weighted by residents; then the gaps.
    inc_sum, inc_w, nta_pop, nta_boro = {}, {}, {}, {}
    for a, n, p in zip(attrs, nta, pop):
        nta_pop[n] = nta_pop.get(n, 0) + p
        nta_boro[n] = a["boroname"]
        if a["geoid"] in inc_t and p > 0:
            inc_sum[n] = inc_sum.get(n, 0) + p * inc_t[a["geoid"]]
            inc_w[n] = inc_w.get(n, 0) + p
    nta_income = {n: round(inc_sum[n] / inc_w[n]) for n in inc_w}
    nta_nb = {n: set() for n in nta_pop}
    for i, js in enumerate(nb):
        for j in js:
            if nta[i] != nta[j]:
                nta_nb[nta[i]].add(nta[j])
    while len(nta_income) < len(nta_pop):
        for n in sorted(set(nta_pop) - set(nta_income)):
            known = [nta_income[m] for m in nta_nb[n] if m in nta_income]
            if known:
                nta_income[n] = round(sum(known) / len(known))
    wealth = wealth_scale([nta_income[n] for n in nta_income if nta_type.get(n) == "0" and nta_pop[n] > 0])
    income = [round(inc_t[a["geoid"]]) if a["geoid"] in inc_t else nta_income[n] for a, n in zip(attrs, nta)]

    # The areas: unions of their tracts.
    cov = Coverage(shapes).simplify(3.0)
    order = sorted(nta_pop)
    names = {a["nta2020"]: a["ntaname"] for a in attrs}
    unions = Coverage(cov.shapes).dissolve(nta)
    areas = []
    for n in order:
        areas.append({"type": "Feature", "properties": {
            "id": n, "name": names[n], "borough": nta_boro[n], "land_use": NTA_LAND_USE.get(nta_type.get(n), "other"),
            "population": nta_pop[n], "income": nta_income[n], "wealth": wealth(nta_income[n])},
            "geometry": geometry(unions[n], lm.inv)})
    area_links = sorted({tuple(sorted((nta[a], nta[b]))) for a, b in links if nta[a] != nta[b]})
    area_shapes = [keyhole(unions[n]) for n in order]
    area_nb = neighbours(area_shapes)
    pos = {n: k for k, n in enumerate(order)}
    # Two areas whose tracts are neighbours are neighbours, even where the simplified unions
    # touch at a single vertex: write those as links too.
    for i, js in enumerate(nb):
        for j in js:
            x, y = sorted((nta[i], nta[j]))
            if x != y and pos[y] not in area_nb[pos[x]] and (x, y) not in area_links:
                area_links.append((x, y))
    area_links = sorted(set(area_links))

    feats = []
    for k, (a, n, p, shape) in enumerate(zip(attrs, nta, pop, cov.shapes)):
        feats.append({"type": "Feature", "properties": {
            "id": f"t{a['geoid']}", "name": f"{names[n]} {a['ctlabel']}", "area": names[n], "borough": a["boroname"],
            "land_use": NTA_LAND_USE.get(nta_type.get(n), "other") if p == 0 or nta_type.get(n) != "0" else "residential",
            "population": p, "income": income[k], "wealth": wealth(income[k])},
            "geometry": geometry(shape, lm.inv)})
    tract_links = sorted((f"t{attrs[a]['geoid']}", f"t{attrs[b]['geoid']}") for a, b in links)

    credit = ("NYC Department of City Planning, 2020 census tracts and Neighborhood Tabulation Areas (NYC Open Data); "
              "US Census Bureau, American Community Survey 2019-2023 5-year estimates: residents and median household income")
    for name, fs, ls in [("newyork-areas.geojson", areas, area_links), ("newyork-sections.geojson", feats, tract_links)]:
        write(name, fs, credit, [list(x) for x in ls])
        check(fs, ls)
    print(f"{len(tract_links)} crossings between tracts, {len(area_links)} between areas")


if __name__ == "__main__":
    main()
