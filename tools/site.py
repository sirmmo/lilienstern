#!/usr/bin/env python3
"""Builds Lilienstern's web site: the documentation and the playable games (Gangland: Bologna,
Milano and Roma, Marches, Italia).

    cargo build --release --target wasm32-unknown-unknown -p lilienstern-web
    python3 tools/site.py [--out site] [--wasm target/wasm32-unknown-unknown/release/lilienstern_web.wasm]

Standard library only. Each page embeds its Markdown, rendered in the browser by marked.
"""
import argparse
import html
import os
import re
import shutil

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
REPO = "https://github.com/sirmmo/lilienstern"

PAGES = [
    # (source, output, title in the menu)
    ("README.md", "index.html", "Lilienstern"),
    ("docs/DESIGN.md", "design.html", "Design"),
    ("docs/SCRIPTING.md", "scripting.html", "Writing a game"),
    ("docs/EXPERIMENTS.md", "experiments.html", "Experiments"),
    ("games/gangland/maps/README.md", "maps.html", "Maps"),
    ("games/italia/maps/README.md", "italia-maps.html", "Italia's maps"),
]
LINKS = {src: out for src, out, _ in PAGES}
# Gangland's cities: the page (web/gangland-<city>) and its maps (games/gangland/maps/<city>-*).
GANGLAND = [("bologna", "Bologna"), ("milano", "Milano"), ("roma", "Roma")]

