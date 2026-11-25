#!/usr/bin/env python3
"""Builds Gangland's Milan maps from open data: the Comune di Milano's 88 NIL (Nuclei di Identità
Locale), residents per NIL and income per postcode (CC BY 4.0), ISTAT's 2021 census sections with
their residents (CC BY 3.0 IT), and OpenStreetMap's addresses (ODbL) to tell which postcode each
section is in.

    python3 tools/milano.py [download-dir]

Downloads (into download-dir, default ./milano-data) and writes:

  games/gangland/maps/milano-areas.geojson     the 88 NIL
  games/gangland/maps/milano-sections.geojson  7,095 census sections (2021)

Every place gets `population` (a NIL: the residents in the city's registry, latest year; a
section: its 2021 census residents), `income` (the mean taxable income per taxpayer of the NIL's
postcodes, weighted by the residents of its sections, latest tax year) and `wealth` (income
scaled to 5..95 between the 5th and 95th percentile of the NIL). A section's postcode is the most
common one among the OpenStreetMap addresses inside it, or its neighbours'. Sections get their
NIL's income, as Bologna's get their area's. ISTAT's sections carry their NIL (COM_ASC2), with
the city's numbering.
"""
import csv
import io
import json
import os
import sys
import zipfile

from citymaps import (Coverage, assign, check, clean, fetch, fetch_postcodes, geometry, keyhole, land_use,
                      mef_income, neighbours, polygons_of, read_points, read_sections, read_table, to_local,
                      wealth_scale, write)

CKAN = "https://dati.comune.milano.it/dataset"
FILES = {
    # ds964: Nuclei d'Identità Locale (NIL) vigenti, PGT 2030
    "nil.geojson": f"{CKAN}/e8e765fc-d882-40b8-95d8-16ff3d39eb7c/resource/9c4e0776-56fc-4f3d-8a90-f4992a3be426/download/ds964_nil_wm.geojson",
    # ds27: Popolazione: residenti per cittadinanza e quartiere (NIL), 1999-2025
    "residents.zip": f"{CKAN}/36ee65f6-c1a2-4734-84e9-50135bf63b34/resource/6fd7172e-4c9a-48ee-b866-2c74c401c214/download/ds27_pop_sto_quartiere.zip",
    # ds3009: Redditi e variabili Irpef su base sub-comunale (CAP), 2025 a.i. 2024
    "income.csv": f"{CKAN}/e2abd355-2b2a-422e-9f83-aa9c1c1aecc0/resource/aecc4ba6-af02-4b84-a83e-05d647d42cf4/download/redditi_e_principali_variabili_irpef_su_base_subcomunale_csv_2024.csv",
    "R03_21.zip": "https://www.istat.it/storage/cartografia/basi_territoriali/2021/R03_21.zip",
}
PRO_COM = 15146
CENTRE = (9.19, 45.4642)


def residents(path):
    """Residents per NIL in the latest year of the series, and the year."""
    z = zipfile.ZipFile(path)
    by_year = {}
    with z.open(z.namelist()[0]) as f:
        for r in csv.DictReader(io.TextIOWrapper(f, "utf-8-sig"), delimiter=";"):
            y = by_year.setdefault(int(r["Anno"]), {})
            y[int(r["IdNil"])] = y.get(int(r["IdNil"]), 0) + int(r["Residenti"])
    year = max(by_year)
    return by_year[year], year


def income_rows(path):
    """The city's long table (year; CAP; variable; amount) as one dict per postcode."""
    rows, year = {}, 0
    with open(path, encoding="utf-8-sig") as f:
        for r in csv.reader(f, delimiter=";"):
            if len(r) < 4 or not r[0].strip().isdigit():
                continue
            year = max(year, int(r[0]))
            rows.setdefault(r[1].strip(), {"CAP": r[1].strip()})[r[2].strip()] = r[3].strip()
    return list(rows.values()), year


