//! Experiments: a game run under several variants (parameter sets), each over several seeds, in
//! parallel. Every run is deterministic, so an experiment gives the same numbers every time.
//!
//! ```toml
//! name = "Policing strategies"
//! question = "Does hot-spot policing lower violence or move it?"
//! game = "../games/gangland"
//! days = 730
//! seeds = 12
//!
//! [set]                       # parameters for every variant
//! neighborhoods = 900
//!
//! [[variant]]
//! name = "no police"
//! set = { police_budget = 0 }
//!
//! [[variant]]
//! name = "hot spots"
//! set = { police_w_violence = 1.0 }
//! ```
//!
//! Writes `summary.csv` (one row per run), `report.md` (mean and spread per variant),
//! `series/<variant>/seed-N.csv` and, for the first seed of each variant, `maps/<variant>.geojson`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::Deserialize;

use crate::game::Game;
use crate::geojson::to_geojson;
use crate::params::Params;
use crate::world::{fmt_num, Series, World};

fn d_seeds() -> u32 {
    8
}
fn d_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentDef {
    pub name: String,
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub description: String,
    /// The game folder, relative to the experiment file.
    pub game: String,
    #[serde(default)]
    pub days: Option<u32>,
    #[serde(default = "d_seeds")]
    pub seeds: u32,
    #[serde(default)]
    pub seed_base: u64,
    #[serde(default)]
    pub set: toml::Table,
    #[serde(rename = "variant")]
    pub variants: Vec<VariantDef>,
    /// Summary values shown in the report (all the usual ones when empty).
    #[serde(default)]
    pub report: Vec<String>,
    #[serde(default = "d_true")]
    pub geojson: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub set: toml::Table,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub out: PathBuf,
    pub threads: usize,
    pub seeds: Option<u32>,
    pub days: Option<u32>,
    pub progress: bool,
}

#[derive(Clone, Debug)]
pub struct RunResult {
    pub variant: usize,
    pub seed: u64,
    pub summary: BTreeMap<String, f64>,
    pub series: Series,
    pub geojson: Option<String>,
}

pub struct Outcome {
    pub def: ExperimentDef,
    pub games: Vec<Game>,
    pub days: u32,
    pub runs: Vec<RunResult>,
    pub report: String,
}

pub fn slug(s: &str) -> String {
    let s: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    s.split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Loads an experiment and its games (one per variant, with its parameters).
pub fn load(path: &Path) -> Result<(ExperimentDef, Vec<Game>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let def: ExperimentDef =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if def.variants.is_empty() {
        return Err(format!(
            "{}: an experiment needs at least one [[variant]]",
            path.display()
        ));
    }
    let dir = path.parent().unwrap_or(Path::new(".")).join(&def.game);
    let base = Params::from_table(&def.set)?;
    let mut games = Vec::new();
    for v in &def.variants {
        let mut p = base.clone();
        for (k, val) in Params::from_table(&v.set)?.0 {
            p.0.insert(k, val);
        }
        games.push(Game::load(&dir, &p).map_err(|e| format!("variant {:?}: {e}", v.name))?);
    }
    Ok((def, games))
}

pub fn run(path: &Path, opts: &Options) -> Result<Outcome, String> {
    let (def, games) = load(path)?;
    let seeds = opts.seeds.unwrap_or(def.seeds).max(1);
    let days = opts.days.or(def.days).unwrap_or(games[0].days);
    let jobs: Vec<(usize, u64)> = (0..games.len())
        .flat_map(|v| (0..u64::from(seeds)).map(move |s| (v, s)))
        .collect();
    let results: Mutex<Vec<Option<RunResult>>> = Mutex::new(vec![None; jobs.len()]);
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for _ in 0..opts.threads.max(1) {
            sc.spawn(|| loop {
                let j = next.fetch_add(1, Ordering::SeqCst);
                if j >= jobs.len() {
                    break;
                }
                let (v, s) = jobs[j];
                let seed = def.seed_base + s;
                match World::new(&games[v], seed) {
                    Ok(mut w) => {
                        w.run(days);
                        let geojson = (def.geojson && s == 0).then(|| to_geojson(&w).to_string());
                        let r = RunResult {
                            variant: v,
                            seed,
                            summary: w.summary(),
                            series: w.series.clone(),
                            geojson,
                        };
                        results.lock().expect("results")[j] = Some(r);
                    }
                    Err(e) => errors
                        .lock()
                        .expect("errors")
                        .push(format!("{} seed {seed}: {e}", def.variants[v].name)),
                }
                let k = done.fetch_add(1, Ordering::SeqCst) + 1;
                if opts.progress {
                    eprint!("\r{}: {k}/{} runs", def.name, jobs.len());
                }
            });
        }
    });
    if opts.progress {
        eprintln!();
    }
    let errors = errors.into_inner().expect("errors");
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    let runs: Vec<RunResult> = results
        .into_inner()
        .expect("results")
        .into_iter()
        .flatten()
        .collect();
    let report = report(&def, &games, days, seeds, &runs);
    write(&opts.out, &def, &runs, &report)?;
    Ok(Outcome {
        def,
        games,
        days,
        runs,
        report,
    })
}

