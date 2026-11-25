#!/usr/bin/env python3
"""Builds Gangland's Rome maps from open data: ISTAT's 2021 census sections, with their residents
(CC BY 3.0 IT), the Ministry of Economy and Finance's income per postcode (CC BY 3.0 IT), and
OpenStreetMap's addresses (ODbL) to tell which postcode each section is in.

    python3 tools/roma.py [download-dir]

Downloads (into download-dir, default ./roma-data) and writes:

  games/gangland/maps/roma-areas.geojson     the 155 zone urbanistiche
  games/gangland/maps/roma-sections.geojson  the census sections (2021), grouped into 4,200 blocks

Roma Capitale's open data portal (dati.comune.roma.it) has no zone boundaries, residents or
income per zone, so the zones are ISTAT's: the sections carry their zona urbanistica (the
third level of Rome's sub-municipal areas, COM_ASC3), and a zone is the union of its sections.

Every place gets `population` (2021 census residents: a section's own, a zone's sum), `income`
(the mean taxable income per taxpayer of the zone's postcodes, weighted by the residents of its
sections, latest tax year) and `wealth` (income scaled to 5..95 between the 5th and 95th
percentile of the zones). A section's postcode is the most common one among the OpenStreetMap
addresses inside it, or its neighbours'. Blocks get their zone's income, as Bologna's sections
get their area's.

Rome has 23,591 sections, too many for a game in the browser: within each zone, the smallest
block joins its smallest neighbour until every block has BLOCK_RESIDENTS residents or BLOCK_KM2.

The Tiber and the Aniene are left out of the sections (their ISTAT river sections), so that they
are gaps crossed at the bridges; the zones are the unions of the sections that are left.
"""
import os
import sys

from citymaps import (Coverage, area_of, assign, check, drop_rivers, fetch, fetch_postcodes, geometry, group, keyhole,
                      land_use, mef_income, neighbours, read_points, read_sections, read_table, to_local, wealth_scale,
                      write)

ISTAT = "https://www.istat.it/storage/cartografia/basi_territoriali/2021/R12_21.zip"
MEF_YEAR = 2024
MEF = ("https://www1.finanze.gov.it/finanze/analisi_stat/public/v_4_0_0/contenuti/"
       f"Redditi_e_principali_variabili_IRPEF_su_base_subcomunale_CSV_{MEF_YEAR}.zip")
PRO_COM = 58091
CENTRE = (12.4964, 41.9028)
BLOCK_RESIDENTS, BLOCK_KM2 = 600, 0.15  # a block: sections of one zone with at least this many residents or km²
ROMAN = ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII", "XIV", "XV"]


def mef_rows(path, pro_com):
    import csv
    import io
    import zipfile
    z = zipfile.ZipFile(path)
    with z.open(z.namelist()[0]) as f:
        rows = csv.DictReader(io.TextIOWrapper(f, "latin1"), delimiter=";")
        return [r for r in rows if int(r["Codice Istat Comune"] or 0) == pro_com]


