//! `lilienstern`: run games and experiments.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lilienstern::experiment::{self, Options};
use lilienstern::geojson::to_geojson;
use lilienstern::viewer;
use lilienstern::{Game, Params, World};

const USAGE: &str = "\
lilienstern: a configurable grand-strategy simulation engine

USAGE:
    lilienstern check <game-dir> [--set key=value]...
    lilienstern run <game-dir> [--seed N] [--days N] [--set key=value]...
                    [--csv FILE] [--geojson FILE] [--map DIR] [--quiet]
    lilienstern experiment <file.toml> [--out DIR] [--threads N] [--seeds N] [--days N]
    lilienstern city <game-dir> [--seed N] [--set key=value]... [--geojson FILE] [--map DIR]

check       loads a game and lists what it declares
run         plays one game and prints how it went
experiment  plays every variant of an experiment over its seeds and writes a report
city        writes the game's map, before anything happens

--map DIR writes a map viewer: DIR/index.html (open it in a browser to replay the game over
the basemap), DIR/style.json (a MapLibre style) and DIR/map.geojson.
";

struct Args {
    positional: Vec<String>,
    set: Params,
    seed: u64,
    days: Option<u32>,
    seeds: Option<u32>,
    threads: Option<usize>,
    csv: Option<PathBuf>,
    geojson: Option<PathBuf>,
    map: Option<PathBuf>,
    out: Option<PathBuf>,
    quiet: bool,
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args {
        positional: Vec::new(),
        set: Params::default(),
        seed: 1,
        days: None,
        seeds: None,
        threads: None,
        csv: None,
        geojson: None,
        map: None,
        out: None,
        quiet: false,
    };
    let mut it = argv.iter();
    while let Some(x) = it.next() {
        let mut val = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match x.as_str() {
            "--set" | "-s" => {
                let (k, v) = Params::parse_assignment(&val("--set")?)?;
                a.set.0.insert(k, v);
            }
            "--seed" => a.seed = val("--seed")?.parse().map_err(|_| "--seed: not a number")?,
            "--days" => a.days = Some(val("--days")?.parse().map_err(|_| "--days: not a number")?),
            "--seeds" => {
                a.seeds = Some(
                    val("--seeds")?
                        .parse()
                        .map_err(|_| "--seeds: not a number")?,
                )
            }
            "--threads" => {
                a.threads = Some(
                    val("--threads")?
                        .parse()
                        .map_err(|_| "--threads: not a number")?,
                )
            }
            "--csv" => a.csv = Some(val("--csv")?.into()),
            "--geojson" => a.geojson = Some(val("--geojson")?.into()),
            "--out" => a.out = Some(val("--out")?.into()),
            "--map" => a.map = Some(val("--map")?.into()),
            "--quiet" | "-q" => a.quiet = true,
            "--help" | "-h" => return Err(String::new()),
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s => a.positional.push(s.to_string()),
        }
    }
    Ok(a)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn game(a: &Args) -> Result<Game, String> {
    let dir = a.positional.get(1).ok_or("which game? give its folder")?;
    let g = Game::load(Path::new(dir), &a.set)?;
    for w in &g.warnings {
        eprintln!("warning: {w}");
    }
    Ok(g)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let a = match parse(&argv) {
        Ok(a) if !a.positional.is_empty() => a,
        Ok(_) | Err(_) if argv.is_empty() || argv.iter().any(|x| x == "-h" || x == "--help") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(_) => {
            eprint!("{USAGE}");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match command(&a) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn command(a: &Args) -> Result<(), String> {
    match a.positional[0].as_str() {
        "check" => {
            let g = game(a)?;
            println!("{}", g.summary());
            println!(
                "parameters: {}",
                g.params
                    .0
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            println!("metrics: {}", g.rules.metrics.join(", "));
            let decisions: Vec<&str> = g.script.decisions.iter().map(|d| d.name.as_str()).collect();
            println!("decisions: {}", decisions.join(", "));
            let w = World::new(&g, a.seed)?;
            println!("map: {} places", w.city.len());
            Ok(())
        }
        "city" => {
            let g = game(a)?;
            let mut w = World::new(&g, a.seed)?;
            if a.geojson.is_none() && a.map.is_none() {
                return Err("give --geojson FILE or --map DIR".into());
            }
            if let Some(path) = &a.geojson {
                write(path, &to_geojson(&w).to_string())?;
                println!("{} places written to {}", w.city.len(), path.display());
            }
            if let Some(dir) = &a.map {
                w.keep_history();
                viewer::write(dir, &w)?;
                println!("map viewer written to {}", dir.join("index.html").display());
            }
            Ok(())
        }
        "run" => {
            let g = game(a)?;
            let mut w = World::new(&g, a.seed)?;
            if a.map.is_some() {
                w.keep_history();
            }
            let days = a.days.unwrap_or(g.days);
            let step = (days / 10).max(1);
            if !a.quiet {
                println!(
                    "{} on {} places, seed {}, {days} days",
                    g.name,
                    w.city.len(),
                    a.seed
                );
                println!(
                    "{:>5} {:>9} {:>7} {:>8} {:>6} {:>5} {:>6}  territory",
                    "day", "violence", "clashes", "captures", "gangs", "hhi", "held"
                );
            }
            let mut last = (0.0, 0.0, 0.0);
            for d in 1..=days {
                w.step();
                if !a.quiet && (d % step == 0 || d == days) {
                    let s = w.summary();
                    let terr: Vec<String> = w
                        .gangs
                        .iter()
                        .filter(|&&h| w.factions[h].territory > 0)
                        .map(|&h| format!("{}:{}", g.factions[h].id, w.factions[h].territory))
                        .collect();
                    println!(
                        "{d:>5} {:>9.1} {:>7} {:>8} {:>6} {:>5.2} {:>5.0}%  {}",
                        s["violence"] - last.0,
                        s["clashes"] - last.1,
                        s["captures"] - last.2,
                        w.gangs_alive(),
                        w.hhi(),
                        100.0 * w.held(),
                        terr.join(" ")
                    );
                    last = (s["violence"], s["clashes"], s["captures"]);
                }
            }
            let s = w.summary();
            println!("\nsummary");
            for (k, v) in &s {
                println!("  {k:<28} {}", lilienstern::world::fmt_num(*v));
            }
            if let Some(p) = &a.csv {
                write(p, &w.series.to_csv())?;
                println!("series written to {}", p.display());
            }
            if let Some(p) = &a.geojson {
                write(p, &to_geojson(&w).to_string())?;
                println!("map written to {}", p.display());
            }
            if let Some(dir) = &a.map {
                viewer::write(dir, &w)?;
                println!("map viewer written to {}", dir.join("index.html").display());
            }
            Ok(())
        }
        "experiment" => {
            let file = PathBuf::from(
                a.positional
                    .get(1)
                    .ok_or("which experiment? give its .toml file")?,
            );
            let stem = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("experiment")
                .to_string();
            let out = a
                .out
                .clone()
                .unwrap_or_else(|| PathBuf::from("out").join(stem));
            let threads = a
                .threads
                .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()));
            let o = experiment::run(
                &file,
                &Options {
                    out: out.clone(),
                    threads,
                    seeds: a.seeds,
                    days: a.days,
                    progress: !a.quiet,
                },
            )?;
            println!("{}", o.report);
            println!("written to {}", out.display());
            Ok(())
        }
        c => Err(format!("unknown command {c:?}")),
    }
}