TEMPLATE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="Lilienstern: a deeply configurable grand-strategy simulation engine in Rust, with GlassBox rules.">
<script src="https://cdn.jsdelivr.net/npm/marked@12.0.2/marked.min.js"></script>
<style>
  :root {{ --ink: #1d1b22; --muted: #6b6676; --line: #e4e0ea; --accent: #b3261e; }}
  body {{ margin: 0; font: 16px/1.6 system-ui, -apple-system, Segoe UI, sans-serif; color: var(--ink); background: #fbfaf8; }}
  header {{ background: #1d1b22; color: #fff; }}
  nav {{ max-width: 900px; margin: 0 auto; padding: 10px 20px; display: flex; flex-wrap: wrap; gap: 6px 18px; align-items: baseline; }}
  nav a {{ color: #ddd; text-decoration: none; }}
  nav a:hover, nav a.on {{ color: #fff; text-decoration: underline; }}
  nav .brand {{ font-weight: 700; color: #fff; font-size: 18px; margin-right: 8px; }}
  nav .play {{ background: var(--accent); color: #fff; padding: 3px 12px; border-radius: 6px; }}
  nav .push {{ margin-left: auto; }}
  main {{ max-width: 900px; margin: 0 auto; padding: 10px 20px 60px; }}
  h1, h2, h3 {{ line-height: 1.25; }}
  h1 {{ font-size: 34px; }}
  a {{ color: #1f5fbf; }}
  img {{ max-width: 100%; border-radius: 8px; border: 1px solid var(--line); }}
  code {{ background: #f0edf3; padding: 1px 5px; border-radius: 4px; font-size: 90%; }}
  pre {{ background: #1d1b22; color: #eee; padding: 12px 14px; border-radius: 8px; overflow-x: auto; }}
  pre code {{ background: none; padding: 0; font-size: 13px; }}
  table {{ border-collapse: collapse; margin: 12px 0; display: block; overflow-x: auto; }}
  th, td {{ border: 1px solid var(--line); padding: 5px 9px; text-align: left; vertical-align: top; }}
  th {{ background: #f3f0f6; }}
  footer {{ max-width: 900px; margin: 0 auto; padding: 0 20px 30px; color: var(--muted); font-size: 13px; }}
</style>
</head>
<body>
<header><nav>{nav}</nav></header>
<main id="doc"><noscript><pre>{escaped}</pre></noscript></main>
<footer>Lilienstern is free software (GPL-3.0-or-later). <a href="{repo}">Source on GitHub</a> · <a href="{repo}/blob/main/{src}">this page</a></footer>
<textarea id="md" hidden>{escaped}</textarea>
<script>
  document.getElementById('doc').innerHTML = marked.parse(document.getElementById('md').value);
</script>
</body>
</html>
"""


def rewrite_links(md, src):
    """Links to other documents point to their pages; links to other repository files to GitHub."""
    base = os.path.dirname(src)

    def fix(m):
        text, target = m.group(1), m.group(2)
        if re.match(r"^[a-z]+://|^#|^mailto:", target):
            return m.group(0)
        path, _, anchor = target.partition("#")
        full = os.path.normpath(os.path.join(base, path)).replace(os.sep, "/")
        if full in LINKS:
            return f"[{text}]({LINKS[full]}{'#' + anchor if anchor else ''})"
        if full.endswith(".png"):
            return f"[{text}]({os.path.basename(full)})"
        kind = "tree" if os.path.isdir(os.path.join(ROOT, full)) else "blob"
        return f"[{text}]({REPO}/{kind}/main/{full})"

    return re.sub(r"\[([^\]]*)\]\(([^)\s]+)\)", fix, md)


def nav(current):
    items = [f'<a class="brand" href="index.html">Lilienstern</a>']
    for _, out, title in PAGES[1:]:
        items.append(f'<a href="{out}"{" class=on" if out == current else ""}>{title}</a>')
    items.append(f'<a href="{REPO}">GitHub</a>')
    for k, (city, name) in enumerate(GANGLAND):
        items.append(f'<a class="play{" push" if k == 0 else ""}" href="gangland-{city}/">Play Gangland: {name}</a>')
    items.append('<a class="play" href="marches/">Play Marches</a>')
    items.append('<a class="play" href="italia/">Play Italia</a>')
    return "".join(items)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=os.path.join(ROOT, "site"))
    ap.add_argument("--wasm", default=os.path.join(ROOT, "target", "wasm32-unknown-unknown", "release", "lilienstern_web.wasm"))
    a = ap.parse_args()
    out = a.out
    if os.path.exists(out):
        shutil.rmtree(out)
    os.makedirs(out)

    for src, page, title in PAGES:
        with open(os.path.join(ROOT, src)) as f:
            md = rewrite_links(f.read(), src)
        h1 = re.search(r"^# (.+)$", md, re.M)
        full_title = title if page == "index.html" else f"{h1.group(1) if h1 else title} · Lilienstern"
        with open(os.path.join(out, page), "w") as f:
            f.write(TEMPLATE.format(title=html.escape(full_title), nav=nav(page), escaped=html.escape(md, quote=False), repo=REPO, src=src))
    shutil.copy(os.path.join(ROOT, "docs", "gangland.png"), out)

    # Gangland is the Marches page (app.js, net.js, app.css, icons) with its own start screen and
    # its crews' portraits (web/gangland-bologna/icons, for every city), one page per city.
    if not os.path.exists(a.wasm):
        raise SystemExit(f"{a.wasm} is missing: build it with cargo build --release --target wasm32-unknown-unknown -p lilienstern-web")
    for city, _ in GANGLAND:
        play = os.path.join(out, f"gangland-{city}")
        shutil.copytree(os.path.join(ROOT, "web", "marches"), play, ignore=shutil.ignore_patterns("index.html"))
        shutil.copy(os.path.join(ROOT, "web", f"gangland-{city}", "index.html"), play)
        shutil.copytree(os.path.join(ROOT, "web", "gangland-bologna", "icons"), os.path.join(play, "icons"), dirs_exist_ok=True)
        os.rename(os.path.join(play, "icons", "README.md"), os.path.join(play, "icons", "README-gangland.md"))
        shutil.copy(os.path.join(ROOT, "web", "marches", "icons", "README.md"), os.path.join(play, "icons", "README.md"))
        shutil.copy(a.wasm, os.path.join(play, "lilienstern.wasm"))
        game = os.path.join(play, "game")
        os.makedirs(os.path.join(game, "maps"))
        for f in ["params.toml", "game.toml", "economy.gbx", "politics.lil", "crews.toml", "crews.lil"]:
            shutil.copy(os.path.join(ROOT, "games", "gangland", f), game)
        for f in [f"{city}-areas.geojson", f"{city}-sections.geojson"]:
            shutil.copy(os.path.join(ROOT, "games", "gangland", "maps", f), os.path.join(game, "maps"))

    play = os.path.join(out, "marches")
    shutil.copytree(os.path.join(ROOT, "web", "marches"), play)
    shutil.copy(a.wasm, os.path.join(play, "lilienstern.wasm"))
    game = os.path.join(play, "game")
    os.makedirs(os.path.join(game, "eras"))
    for f in ["params.toml", "game.toml", "realms.lil", "people.lil", "eras/medieval.toml", "eras/medieval.lil", "eras/future.toml"]:
        shutil.copy(os.path.join(ROOT, "games", "marches", f), os.path.join(game, f))

    # Italia: the Marches page with Italia's start screen, its files and Marches' shared ones.
    play = os.path.join(out, "italia")
    shutil.copytree(os.path.join(ROOT, "web", "marches"), play, ignore=shutil.ignore_patterns("index.html"))
    shutil.copy(os.path.join(ROOT, "web", "italia", "index.html"), play)
    shutil.copytree(os.path.join(ROOT, "web", "italia", "crests"), os.path.join(play, "crests"))
    shutil.copy(a.wasm, os.path.join(play, "lilienstern.wasm"))
    game = os.path.join(play, "game")
    for f in ["params.toml", "game.toml", "italia.lil", "maps/italia-1871-circondari.geojson", "maps/italia-1871-province.geojson",
              "maps/italia-cities.geojson"]:
        os.makedirs(os.path.dirname(os.path.join(game, f)), exist_ok=True)
        shutil.copy(os.path.join(ROOT, "games", "italia", f), os.path.join(game, f))
    for f in ["realms.lil", "people.lil", "eras/medieval.toml", "eras/medieval.lil"]:
        os.makedirs(os.path.dirname(os.path.join(game, "marches", f)), exist_ok=True)
        shutil.copy(os.path.join(ROOT, "games", "marches", f), os.path.join(game, "marches", f))
    with open(os.path.join(out, ".nojekyll"), "w"):
        pass
    total = sum(os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(out) for f in fs)
    print(f"site written to {out} ({total / 1e6:.1f} MB)")


if __name__ == "__main__":
    main()
