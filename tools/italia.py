#!/usr/bin/env python3
"""Builds the maps of Italia (games/italia/maps) from open data:

* boundaries: ISTAT, "Confini statistico-amministrativi: analisi storica", the 1871 census
  (provinces, and circondari and distretti), https://www.istat.it/notizia/confini-statistico-amministrativi-analisi-storica/
* population: the 1871 census by province and for the 13 largest cities, as tabulated by
  DEMOPAST (https://demopast.it) from ISTAT's census volumes;
* elevation: SRTM (90 m), sampled through the OpenTopoData public API
  (https://www.opentopodata.org), cached in tools/italia-elevation.json so later builds do
  not ask again.

    python3 tools/italia.py

Needs ogr2ogr (GDAL) for reading the shapefiles; the rest is the standard library. Shapes are
simplified by snapping every vertex to a grid of GRID degrees, which keeps the borders that
two places share identical (Lilienstern finds neighbours by shared vertices).
"""
import csv
import io
import json
import math
import os
import subprocess
import tempfile
import time
import urllib.request
import zipfile

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
OUT = os.path.join(ROOT, "games", "italia", "maps")
CACHE = os.path.join(ROOT, "tools", ".cache", "italia")
ELEVATION = os.path.join(ROOT, "tools", "italia-elevation.json")
ISTAT = "https://www.istat.it/storage/cartografia/confini-amministrativi-storici/Limiti_1871.zip"
POPULATION = "https://demopast.it/downloads/1871/data_census_1871_adjusted_age.csv"
CITIES = "https://demopast.it/downloads/citta/data_census_citta.csv"
TOPO = "https://api.opentopodata.org/v1/srtm90m?locations="
AGENT = {"User-Agent": "lilienstern-tools (https://github.com/sirmmo/lilienstern)"}
GRID = 0.004          # degrees, about 400 m
SAMPLES = 16          # elevation samples per place
ATTRIBUTION = ("Boundaries: ISTAT, confini statistico-amministrativi 1871 (CC BY). "
               "Population: 1871 census (ISTAT), via DEMOPAST. Elevation: SRTM via OpenTopoData.")


def fetch(url, name):
    os.makedirs(CACHE, exist_ok=True)
    path = os.path.join(CACHE, name)
    if not os.path.exists(path):
        print("downloading", url)
        with urllib.request.urlopen(urllib.request.Request(url, headers=AGENT), timeout=300) as r:
            data = r.read()
        with open(path, "wb") as f:
            f.write(data)
    return path


def shapes(zip_path, layer):
    """The features of one shapefile in the ISTAT zip, as GeoJSON in WGS84."""
    with tempfile.TemporaryDirectory() as tmp:
        with zipfile.ZipFile(zip_path) as z:
            z.extractall(tmp)
        shp = next(os.path.join(d, f) for d, _, fs in os.walk(tmp) for f in fs if f == layer + ".shp")
        out = os.path.join(tmp, "out.geojson")
        subprocess.run(["ogr2ogr", "-f", "GeoJSON", "-t_srs", "EPSG:4326", "-lco", "COORDINATE_PRECISION=6",
                        out, shp], check=True)
        with open(out) as f:
            return json.load(f)["features"]


def polygons(geom):
    if geom["type"] == "Polygon":
        return [geom["coordinates"]]
    if geom["type"] == "MultiPolygon":
        return geom["coordinates"]
    return []


def snap_ring(ring):
    out = []
    for x, y in ring:
        p = [round(round(x / GRID) * GRID, 4), round(round(y / GRID) * GRID, 4)]
        if not out or out[-1] != p:
            out.append(p)
    if len(out) > 1 and out[0] == out[-1]:
        out.pop()
    # A, B, A: a spike left by snapping; and repeats left by removing one.
    changed = True
    while changed and len(out) >= 3:
        changed = False
        n = len(out)
        for i in range(n):
            if out[(i - 2) % n] == out[i] or out[(i - 1) % n] == out[i]:
                drop = {i} if out[(i - 1) % n] == out[i] else {(i - 1) % n, i}
                for j in sorted(drop, reverse=True):
                    del out[j]
                changed = True
                break
    if len(out) < 3:
        return None
    return out + [out[0]]


def snap(geom):
    polys = []
    for poly in polygons(geom):
        rings = [snap_ring(r) for r in poly]
        if rings and rings[0]:
            polys.append([r for r in rings if r])
    if not polys:
        return None
    return {"type": "Polygon", "coordinates": polys[0]} if len(polys) == 1 else {"type": "MultiPolygon", "coordinates": polys}


KM_LAT = 111.32


