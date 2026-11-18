#!/usr/bin/env python3
"""Builds Gangland's Bologna maps from the city's open data (CC BY 4.0, Comune di Bologna).

    python3 tools/bologna.py [download-dir]

Downloads (into download-dir, default ./bologna-data) the 90 statistical areas, the 2021 census
sections, the median taxable income per area and the residents per area, and writes:

  games/gangland/maps/bologna-areas.geojson     90 statistical areas
  games/gangland/maps/bologna-sections.geojson  2,606 census sections (2021)

Every place gets `population`, `income` (median taxable income of its area, latest year) and
`wealth` (income scaled to 5..95). Sections get their area's income, and their area's residents
split by surface among its residential sections (other sections get a tenth of the weight): a
rough guess at where people live, not census counts.
"""
import json
import os
import sys
import urllib.request

API = "https://opendata.comune.bologna.it/api/explore/v2.1/catalog/datasets"
FILES = {
    "areas.geojson": f"{API}/aree-statistiche/exports/geojson",
    "sections.geojson": f"{API}/sezioni-di-censimento-anno-2021/exports/geojson",
    "income.json": f"{API}/reddito-mediano-per-area-statistica/exports/json",
    "residents.json": f"{API}/popolazione-residente-per-eta-sesso-cittadinanza-quartiere-zona-area-statistica-/exports/json"
    "?select=anno,codice_area_statistica,sum(residenti)%20as%20residenti&group_by=anno,codice_area_statistica",
}
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "games", "gangland", "maps")


def key(name):
    return "".join(c for c in name.upper() if c.isalnum())


def load(d, name):
    path = os.path.join(d, name)
    if not os.path.exists(path):
        print("downloading", FILES[name])
        urllib.request.urlretrieve(FILES[name], path)
    with open(path) as f:
        return json.load(f)


def rounded(geom):
    def r(c):
        return [round(c[0], 6), round(c[1], 6)] if isinstance(c[0], (int, float)) else [r(x) for x in c]
    return {"type": geom["type"], "coordinates": r(geom["coordinates"])}


def ring_area(ring):
    a = 0.0
    for (x1, y1), (x2, y2) in zip(ring, ring[1:] + ring[:1]):
        a += x1 * y2 - x2 * y1
    return abs(a) / 2


def main():
    d = sys.argv[1] if len(sys.argv) > 1 else "bologna-data"
    os.makedirs(d, exist_ok=True)
    areas = load(d, "areas.geojson")["features"]
    sections = load(d, "sections.geojson")["features"]
    income_rows = load(d, "income.json")
    resident_rows = load(d, "residents.json")

    year = max(r["anno"] for r in income_rows)
    income = {key(r["area_statistica"]): r["reddito_imponibile_mediano_dei_contribuenti_residenti"] for r in income_rows if r["anno"] == year}
    ryear = max(r["anno"] for r in resident_rows)
    residents = {int(r["codice_area_statistica"]): r["residenti"] for r in resident_rows if r["anno"] == ryear}
    wealth = None  # set once the areas' incomes are known

    out_areas = []
    by_code = {}
    for f in areas:
        p = f["properties"]
        code = int(p["codice_area_statistica"])
        name = p["area_statistica"].upper()
        inc = income.get(key(name))
        if inc is None:  # some names are cut short in the income data
            near = [k for k in income if key(name).startswith(k) and len(k) >= 10]
            inc = income[near[0]] if len(near) == 1 else None
        if inc is None:
            print("no income for", name)
            inc = sorted(income.values())[len(income) // 2]
        by_code[code] = (name, inc)
        out_areas.append({
            "type": "Feature",
            "properties": {"id": f"a{code:02d}", "name": p["area_statistica"].title(), "quartiere": p["quartiere"], "zona": p["zona"],
                           "population": residents.get(code, 0), "income": inc, "wealth": 0},
            "geometry": rounded(f["geometry"]),
        })

    # Wealth: income scaled between the 5th and 95th percentile of the areas, so a few tiny
    # areas (a rail yard with a handful of taxpayers) do not set the scale.
    incs = sorted(inc for _, inc in by_code.values())
    lo, hi = incs[len(incs) * 5 // 100], incs[len(incs) * 95 // 100]

    def wealth(inc):
        return round(min(100.0, max(2.0, 5 + 90 * (inc - lo) / (hi - lo))), 1)

    for f in out_areas:
        f["properties"]["wealth"] = wealth(f["properties"]["income"])

    weight = {}
    for f in sections:
        p = f["properties"]
        w = ring_area(f["geometry"]["coordinates"][0]) * (1.0 if p["descrizione"].startswith("Edificio o area ad uso civile") else 0.1)
        weight[id(f)] = w
    total = {}
    for f in sections:
        c = int(f["properties"]["cod_areast"])
        total[c] = total.get(c, 0) + weight[id(f)]
    out_sections = []
    for f in sections:
        p = f["properties"]
        c = int(p["cod_areast"])
        name, inc = by_code.get(c, (p["area_statistica"], sorted(income.values())[len(income) // 2]))
        pop = residents.get(c, 0) * weight[id(f)] / total[c] if total[c] else 0
        out_sections.append({
            "type": "Feature",
            "properties": {"id": f"s{int(p['sez_2021_istat']):04d}", "name": f"{name.title()} {p['sez_2021_istat']}", "area": name.title(),
                           "quartiere": p["quartiere"], "land_use": p["descrizione"], "population": round(pop),
                           "income": inc, "wealth": wealth(inc)},
            "geometry": rounded(f["geometry"]),
        })

    credit = f"Comune di Bologna, open data (CC BY 4.0): statistical areas, census sections 2021, median income {year}, residents {ryear[:4]}"
    for name, feats in [("bologna-areas.geojson", out_areas), ("bologna-sections.geojson", out_sections)]:
        with open(os.path.join(OUT, name), "w") as f:
            json.dump({"type": "FeatureCollection", "attribution": credit, "features": feats}, f, separators=(",", ":"))
        print(name, len(feats), "places,", sum(x["properties"]["population"] for x in feats), "residents")


if __name__ == "__main__":
    main()