const DEFAULT_REPORT: [&str; 9] = [
    "violence",
    "clashes",
    "captures",
    "hotspots",
    "contested",
    "gangs_alive",
    "hhi",
    "top_share",
    "held",
];

fn mean_sd(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    if n == 0.0 {
        return (0.0, 0.0);
    }
    let m = xs.iter().sum::<f64>() / n;
    let var = if n > 1.0 {
        xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0)
    } else {
        0.0
    };
    (m, var.sqrt())
}

fn short(v: f64) -> String {
    let a = v.abs();
    if a >= 1000.0 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.3}")
    }
}

pub fn report(
    def: &ExperimentDef,
    games: &[Game],
    days: u32,
    seeds: u32,
    runs: &[RunResult],
) -> String {
    let mut out = format!("# {}\n\n", def.name);
    if !def.question.is_empty() {
        out.push_str(&format!("**Question:** {}\n\n", def.question));
    }
    if !def.description.is_empty() {
        out.push_str(&format!("{}\n\n", def.description.trim()));
    }
    let places: Vec<f64> = runs.iter().map(|r| r.summary["places"]).collect();
    let (pm, _) = mean_sd(&places);
    out.push_str(&format!("Game: {}. {days} days, {seeds} seeds per variant (from seed {}), about {pm:.0} places.\n\n## Variants\n\n", games[0].name, def.seed_base));
    for v in &def.variants {
        let set: Vec<String> = v.set.iter().map(|(k, x)| format!("`{k} = {x}`")).collect();
        out.push_str(&format!(
            "- **{}**{}{}\n",
            v.name,
            if v.description.is_empty() {
                String::new()
            } else {
                format!(": {}", v.description)
            },
            if set.is_empty() {
                String::new()
            } else {
                format!(" ({})", set.join(", "))
            }
        ));
    }
    let keys: Vec<String> = if def.report.is_empty() {
        let mut k: Vec<String> = DEFAULT_REPORT.iter().map(|s| s.to_string()).collect();
        if let Some(r) = runs.first() {
            k.extend(r.summary.keys().filter(|k| k.starts_with("m.")).cloned());
        }
        k
    } else {
        def.report.clone()
    };
    out.push_str("\n## Results\n\nMean ± standard deviation over seeds. Totals are over the whole run; `contested`, `hotspots` and `map.*` are daily means; the others are values on the last day.\n\n");
    out.push_str(&format!(
        "| | {} |\n|---|{}\n",
        def.variants
            .iter()
            .map(|v| v.name.as_str())
            .collect::<Vec<_>>()
            .join(" | "),
        "---:|".repeat(def.variants.len())
    ));
    for k in &keys {
        let cells: Vec<String> = (0..def.variants.len())
            .map(|v| {
                let xs: Vec<f64> = runs
                    .iter()
                    .filter(|r| r.variant == v)
                    .filter_map(|r| r.summary.get(k).copied())
                    .collect();
                if xs.is_empty() {
                    return "–".to_string();
                }
                let (m, sd) = mean_sd(&xs);
                format!("{} ± {}", short(m), short(sd))
            })
            .collect();
        out.push_str(&format!("| `{k}` | {} |\n", cells.join(" | ")));
    }
    out
}

fn write(dir: &Path, def: &ExperimentDef, runs: &[RunResult], report: &str) -> Result<(), String> {
    let io = |e: std::io::Error| format!("{}: {e}", dir.display());
    std::fs::create_dir_all(dir.join("series")).map_err(io)?;
    let keys: BTreeSet<&String> = runs.iter().flat_map(|r| r.summary.keys()).collect();
    let mut csv = format!(
        "variant,seed,{}\n",
        keys.iter()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(",")
    );
    for r in runs {
        let cells: Vec<String> = keys
            .iter()
            .map(|k| r.summary.get(*k).map_or(String::new(), |v| fmt_num(*v)))
            .collect();
        csv.push_str(&format!(
            "{},{},{}\n",
            def.variants[r.variant].name.replace(',', ";"),
            r.seed,
            cells.join(",")
        ));
    }
    std::fs::write(dir.join("summary.csv"), csv).map_err(io)?;
    std::fs::write(dir.join("report.md"), report).map_err(io)?;
    for r in runs {
        let vdir = dir.join("series").join(slug(&def.variants[r.variant].name));
        std::fs::create_dir_all(&vdir).map_err(io)?;
        std::fs::write(vdir.join(format!("seed-{}.csv", r.seed)), r.series.to_csv()).map_err(io)?;
        if let Some(g) = &r.geojson {
            std::fs::create_dir_all(dir.join("maps")).map_err(io)?;
            std::fs::write(
                dir.join("maps")
                    .join(format!("{}.geojson", slug(&def.variants[r.variant].name))),
                g,
            )
            .map_err(io)?;
        }
    }
    Ok(())
}