def main():
    d = sys.argv[1] if len(sys.argv) > 1 else "milano-data"
    os.makedirs(d, exist_ok=True)
    path = {name: fetch(d, name, url) for name, url in FILES.items()}
    rows, tax_year = income_rows(path["income.csv"])
    income = mef_income(rows)
    pop_nil, pop_year = residents(path["residents.zip"])
    names = {int(r["COM_ASC2"]) % 1000: r["DEN_ASC2"] for r in read_table(path["R03_21.zip"], "ASC2_R03_21.csv")
             if int(r["PRO_COM"]) == PRO_COM}

    sections, lm = to_local(read_sections(path["R03_21.zip"], PRO_COM), CENTRE)
    attrs = [r for r, _ in sections]
    pop = [int(r["POP21"] or 0) for r in attrs]
    nil_of = [int(r["COM_ASC2"]) % 1000 for r in attrs]
    print(len(sections), "sections,", sum(pop), "residents (2021 census)")

    # Postcodes, and the NIL's income.
    points = [lm.fwd((lon, lat)) + (cap,) for lat, lon, cap in read_points(fetch_postcodes(d, f"{PRO_COM:06d}")) if cap in income]
    shapes = [keyhole(s) for _, s in sections]
    caps, found = assign(shapes, points, neighbours(shapes))
    print(f"postcodes: {len(points)} addresses, {found} sections with some, {len(set(caps))} postcodes of {len(income)}")
    inc_sum, inc_w, nil_caps, nil_mun = {}, {}, {}, {}
    for n, r, cap, p in zip(nil_of, attrs, caps, pop):
        w = p if p > 0 else 0.01
        inc_sum[n] = inc_sum.get(n, 0) + w * income[cap]
        inc_w[n] = inc_w.get(n, 0) + w
        nil_caps.setdefault(n, set()).add(cap)
        m = nil_mun.setdefault(n, {})
        m[int(r["COM_ASC1"]) % 1000] = m.get(int(r["COM_ASC1"]) % 1000, 0) + p + 1
    nil_income = {n: round(inc_sum[n] / inc_w[n]) for n in inc_sum}
    wealth = wealth_scale(nil_income.values())

    # The NIL, from the city's map.
    with open(path["nil.geojson"]) as f:
        nil = sorted(json.load(f)["features"], key=lambda f: f["properties"]["ID_NIL"])
    shapes_nil = []
    for f in nil:
        g = f["geometry"]
        polys = [g["coordinates"]] if g["type"] == "Polygon" else g["coordinates"]
        rings = [[lm.fwd(tuple(p[:2])) for p in ring] for poly in polys for ring in poly]
        shapes_nil.append(keyhole(polygons_of([clean(r) for r in rings])))
    areas = []
    for f, shape in zip(nil, Coverage(shapes_nil).simplify(4.0).shapes):
        n = f["properties"]["ID_NIL"]
        name = names.get(n) or f["properties"]["NIL"].title()
        mun = max(nil_mun[n], key=nil_mun[n].get)
        areas.append({"type": "Feature", "properties": {
            "id": f"n{n:02d}", "name": name, "municipio": f"Municipio {mun}", "population": pop_nil.get(n, 0),
            "income": nil_income[n], "wealth": wealth(nil_income[n]), "postcodes": " ".join(sorted(nil_caps[n]))},
            "geometry": geometry(shape, lm.inv)})

    # Census sections.
    feats = []
    for r, n, p, shape in zip(attrs, nil_of, pop, Coverage(shapes).simplify(3.0).shapes):
        name = names.get(n, str(n))
        feats.append({"type": "Feature", "properties": {
            "id": f"s{int(r['SEZ21'])}", "name": f"{name} {int(r['SEZ21'])}", "area": name,
            "land_use": land_use(r["COD_TIPO_S"]), "population": p, "income": nil_income[n], "wealth": wealth(nil_income[n])},
            "geometry": geometry(shape, lm.inv)})

    credit = (f"Comune di Milano, open data (CC BY 4.0): NIL, residents {pop_year}, IRPEF income per CAP, tax year {tax_year}; "
              f"ISTAT, basi territoriali e censimento 2021 (CC BY 3.0 IT); © OpenStreetMap contributors, addresses (ODbL)")
    for name, fs in [("milano-areas.geojson", areas), ("milano-sections.geojson", feats)]:
        write(name, fs, credit)
        check(fs)


if __name__ == "__main__":
    main()
