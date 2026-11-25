"""Shared helpers for the scripts that build Gangland's city maps from open data (milano.py,
roma.py). Standard library only.

- fetch: downloads once into the download directory, with a generic User-Agent;
- read_sections: ISTAT's 2021 census sections ("basi territoriali", a regional shapefile in a
  zip, WGS84 UTM zone 32N) of one comune, with their attributes;
- utm32_to_lonlat: UTM zone 32N metres to WGS84 degrees (Krüger's series, sub-millimetre);
- Coverage: polygons that share their borders (a coverage), simplified without opening gaps
  between neighbours (shared borders are simplified once, the same way for both sides), and
  dissolved into larger polygons;
- PointIndex: which polygon holds a point;
- wealth_scale: income to the 5..95 wealth scale of Bologna's maps;
- write: a GeoJSON FeatureCollection with an attribution, coordinates rounded to 5 decimals.

Polygons here are lists of rings (lists of (x, y), not closed), the first outer, the others holes.
"""
import json
import math
import os
import struct
import urllib.parse
import urllib.request
import zipfile

USER_AGENT = "Mozilla/5.0 (compatible; open-data-fetch/1.0)"
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "games", "gangland", "maps")


def fetch(d, name, url, form=None, timeout=600):
    """The file `name` in directory `d`, downloaded from `url` (POSTing `form`) if missing."""
    path = os.path.join(d, name)
    if not os.path.exists(path):
        print("downloading", url)
        data = urllib.parse.urlencode(form).encode() if form else None
        req = urllib.request.Request(url, data=data, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(req, timeout=timeout) as r, open(path + ".part", "wb") as f:
            while True:
                chunk = r.read(1 << 20)
                if not chunk:
                    break
                f.write(chunk)
        os.rename(path + ".part", path)
    return path


# --- ISTAT census sections -------------------------------------------------------------------

def _dbf(data):
    n, header, size = struct.unpack("<IHH", data[4:12])
    fields, o = [], 32
    while data[o] != 13:
        fields.append((data[o:o + 11].split(b"\0")[0].decode(), data[o + 16]))
        o += 32
    for i in range(n):
        r, rec, p = data[header + i * size: header + (i + 1) * size], {}, 1
        for name, length in fields:
            rec[name] = r[p:p + length].decode("latin1").strip()
            p += length
        yield rec


def _shape_rings(shp, shx, i):
    off = struct.unpack(">i", shx[100 + 8 * i: 104 + 8 * i])[0] * 2 + 8
    nparts, npoints = struct.unpack("<ii", shp[off + 36: off + 44])
    parts = list(struct.unpack(f"<{nparts}i", shp[off + 44: off + 44 + 4 * nparts])) + [npoints]
    base = off + 44 + 4 * nparts
    xy = struct.unpack(f"<{2 * npoints}d", shp[base: base + 16 * npoints])
    rings = []
    for a, b in zip(parts, parts[1:]):
        ring = [(round(xy[2 * k], 2), round(xy[2 * k + 1], 2)) for k in range(a, b)]
        rings.append(clean(ring))
    return [r for r in rings if len(r) >= 3]


def read_sections(zip_path, pro_com):
    """[(attributes, polygons)] of the census sections of comune `pro_com` (ISTAT code, e.g.
    15146), in UTM 32N metres, from a regional "basi territoriali 2021" zip."""
    z = zipfile.ZipFile(zip_path)
    names = z.namelist()
    pick = lambda ext: next(n for n in names if n.upper().endswith(ext) and "/SHP/" in "/" + n.upper())
    recs = [(i, r) for i, r in enumerate(_dbf(z.read(pick(".DBF")))) if int(r["PRO_COM"]) == pro_com]
    shp, shx = z.read(pick(".SHP")), z.read(pick(".SHX"))
    return [(r, polygons_of(_shape_rings(shp, shx, i))) for i, r in recs]


def read_table(zip_path, suffix):
    """Rows (dicts) of a TAB/*.csv of the zip (UTF-16, tab-separated), e.g. "ASC3_R12_21.csv"."""
    z = zipfile.ZipFile(zip_path)
    name = next(n for n in z.namelist() if n.endswith(suffix))
    lines = z.read(name).decode("utf-16").splitlines()
    head = lines[0].split("\t")
    return [dict(zip(head, l.split("\t"))) for l in lines[1:] if l.strip()]


SECTION_TYPES = {
    1: "residential", 2: "church", 3: "monument", 4: "square", 5: "park", 6: "port", 7: "airport",
    8: "barracks", 9: "hospital", 10: "railway", 12: "industry", 15: "cemetery", 16: "sports",
    18: "university", 19: "camp", 21: "quarry", 22: "woods", 23: "water", 24: "prison", 25: "hotel",
    26: "farmland", 29: "town hall", 30: "power plant", 31: "museum", 33: "waste", 34: "logistics",
    35: "villa", 36: "road", 37: "public service", 50: "water works", 53: "leisure", 55: "market",
    60: "water works", 78: "archaeology", 79: "wasteland", 99: "other",
}


def land_use(code):
    code = int(code or 0)
    return SECTION_TYPES.get(code, "farmland" if 61 <= code <= 69 or code == 81 else "other")


# --- Projection ------------------------------------------------------------------------------

_A, _F = 6378137.0, 1 / 298.257223563
_N = _F / (2 - _F)
_AA = _A / (1 + _N) * (1 + _N ** 2 / 4 + _N ** 4 / 64)
_BETA = [_N / 2 - 2 * _N ** 2 / 3 + 37 * _N ** 3 / 96, _N ** 2 / 48 + _N ** 3 / 15, 17 * _N ** 3 / 480]
_DELTA = [2 * _N - 2 * _N ** 2 / 3 - 2 * _N ** 3, 7 * _N ** 2 / 3 - 8 * _N ** 3 / 5, 56 * _N ** 3 / 15]


def utm32_to_lonlat(x, y, zone=32):
    """WGS84 UTM (northern hemisphere) to (lon, lat) in degrees."""
    xi = y / (0.9996 * _AA)
    eta = (x - 500000) / (0.9996 * _AA)
    xi1, eta1 = xi, eta
    for j, b in enumerate(_BETA, 1):
        xi1 -= b * math.sin(2 * j * xi) * math.cosh(2 * j * eta)
        eta1 -= b * math.cos(2 * j * xi) * math.sinh(2 * j * eta)
    chi = math.asin(math.sin(xi1) / math.cosh(eta1))
    lat = chi + sum(d * math.sin(2 * j * chi) for j, d in enumerate(_DELTA, 1))
    lon = math.radians(zone * 6 - 183) + math.atan2(math.sinh(eta1), math.cos(xi1))
    return math.degrees(lon), math.degrees(lat)


class LocalMetres:
    """lon/lat to metres around a centre and back (equirectangular: fine for one city)."""

    def __init__(self, lon0, lat0):
        self.lon0, self.lat0 = lon0, lat0
        self.kx = 111320.0 * math.cos(math.radians(lat0))
        self.ky = 110574.0

    def fwd(self, p):
        return (round((p[0] - self.lon0) * self.kx, 2), round((p[1] - self.lat0) * self.ky, 2))

    def inv(self, p):
        return (p[0] / self.kx + self.lon0, p[1] / self.ky + self.lat0)


# --- Polygons --------------------------------------------------------------------------------

def clean(ring):
    """A ring without its closing point and without repeated points."""
    out = []
    for p in ring:
        if not out or p != out[-1]:
            out.append(p)
    while len(out) > 1 and out[0] == out[-1]:
        out.pop()
    return out


def signed_area(ring):
    return sum(x1 * y2 - x2 * y1 for (x1, y1), (x2, y2) in zip(ring, ring[1:] + ring[:1])) / 2


def ring_contains(ring, p):
    x, y, inside = p[0], p[1], False
    for (x1, y1), (x2, y2) in zip(ring, ring[1:] + ring[:1]):
        if (y1 > y) != (y2 > y) and x < x1 + (y - y1) * (x2 - x1) / (y2 - y1):
            inside = not inside
    return inside


def polygons_of(rings):
    """Rings (any orientation) grouped into polygons: each outer ring (counter-clockwise) with
    the holes (clockwise) inside it. A ring is a hole when an odd number of rings hold it."""
    rings = [r for r in rings if len(r) >= 3 and abs(signed_area(r)) > 0]
    depth = [sum(1 for j, o in enumerate(rings) if j != i and abs(signed_area(o)) > abs(signed_area(r))
                 and ring_contains(o, r[0])) for i, r in enumerate(rings)]
    polys = []
    for i, r in enumerate(rings):
        if depth[i] % 2 == 0:
            polys.append([r if signed_area(r) > 0 else r[::-1]])
    for i, r in enumerate(rings):
        if depth[i] % 2 == 1:
            hole = r if signed_area(r) < 0 else r[::-1]
            holders = [p for p in polys if ring_contains(p[0], r[0])]
            if holders:  # the smallest one
                min(holders, key=lambda p: abs(signed_area(p[0]))).append(hole)
    return polys


def area_of(polys):
    return sum(signed_area(p[0]) + sum(signed_area(h) for h in p[1:]) for p in polys)


def centroid(polys):
    best = max((p[0] for p in polys), key=lambda r: abs(signed_area(r)))
    a = signed_area(best)
    if abs(a) < 1e-9:
        return best[0]
    cx = sum((x1 + x2) * (x1 * y2 - x2 * y1) for (x1, y1), (x2, y2) in zip(best, best[1:] + best[:1]))
    cy = sum((y1 + y2) * (x1 * y2 - x2 * y1) for (x1, y1), (x2, y2) in zip(best, best[1:] + best[:1]))
    return (cx / (6 * a), cy / (6 * a))


def _dp(pts, tol):
    """Douglas-Peucker on a polyline: the indices kept (always the two ends)."""
    keep = {0, len(pts) - 1}
    stack = [(0, len(pts) - 1)]
    while stack:
        a, b = stack.pop()
        if b - a < 2:
            continue
        (x1, y1), (x2, y2) = pts[a], pts[b]
        dx, dy = x2 - x1, y2 - y1
        n = math.hypot(dx, dy)
        best, bi = -1.0, None
        for i in range(a + 1, b):
            x, y = pts[i]
            d = abs(dy * (x - x1) - dx * (y - y1)) / n if n > 0 else math.hypot(x - x1, y - y1)
            if d > best:
                best, bi = d, i
        if best > tol:
            keep.add(bi)
            stack += [(a, bi), (bi, b)]
    return sorted(keep)


class Coverage:
    """Polygons (lists of polygons each, as `polygons_of` makes them) that share borders."""

    def __init__(self, shapes):
        self.shapes = shapes

    def dissolve(self, groups):
        """The union of the shapes of each group: {group: polygons}. Borders inside a group
        cancel out (they are the same edges walked both ways)."""
        edges = {}
        for shape, g in zip(self.shapes, groups):
            e = edges.setdefault(g, set())
            for poly in shape:
                for ring in poly:
                    for a, b in zip(ring, ring[1:] + ring[:1]):
                        if (b, a) in e:
                            e.discard((b, a))
                        else:
                            e.add((a, b))
        out = {}
        for g, e in edges.items():
            nxt = {}
            for a, b in e:
                nxt.setdefault(a, []).append(b)
            rings = []
            while nxt:
                start = next(iter(nxt))
                ring, p = [start], start
                while True:
                    q = nxt[p].pop()
                    if not nxt[p]:
                        del nxt[p]
                    if q == start:
                        break
                    ring.append(q)
                    p = q
                    if p not in nxt:
                        break
                rings.append(clean(ring))
            out[g] = polygons_of(rings)
        return out

    def simplify(self, tol):
        """Douglas-Peucker with tolerance `tol` on each border between junctions, so that a
        border shared by two shapes stays shared. Rings are never reduced below a triangle."""
        nb = {}
        for shape in self.shapes:
            for poly in shape:
                for ring in poly:
                    n = len(ring)
                    for i, p in enumerate(ring):
                        s = nb.setdefault(p, set())
                        s.add(ring[i - 1])
                        s.add(ring[(i + 1) % n])
        fixed = {p for p, s in nb.items() if len(s) != 2}

        def chains(ring):
            idx = [i for i, p in enumerate(ring) if p in fixed]
            if not idx:  # a ring touching nothing else at a junction: fix two far apart points
                a = min(range(len(ring)), key=lambda i: ring[i])
                b = max(range(len(ring)), key=lambda i: (ring[i][0] - ring[a][0]) ** 2 + (ring[i][1] - ring[a][1]) ** 2)
                idx = sorted({a, b})
            n = len(ring)
            out = []
            for k, i in enumerate(idx):
                j = idx[(k + 1) % len(idx)]
                span = (j - i) % n or n
                out.append([ring[(i + t) % n] for t in range(span + 1)])
            return out

        def key(chain):
            return min(tuple(chain), tuple(chain[::-1]))

        thin = set()  # chains of rings with fewer than three junctions keep a middle point
        for shape in self.shapes:
            for poly in shape:
                for ring in poly:
                    cs = chains(ring)
                    if len(cs) < 3:
                        thin.update(key(c) for c in cs)
        memo = {}

        def simple(chain):
            k = key(chain)
            if k not in memo:
                c = list(k)
                kept = _dp(c, tol)
                if k in thin and len(kept) == 2 and len(c) > 2:
                    x1, y1 = c[0]
                    kept = [0, max(range(1, len(c) - 1), key=lambda i: (c[i][0] - x1) ** 2 + (c[i][1] - y1) ** 2), len(c) - 1]
                memo[k] = [c[i] for i in kept]
            return memo[k] if tuple(chain) == k else memo[k][::-1]

        out = []
        for shape in self.shapes:
            polys = []
            for poly in shape:
                rings = []
                for r, ring in enumerate(poly):
                    pts = []
                    for c in chains(ring):
                        pts += simple(c)[:-1]
                    pts = clean(pts)
                    if len(pts) < 3:
                        pts = ring if r == 0 else None
                    if pts:
                        rings.append(pts)
                polys.append(rings)
            out.append(polys)
        return Coverage(out)


class PointIndex:
    """Finds the shape holding a point (a grid of the shapes' bounding boxes)."""

    def __init__(self, shapes, cell=500.0):
        self.shapes, self.cell, self.grid = shapes, cell, {}
        for i, shape in enumerate(shapes):
            pts = [p for poly in shape for p in poly[0]]
            if not pts:
                continue
            x0, y0 = min(p[0] for p in pts), min(p[1] for p in pts)
            x1, y1 = max(p[0] for p in pts), max(p[1] for p in pts)
            for gx in range(int(x0 // cell), int(x1 // cell) + 1):
                for gy in range(int(y0 // cell), int(y1 // cell) + 1):
                    self.grid.setdefault((gx, gy), []).append(i)

    def find(self, p):
        for i in self.grid.get((int(p[0] // self.cell), int(p[1] // self.cell)), []):
            for poly in self.shapes[i]:
                if ring_contains(poly[0], p) and not any(ring_contains(h, p) for h in poly[1:]):
                    return i
        return None


# --- Output ----------------------------------------------------------------------------------

def wealth_scale(incomes):
    """The wealth of an income: 5 at the 5th percentile of `incomes`, 95 at the 95th, linear,
    clamped to 2..100 (as Bologna's maps)."""
    incs = sorted(incomes)
    lo, hi = incs[len(incs) * 5 // 100], incs[len(incs) * 95 // 100]
    return lambda inc: round(min(100.0, max(2.0, 5 + 90 * (inc - lo) / (hi - lo))), 1)


def geometry(polys, to_lonlat):
    """A GeoJSON Polygon or MultiPolygon, coordinates in lon/lat rounded to 5 decimals."""
    def ring(r):
        pts = clean([tuple(round(v, 5) for v in to_lonlat(p)) for p in r])
        return [list(p) for p in pts + pts[:1]] if len(pts) >= 3 else None
    out = []
    for poly in polys:
        rings = [ring(r) for r in poly]
        if rings and rings[0]:
            out.append([r for r in rings if r])
    if len(out) == 1:
        return {"type": "Polygon", "coordinates": out[0]}
    return {"type": "MultiPolygon", "coordinates": out}


def write(name, features, attribution, links=None):
    """Writes the map; `links` ([[id, id], ...]) are extra borders between places by their `id`
    (bridges, tunnels, ferries), which Lilienstern adds to the shared-vertex ones."""
    path = os.path.join(OUT, name)
    doc = {"type": "FeatureCollection", "attribution": attribution}
    if links:
        doc["links"] = links
    doc["features"] = features
    with open(path, "w") as f:
        json.dump(doc, f, separators=(",", ":"), ensure_ascii=False)
    print(f"{name}: {len(features)} places, {sum(x['properties']['population'] for x in features):,} residents, "
          f"{os.path.getsize(path) / 1e6:.1f} MB")


# --- Shared steps of the city scripts --------------------------------------------------------

def to_local(sections, centre):
    """Sections' polygons from UTM 32N to metres around `centre` (lon, lat), and the projection."""
    lm = LocalMetres(*centre)
    memo = {}

    def p(v):
        if v not in memo:
            memo[v] = lm.fwd(utm32_to_lonlat(*v))
        return memo[v]
    return [(r, [[[p(v) for v in ring] for ring in poly] for poly in polys]) for r, polys in sections], lm


def keyhole(polys):
    """Each hole joined to its outer ring by a slit (a keyhole polygon): the same surface, but the
    places inside a hole now share vertices with the outer ring, which is what Lilienstern
    looks at to find neighbours."""
    out = []
    for poly in polys:
        outer = list(poly[0])
        for hole in poly[1:]:
            i, j = min(((i, j) for i in range(len(outer)) for j in range(len(hole))),
                       key=lambda ij: (outer[ij[0]][0] - hole[ij[1]][0]) ** 2 + (outer[ij[0]][1] - hole[ij[1]][1]) ** 2)
            outer = clean(outer[:i + 1] + hole[j:] + hole[:j + 1] + outer[i:])
        out.append([outer])
    return out


def neighbours(shapes):
    """Neighbour sets of shapes sharing at least two vertices (Lilienstern's rule)."""
    by_v = {}
    for i, shape in enumerate(shapes):
        for poly in shape:
            for ring in poly:
                for v in ring:
                    by_v.setdefault(v, set()).add(i)
    shared = {}
    for ids in by_v.values():
        if len(ids) > 1:
            ids = sorted(ids)
            for a in range(len(ids)):
                for b in range(a + 1, len(ids)):
                    shared[(ids[a], ids[b])] = shared.get((ids[a], ids[b]), 0) + 1
    nb = [set() for _ in shapes]
    for (a, b), c in shared.items():
        if c >= 2:
            nb[a].add(b)
            nb[b].add(a)
    return nb


def components(nb):
    """Connected groups of indices, largest first."""
    seen, comps = set(), []
    for i in range(len(nb)):
        if i in seen:
            continue
        stack, comp = [i], []
        seen.add(i)
        while stack:
            x = stack.pop()
            comp.append(x)
            for y in nb[x]:
                if y not in seen:
                    seen.add(y)
                    stack.append(y)
        comps.append(comp)
    return sorted(comps, key=len, reverse=True)


def drop_rivers(sections):
    """The sections without those of the main rivers (ISTAT COD_ACQUE 2xxxx), so that a river is
    a gap crossed at its bridges; river sections are kept where leaving them out would cut a
    group of places off the city (an island)."""
    wet = [i for i, (r, _) in enumerate(sections) if r["COD_ACQUE"].startswith("2")]
    out = set(wet)
    while True:
        keep = [i for i in range(len(sections)) if i not in out]
        nb = neighbours([keyhole(sections[i][1]) for i in keep])
        comps = components(nb)
        if len(comps) == 1:
            break
        cut = {keep[i] for c in comps[1:] for i in c}
        allnb = neighbours([keyhole(sections[i][1]) for i in range(len(sections))])
        back = {w for w in out if allnb[w] & cut}
        if not back:
            break
        out -= back
    print(f"rivers: {len(out)} of {len(wet)} river sections left out")
    return [s for i, s in enumerate(sections) if i not in out]


def read_points(path):
    """(lat, lon, postcode) rows of an Overpass CSV export."""
    rows = []
    with open(path) as f:
        for line in f:
            parts = line.strip().split(";")
            if len(parts) == 3 and parts[0] and parts[2]:
                try:
                    rows.append((float(parts[0]), float(parts[1]), parts[2].strip()))
                except ValueError:
                    pass
    return rows


OVERPASS = ["https://overpass-api.de/api/interpreter", "https://maps.mail.ru/osm/tools/overpass/api/interpreter",
            "https://overpass.private.coffee/api/interpreter"]


def fetch_postcodes(d, istat_code):
    """OpenStreetMap's addresses with a postcode in a comune (ODbL), from the Overpass API:
    lat;lon;postcode."""
    import time
    name = f"osm-postcodes-{istat_code}.csv"
    path = os.path.join(d, name)
    query = (f'[out:csv(::lat,::lon,"addr:postcode";false;";")][timeout:250];'
             f'area["ref:ISTAT"="{istat_code}"]["boundary"="administrative"]->.a;nwr(area.a)["addr:postcode"];out center;')
    for attempt in range(6):
        if os.path.exists(path) and len(read_points(path)) > 1000:
            return path
        if os.path.exists(path):
            os.remove(path)
        try:
            fetch(d, name, OVERPASS[attempt % len(OVERPASS)], {"data": query}, timeout=300)
        except OSError as e:  # busy, refused, timed out: the next server
            print("overpass:", e)
            if os.path.exists(path + ".part"):
                os.remove(path + ".part")
        time.sleep(5 * attempt)
    raise SystemExit("could not download the postcodes from the Overpass API: try again later")


def assign(shapes, points, nb):
    """A value for every shape: the most common value of the points (x, y, value) inside it,
    else the most common among its neighbours' (spreading outwards)."""
    index = PointIndex(shapes)
    votes = [{} for _ in shapes]
    for x, y, v in points:
        i = index.find((x, y))
        if i is not None:
            votes[i][v] = votes[i].get(v, 0) + 1
    val = [max(sorted(c), key=c.get) if c else None for c in votes]
    found = sum(1 for v in val if v)
    while any(v is None for v in val):
        new = list(val)
        for i, v in enumerate(val):
            if v is None:
                c = {}
                for j in nb[i]:
                    if val[j] is not None:
                        c[val[j]] = c.get(val[j], 0) + 1
                if c:
                    new[i] = max(sorted(c), key=c.get)
        if new == val:  # cut off: the value of the nearest shape that has one
            cs = [centroid(s) for s in shapes]
            for i, v in enumerate(val):
                if v is None:
                    j = min((j for j in range(len(val)) if val[j] is not None),
                            key=lambda j: (cs[i][0] - cs[j][0]) ** 2 + (cs[i][1] - cs[j][1]) ** 2)
                    new[i] = val[j]
        val = new
    return val, found


def mef_income(rows):
    """Mean taxable income per taxpayer of each postcode, from rows of the Ministry of Economy
    and Finance's "redditi e principali variabili IRPEF su base subcomunale" (one dict per
    postcode, with the columns "Reddito imponibile - Ammontare in euro" and "- Frequenza")."""
    out = {}
    for r in rows:
        amount, n = float(r["Reddito imponibile - Ammontare in euro"] or 0), float(r["Reddito imponibile - Frequenza"] or 0)
        if n > 0:
            out[r["CAP"].strip().zfill(5)] = round(amount / n)
    return out


def check(features, links=()):
    """Counts the groups of places as Lilienstern will see them (outer rings of the written
    coordinates, two shared vertices, and the map's `links`): a map must be one group, or places
    get dropped."""
    shapes = []
    for f in features:
        g = f["geometry"]
        polys = [g["coordinates"]] if g["type"] == "Polygon" else g["coordinates"]
        shapes.append([[[tuple(round(c * 1e6) for c in p) for p in poly[0]]] for poly in polys])
    nb = neighbours(shapes)
    index = {f["properties"]["id"]: i for i, f in enumerate(features)}
    for a, b in links:
        nb[index[a]].add(index[b])
        nb[index[b]].add(index[a])
    comps = components(nb)
    lonely = sum(1 for c in comps if len(c) == 1)
    print(f"  {len(comps[0])} places joined; {len(comps) - 1 - lonely} cut-off groups ({sum(len(c) for c in comps[1:]) - lonely} places), "
          f"{lonely} places with no neighbour (joined to their nearest two)")
    return comps


def group(shapes, zone, pop, nb, min_pop, min_area_km2):
    """Groups neighbouring shapes of the same zone into blocks of at least `min_pop` residents or
    `min_area_km2`, merging the smallest block into its smallest neighbour first; a group of
    shapes cut off from the rest is one block. Returns the block of each shape (an index)."""
    import heapq
    area = [abs(area_of(s)) / 1e6 for s in shapes]
    members = {i: [i] for i in range(len(shapes))}
    bpop, barea = dict(enumerate(pop)), dict(enumerate(area))
    bnb = {i: {j for j in nb[i] if zone[j] == zone[i]} for i in range(len(shapes))}
    size = lambda b: max(bpop[b] / min_pop, barea[b] / min_area_km2)
    heap = [(size(b), b) for b in members if size(b) < 1]
    heapq.heapify(heap)
    while heap:
        s, b = heapq.heappop(heap)
        if b not in members or s != size(b) or not bnb[b]:
            continue
        o = min(bnb[b], key=lambda x: (size(x), x))
        keep, gone = min(b, o), max(b, o)
        members[keep] += members.pop(gone)
        bpop[keep] += bpop.pop(gone)
        barea[keep] += barea.pop(gone)
        merged = (bnb.pop(gone) | bnb[keep]) - {keep, gone}
        bnb[keep] = merged
        for x in merged:
            bnb[x].discard(gone)
            bnb[x].add(keep)
        if size(keep) < 1:
            heapq.heappush(heap, (size(keep), keep))
    block = [0] * len(shapes)
    for b, ms in members.items():
        for i in ms:
            block[i] = b
    for comp in components(nb)[1:]:  # a group cut off from the city (an exclave) is one block
        for i in comp:
            block[i] = min(comp)
    return block