def km(lon, lat):
    return lon * KM_LAT * math.cos(math.radians(42.0)), lat * KM_LAT


def ring_area(ring):
    a = 0.0
    for (x1, y1), (x2, y2) in zip(ring, ring[1:]):
        (x1, y1), (x2, y2) = km(x1, y1), km(x2, y2)
        a += x1 * y2 - x2 * y1
    return abs(a) / 2


def area(geom):
    return sum(ring_area(p[0]) - sum(ring_area(h) for h in p[1:]) for p in polygons(geom))


def in_ring(x, y, ring):
    inside = False
    for (x1, y1), (x2, y2) in zip(ring, ring[1:]):
        if (y1 > y) != (y2 > y) and x < x1 + (y - y1) * (x2 - x1) / (y2 - y1):
            inside = not inside
    return inside


def inside(x, y, geom):
    return any(in_ring(x, y, p[0]) and not any(in_ring(x, y, h) for h in p[1:]) for p in polygons(geom))


def samples(geom, n=SAMPLES):
    """About n points inside the shape, on a grid over its bounding box."""
    pts = [c for p in polygons(geom) for c in p[0]]
    x0, x1 = min(p[0] for p in pts), max(p[0] for p in pts)
    y0, y1 = min(p[1] for p in pts), max(p[1] for p in pts)
    k = 3
    found = []
    while k <= 64:
        found = []
        for i in range(k):
            for j in range(k):
                x = x0 + (i + 0.5) * (x1 - x0) / k
                y = y0 + (j + 0.5) * (y1 - y0) / k
                if inside(x, y, geom):
                    found.append((round(x, 4), round(y, 4)))
        if len(found) >= n:
            break
        k *= 2
    if not found:  # a sliver: its first vertex
        found = [tuple(pts[0])]
    step = max(1, len(found) // n)
    return found[::step][:n]


def elevations(points):
    cache = {}
    if os.path.exists(ELEVATION):
        with open(ELEVATION) as f:
            cache = json.load(f)
    want = sorted({f"{y:.4f},{x:.4f}" for x, y in points} - set(cache))
    for i in range(0, len(want), 100):
        batch = want[i:i + 100]
        url = TOPO + "|".join(batch)
        for attempt in range(5):
            try:
                with urllib.request.urlopen(urllib.request.Request(url, headers=AGENT), timeout=60) as r:
                    res = json.load(r)["results"]
                break
            except Exception as e:  # rate limits: wait and try again
                print("elevation:", e, "- retrying")
                time.sleep(5 * (attempt + 1))
        else:
            raise SystemExit("OpenTopoData did not answer; run again later (what was fetched is cached)")
        for key, r in zip(batch, res):
            cache[key] = r["elevation"] if r["elevation"] is not None else 0
        print(f"elevation: {min(i + 100, len(want))}/{len(want)}")
        with open(ELEVATION, "w") as f:
            json.dump(cache, f, sort_keys=True, separators=(",", ":"))
        time.sleep(1.1)
    return cache


def population():
    prov = {}
    with open(fetch(POPULATION, "pop1871.csv"), newline="") as f:
        for r in csv.DictReader(f):
            prov[int(r["code"])] = prov.get(int(r["code"]), 0) + int(r["total"])
    cities = {}
    with open(fetch(CITIES, "citta.csv"), newline="") as f:
        for r in csv.DictReader(f):
            if r["year"] == "1871":
                cities[r["city"]] = cities.get(r["city"], 0) + int(r["total"])
    return prov, cities


def scaled(values, lo=10.0, hi=90.0):
    """Values to lo..hi, between their 5th and 95th percentiles (log scale)."""
    logs = sorted(math.log(max(v, 1e-6)) for v in values)
    a, b = logs[int(0.05 * (len(logs) - 1))], logs[int(0.95 * (len(logs) - 1))]
    return [lo + (hi - lo) * min(1.0, max(0.0, (math.log(max(v, 1e-6)) - a) / max(b - a, 1e-9))) for v in values]


def merge_same_code(features, field):
    """One feature per code: ISTAT draws Catanzaro (1871) apart from the rest of its province
    under the same code. The parts become one MultiPolygon, named after the largest."""
    groups = {}
    for f in features:
        groups.setdefault(f["properties"][field], []).append(f)
    out = []
    for fs in groups.values():
        if len(fs) == 1:
            out.append(fs[0])
            continue
        fs.sort(key=lambda f: -area(f["geometry"]))
        names = [f["properties"].get("DEN_PROV") or f["properties"].get("DEN_CIRC") for f in fs]
        print(f"merged {names} (same ISTAT code)")
        merged = dict(fs[0])
        merged["geometry"] = {"type": "MultiPolygon", "coordinates": [p for f in fs for p in polygons(f["geometry"])]}
        out.append(merged)
    return out


def write(name, features, title):
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, name)
    with open(path, "w") as f:
        json.dump({"type": "FeatureCollection", "name": title, "attribution": ATTRIBUTION, "features": features},
                  f, ensure_ascii=False, separators=(",", ":"))
    print(f"{path}: {len(features)} places, {os.path.getsize(path) / 1e6:.1f} MB")