def main():
    d = sys.argv[1] if len(sys.argv) > 1 else "roma-data"
    os.makedirs(d, exist_ok=True)
    istat = fetch(d, "R12_21.zip", ISTAT)
    income = mef_income(mef_rows(fetch(d, f"mef-cap-{MEF_YEAR}.zip", MEF), PRO_COM))
    zones = {r["COM_ASC3"]: (r["COD_ASC3_T"], r["DEN_ASC3"]) for r in read_table(istat, "ASC3_R12_21.csv") if int(r["PRO_COM"]) == PRO_COM}

    sections, lm = to_local(read_sections(istat, PRO_COM), CENTRE)
    print(len(sections), "sections,", sum(int(r["POP21"] or 0) for r, _ in sections), "residents (2021 census)")
    sections = drop_rivers(sections)
    attrs = [r for r, _ in sections]
    pop = [int(r["POP21"] or 0) for r in attrs]

    # Postcodes, and the zones' income.
    points = [lm.fwd((lon, lat)) + (cap,) for lat, lon, cap in read_points(fetch_postcodes(d, f"{PRO_COM:06d}")) if cap in income]
    shapes = [keyhole(s) for _, s in sections]
    caps, found = assign(shapes, points, neighbours(shapes))
    print(f"postcodes: {len(points)} addresses, {found} sections with some, {len(set(caps))} postcodes of {len(income)}")
    inc_sum, inc_w, zone_caps = {}, {}, {}
    for r, cap, p in zip(attrs, caps, pop):
        z = r["COM_ASC3"]
        w = p if p > 0 else 0.01
        inc_sum[z] = inc_sum.get(z, 0) + w * income[cap]
        inc_w[z] = inc_w.get(z, 0) + w
        zone_caps.setdefault(z, set()).add(cap)
    zone_income = {z: round(inc_sum[z] / inc_w[z]) for z in inc_sum}
    wealth = wealth_scale(zone_income.values())

    # Zones: the union of their sections.
    groups = [r["COM_ASC3"] for r in attrs]
    union = Coverage([s for _, s in sections]).dissolve(groups)
    def zone_key(z):  # "1a", "1b", ..., "10l", "11y", ...
        code = zones[z][0]
        digits = "".join(c for c in code if c.isdigit())
        return (int(digits or 0), code)
    order = sorted(union, key=zone_key)
    zone_pop, zone_mun = {}, {}
    for r, p in zip(attrs, pop):
        zone_pop[r["COM_ASC3"]] = zone_pop.get(r["COM_ASC3"], 0) + p
        zone_mun.setdefault(r["COM_ASC3"], {}).setdefault(int(r["COM_ASC1"]) % 1000, 0)
        zone_mun[r["COM_ASC3"]][int(r["COM_ASC1"]) % 1000] += p + 1
    zcov = Coverage([keyhole(union[z]) for z in order]).simplify(6.0)
    areas = []
    for z, shape in zip(order, zcov.shapes):
        code, name = zones[z]
        mun = max(zone_mun[z], key=zone_mun[z].get)
        areas.append({"type": "Feature", "properties": {
            "id": f"z{code}", "name": name, "code": code, "municipio": f"Municipio {ROMAN[mun]}",
            "population": zone_pop[z], "income": zone_income[z], "wealth": wealth(zone_income[z]),
            "postcodes": " ".join(sorted(zone_caps[z]))},
            "geometry": geometry(shape, lm.inv)})

    # Blocks: census sections grouped within their zone (23,000 sections are too many for a
    # game in the browser).
    nb = neighbours(shapes)
    block = group(shapes, [r["COM_ASC3"] for r in attrs], pop, nb, BLOCK_RESIDENTS, BLOCK_KM2)
    bunion = Coverage([s for _, s in sections]).dissolve(block)
    members = {}
    for i, b in enumerate(block):
        members.setdefault(b, []).append(i)
    blocks = sorted(members, key=lambda b: (zone_key(attrs[b]["COM_ASC3"]), int(attrs[b]["SEZ21"])))
    bcov = Coverage([keyhole(bunion[b]) for b in blocks]).simplify(4.0)
    feats, nth = [], {}
    for b, shape in zip(blocks, bcov.shapes):
        z = attrs[b]["COM_ASC3"]
        nth[z] = nth.get(z, 0) + 1
        ms = members[b]
        uses = {}
        for i in ms:
            u = land_use(attrs[i]["COD_TIPO_S"])
            uses[u] = uses.get(u, 0) + abs(area_of(shapes[i]))
        feats.append({"type": "Feature", "properties": {
            "id": f"b{int(attrs[min(ms, key=lambda i: int(attrs[i]['SEZ21']))]['SEZ21'])}", "name": f"{zones[z][1]} {nth[z]}",
            "area": zones[z][1], "land_use": max(uses, key=uses.get), "sections": len(ms),
            "population": sum(pop[i] for i in ms), "income": zone_income[z], "wealth": wealth(zone_income[z])},
            "geometry": geometry(shape, lm.inv)})

    credit = (f"ISTAT, basi territoriali e censimento 2021 (CC BY 3.0 IT); MEF Dipartimento delle Finanze, redditi IRPEF "
              f"per CAP, anno d'imposta {MEF_YEAR} (CC BY 3.0 IT); © OpenStreetMap contributors, addresses (ODbL)")
    for name, fs in [("roma-areas.geojson", areas), ("roma-sections.geojson", feats)]:
        write(name, fs, credit)
        check(fs)


if __name__ == "__main__":
    main()