def main():
    zp = fetch(ISTAT, "Limiti_1871.zip")
    prov_pop, city_pop = population()
    provinces = shapes(zp, "Province_1871")
    circondari = shapes(zp, "Circondari_1871")
    provinces = merge_same_code(provinces, "COD_PROV")
    circondari = merge_same_code(circondari, "COD_CIRC")
    for f in provinces + circondari:
        f["geometry"] = snap(f["geometry"])
    provinces = [f for f in provinces if f["geometry"]]
    circondari = [f for f in circondari if f["geometry"]]
    for f in provinces + circondari:
        f["samples"] = samples(f["geometry"])
    elev = elevations([p for f in provinces + circondari for p in f["samples"]])

    def stats(f):
        hs = [elev[f"{y:.4f},{x:.4f}"] for x, y in f["samples"]]
        return round(sum(hs) / len(hs)), round(max(hs) - min(hs))

    # Provinces.
    mean_density = sum(prov_pop.values()) / sum(area(f["geometry"]) for f in provinces)
    for f in provinces:
        p = f["properties"]
        code, a = int(p["COD_PROV"]), area(f["geometry"])
        pop = prov_pop.get(code)
        if pop is None:
            print("no population for province", p["DEN_PROV"], "- estimated from its area")
            pop = round(mean_density * a)
        f["data"] = {"name": p["DEN_PROV"], "kind": "provincia", "istat": code, "population": pop, "area": a}
    # Circondari: their province is where most of their samples fall.
    for f in circondari:
        votes = {}
        for x, y in f["samples"]:
            for pf in provinces:
                if inside(x, y, pf["geometry"]):
                    votes[pf["data"]["istat"]] = votes.get(pf["data"]["istat"], 0) + 1
                    break
        code = max(votes, key=votes.get) if votes else None
        p = f["properties"]
        f["data"] = {"name": p["DEN_CIRC"], "kind": p["DESC_CIRC"].lower(), "istat": int(p["COD_CIRC"]),
                     "province_code": code, "area": area(f["geometry"])}
    # Each province's people: its big city to the circondario of the same name, the rest by area.
    by_prov = {}
    for f in circondari:
        by_prov.setdefault(f["data"]["province_code"], []).append(f)
    names = {pf["data"]["istat"]: pf["data"] for pf in provinces}
    for code, fs in by_prov.items():
        pd = names.get(code)
        total = pd["population"] if pd else round(mean_density * sum(f["data"]["area"] for f in fs))
        city = {f["data"]["name"]: city_pop[f["data"]["name"]] for f in fs if f["data"]["name"] in city_pop}
        rest = max(0, total - sum(city.values()))
        a = sum(f["data"]["area"] for f in fs)
        for f in fs:
            d = f["data"]
            d["province"] = pd["name"] if pd else None
            d["population"] = round(rest * d["area"] / a) + city.get(d["name"], 0)
    big = set(city_pop)
    for layer in (provinces, circondari):
        dens = [f["data"]["population"] / max(f["data"]["area"], 1e-6) for f in layer]
        wealth = scaled(dens)
        for f, w in zip(layer, wealth):
            d = f["data"]
            h, relief = stats(f)
            has_city = d["name"] in big or (d["kind"] == "provincia" and any(c == d["name"] for c in big))
            f["out"] = {
                "type": "Feature",
                "properties": {
                    "name": d["name"],
                    "kind": d["kind"],
                    "province": d.get("province", d["name"]),
                    "istat": d["istat"],
                    "population": d["population"],
                    "area_km2": round(d["area"], 1),
                    "density": round(d["population"] / max(d["area"], 1e-6), 1),
                    "wealth": round(min(100.0, w + (15 if has_city else 0)), 1),
                    "elevation": h,
                    "relief": relief,
                },
                "geometry": f["geometry"],
            }
    write("italia-1871-province.geojson", [f["out"] for f in provinces], "Italy, provinces of 1871 (ISTAT)")
    write("italia-1871-circondari.geojson", [f["out"] for f in circondari],
          "Italy, circondari and distretti of 1871 (ISTAT)")


if __name__ == "__main__":
    main()
