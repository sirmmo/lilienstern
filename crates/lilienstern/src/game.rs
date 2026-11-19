//! A game: a folder with `game.toml` (declarations), `params.toml` (its numbers) and scripts.
//!
//! ```toml
//! name = "Gangland"
//! scripts = ["economy.gbx", "politics.lil"]
//! days = 730
//!
//! [city]                      # see CityDef
//! neighborhoods = $neighborhoods
//!
//! [[map]]                     # values per place
//! id = "Wealth"
//! init = "wealth"             # a city field, or a number
//! decay = 0.0
//! diffuse = 0.0
//!
//! [[resource]]
//! id = "Product"
//! scope = "agent"             # local, global, agent
//! reach = 8
//! private = true
//!
//! [[unit]]
//! id = "corner"
//! name = "Street corner"
//!
//! [[faction]]
//! id = "gang"
//! kind = "gang"               # gang, police, other
//! count = $gangs              # gang1, gang2, ...
//! start = { Cash = 100 }
//! start_units = ["safehouse", "corner"]
//!
//! [control]                   # how gangs hold and fight over places
//! hold = 20
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use glassbox::Rules;
use serde::Deserialize;

use crate::city::CityDef;
use crate::params::Params;
use crate::script::{self, Kind, Script};
use crate::war::{TerrainToml, TroopToml, War, WarToml};

fn d_days() -> u32 {
    730
}
fn d_one() -> u32 {
    1
}
fn d_reach() -> u32 {
    6
}
fn d_noise() -> f64 {
    0.2
}
fn d_hq_max() -> f64 {
    f64::INFINITY
}
fn d_spacing() -> f64 {
    0.5
}
fn d_patience() -> u32 {
    20
}
fn d_choice() -> String {
    "weighted".into()
}
fn d_saving() -> String {
    "short".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GameToml {
    name: String,
    #[serde(default)]
    description: String,
    scripts: Vec<String>,
    #[serde(default = "d_days")]
    days: u32,
    #[serde(default)]
    sample_every: Option<u32>,
    city: CityDef,
    #[serde(default, rename = "map")]
    maps: Vec<MapToml>,
    #[serde(default, rename = "resource")]
    resources: Vec<ResourceToml>,
    #[serde(default, rename = "unit")]
    units: Vec<UnitToml>,
    #[serde(rename = "faction")]
    factions: Vec<FactionToml>,
    control: ControlDef,
    #[serde(default)]
    war: WarToml,
    #[serde(default, rename = "terrain")]
    terrains: Vec<TerrainToml>,
    #[serde(default, rename = "troop")]
    troops: Vec<TroopToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MapToml {
    id: String,
    /// A display name, for tools reading the game.
    #[serde(default)]
    #[allow(dead_code)]
    name: Option<String>,
    #[serde(default)]
    decay: f64,
    #[serde(default)]
    diffuse: f64,
    #[serde(default)]
    min: f64,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    init: Option<toml::Value>,
    /// Multiplies the initial value.
    #[serde(default)]
    scale: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceToml {
    id: String,
    scope: String,
    #[serde(default = "d_reach")]
    reach: u32,
    #[serde(default)]
    private: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitToml {
    id: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FactionToml {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    names: Vec<String>,
    kind: String,
    #[serde(default = "d_one")]
    count: u32,
    #[serde(default)]
    colors: Vec<String>,
    #[serde(default)]
    start: toml::Table,
    #[serde(default)]
    income: toml::Table,
    #[serde(default = "d_one")]
    actions: u32,
    #[serde(default = "d_noise")]
    noise: f64,
    #[serde(default = "d_patience")]
    patience: u32,
    #[serde(default = "d_choice")]
    choice: String,
    #[serde(default = "d_saving")]
    saving: String,
    #[serde(default)]
    start_units: Vec<String>,
    #[serde(default)]
    start_presence: f64,
    #[serde(default)]
    start_radius: u32,
    /// Headquarters only where this city field is at most `hq_max`.
    #[serde(default)]
    hq_field: Option<String>,
    #[serde(default = "d_hq_max")]
    hq_max: f64,
    /// How far apart headquarters are, as a share of the largest possible spacing (0 to 1).
    #[serde(default = "d_spacing")]
    hq_spacing: f64,
    /// Factions of the same team are allies. With `count` above 1, a list (or a comma-separated
    /// string) gives each one its own; an empty name is a team of one.
    #[serde(default)]
    team: Option<toml::Value>,
    /// Troops at the headquarters at the start.
    #[serde(default)]
    start_troops: Vec<String>,
    /// Where the headquarters is, by place name ("Milano"; "Urbino|Pesaro e Urbino" tries each
    /// in turn); with `count` above 1, a list or a comma-separated string, one per copy. A name
    /// the map does not have leaves the choice to `hq_field` and `hq_spacing`.
    #[serde(default)]
    capital: Option<toml::Value>,
}

/// How gangs hold places and fight over them, every day:
/// * a gang **holds** a place when its presence is at least `hold` and `margin` times the next
///   gang's, and loses it when its presence falls under half of `hold`;
/// * gangs with more than `contest` presence in the same place **clash**: each loses
///   `attrition` times the others' presence, and the place gets `violence` times the presence
///   of all but the strongest in its violence map;
/// * presence **decays** by `decay`, and by `police_attrition` x the police map / 100.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlDef {
    pub hold: f64,
    pub margin: f64,
    pub decay: f64,
    pub contest: f64,
    pub attrition: f64,
    pub violence: f64,
    #[serde(default = "d_violence_map")]
    pub violence_map: String,
    #[serde(default)]
    pub police_map: Option<String>,
    #[serde(default)]
    pub police_attrition: f64,
    /// What happens to the units of a gang that loses a place: "seize" or "destroy".
    #[serde(default = "d_capture")]
    pub on_capture: String,
    /// A place counts as a hotspot while its violence map is above this.
    #[serde(default = "d_hotspot")]
    pub hotspot: f64,
}

fn d_violence_map() -> String {
    "Violence".into()
}
fn d_capture() -> String {
    "seize".into()
}
fn d_hotspot() -> f64 {
    5.0
}

/// Where a game's files are: a folder, or in memory.
#[derive(Clone, Debug)]
pub enum Files {
    Dir(PathBuf),
    Memory(Arc<BTreeMap<String, String>>),
}

/// `dir/name`, for `/` separated paths (`a/b.json` + `c.json` = `a/c.json`).
pub fn sibling(path: &str, name: &str) -> String {
    match path.rfind('/') {
        Some(i) => format!("{}/{name}", &path[..i]),
        None => name.to_string(),
    }
}

fn normal(name: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for p in name.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

impl Files {
    pub fn read(&self, name: &str) -> Result<String, String> {
        match self {
            Files::Dir(d) => {
                let p = d.join(name);
                std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))
            }
            Files::Memory(m) => m
                .get(&normal(name))
                .cloned()
                .ok_or_else(|| format!("{name}: no such file")),
        }
    }

    pub fn exists(&self, name: &str) -> bool {
        match self {
            Files::Dir(d) => d.join(name).exists(),
            Files::Memory(m) => m.contains_key(&normal(name)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Weighted,
    Best,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MapInit {
    Value(f64),
    Field(String),
}

#[derive(Clone, Debug)]
pub struct FactionDef {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub color: String,
    pub start: Vec<(usize, f64)>,
    pub income: Vec<(usize, f64)>,
    pub actions: u32,
    pub noise: f64,
    /// When the decision the AI wants most is unaffordable, it saves for it for up to this many
    /// decisions in a row instead of taking a cheaper one (0: never saves).
    pub patience: u32,
    /// How the AI picks among its decisions: `Weighted` (at random, in proportion to their
    /// scores) or `Best` (the highest score).
    pub choice: Choice,
    /// While saving, keep saving for the decision first picked (`saving = "goal"`) rather than
    /// for whatever unaffordable decision is picked next (`"short"`, the default). With
    /// `"short"`, a cheap decision picked in between ends the saving, so dear ones are rarely
    /// bought.
    pub save_for_goal: bool,
    pub start_units: Vec<usize>,
    pub start_presence: f64,
    pub start_radius: u32,
    pub hq_field: Option<String>,
    pub hq_max: f64,
    pub hq_spacing: f64,
    /// Factions with the same team start as allies.
    pub team: Option<String>,
    pub start_troops: Vec<usize>,
    /// Names the headquarters may have, tried in turn.
    pub capital: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Game {
    pub name: String,
    pub description: String,
    pub dir: PathBuf,
    /// Where the game's files are read from.
    pub files: Files,
    pub days: u32,
    pub sample_every: u32,
    pub city: CityDef,
    pub params: Params,
    pub rules: Rules,
    /// Initial value of each map, by map index, with its scale.
    pub map_init: Vec<(MapInit, f64)>,
    pub factions: Vec<FactionDef>,
    pub control: ControlDef,
    pub violence_map: usize,
    pub police_map: Option<usize>,
    /// Troops, terrain and how they fight (empty when the game has no troops).
    pub war: War,
    pub script: Script,
    pub warnings: Vec<String>,
}

const PALETTE: [&str; 16] = [
    "#e6194b", "#3cb44b", "#4363d8", "#f58231", "#911eb4", "#46f0f0", "#f032e6", "#bcf60c",
    "#fabebe", "#008080", "#e6beff", "#9a6324", "#800000", "#aaffc3", "#808000", "#000075",
];

fn amounts(rules: &mut Rules, what: &str, t: &toml::Table) -> Result<Vec<(usize, f64)>, String> {
    let mut out = Vec::new();
    for (k, v) in t {
        let x = v
            .as_float()
            .or_else(|| v.as_integer().map(|i| i as f64))
            .ok_or_else(|| format!("{what}.{k}: expected a number"))?;
        out.push((rules.declare_resource(k, glassbox::Scope::Global)?, x));
    }
    Ok(out)
}

/// A value given per copy of a faction: a list, or for several copies a comma-separated
/// string (one name for a single copy).
fn per_copy(
    v: &Option<toml::Value>,
    count: u32,
    id: &str,
    what: &str,
) -> Result<Vec<String>, String> {
    Ok(match v {
        None => Vec::new(),
        Some(toml::Value::String(s)) if count > 1 => {
            s.split(',').map(|t| t.trim().to_string()).collect()
        }
        Some(toml::Value::String(s)) => vec![s.trim().to_string()],
        Some(toml::Value::Array(a)) => a
            .iter()
            .map(|v| v.as_str().map(|s| s.trim().to_string()))
            .collect::<Option<_>>()
            .ok_or_else(|| format!("faction {id}: {what} must be names"))?,
        Some(_) => return Err(format!("faction {id}: {what} must be a name or a list")),
    })
}

/// Merges an included file into the game's table: lists of tables (`[[troop]]`) are appended,
/// tables are merged, and a value set twice is an error.
fn merge(into: &mut toml::Table, from: toml::Table, file: &str) -> Result<(), String> {
    for (k, v) in from {
        match (into.get_mut(&k), v) {
            (None, v) => {
                into.insert(k, v);
            }
            (Some(toml::Value::Array(a)), toml::Value::Array(b)) => a.extend(b),
            (Some(toml::Value::Table(a)), toml::Value::Table(b)) => merge(a, b, file)?,
            (Some(_), _) => return Err(format!("{file}: {k} is already set in game.toml")),
        }
    }
    Ok(())
}

impl Game {
    /// Loads the game in `dir` with its parameters overridden by `overrides`.
    pub fn load(dir: &Path, overrides: &Params) -> Result<Game, String> {
        Game::load_from(Files::Dir(dir.to_path_buf()), overrides)
    }

    /// Loads a game from files in memory (paths relative to the game, `/` separated): for the
    /// browser, where there is no file system.
    pub fn from_files(files: BTreeMap<String, String>, overrides: &Params) -> Result<Game, String> {
        Game::load_from(Files::Memory(Arc::new(files)), overrides)
    }

    fn load_from(files: Files, overrides: &Params) -> Result<Game, String> {
        let read = |name: &str| files.read(name);
        let dir = match &files {
            Files::Dir(d) => d.clone(),
            Files::Memory(_) => PathBuf::from("."),
        };
        let mut params = if files.exists("params.toml") {
            Params::from_toml(&read("params.toml")?).map_err(|e| format!("params.toml: {e}"))?
        } else {
            Params::default()
        };
        params.apply(overrides)?;
        let text = params.substitute("game.toml", &read("game.toml")?)?;
        let mut table: toml::Table =
            toml::from_str(&text).map_err(|e| format!("game.toml: {e}"))?;
        if let Some(inc) = table.remove("include") {
            let list = inc
                .as_array()
                .ok_or("game.toml: include must be a list of files")?
                .clone();
            for f in list {
                let f = f
                    .as_str()
                    .ok_or("game.toml: include must be a list of files")?;
                let text = params.substitute(f, &read(f)?)?;
                let t: toml::Table = toml::from_str(&text).map_err(|e| format!("{f}: {e}"))?;
                merge(&mut table, t, f)?;
            }
        }
        let g: GameToml = toml::Value::Table(table)
            .try_into()
            .map_err(|e| format!("game.toml: {e}"))?;

        let mut rules = Rules::default();
        let mut map_init = Vec::new();
        for m in &g.maps {
            if rules.map(&m.id).is_some() {
                return Err(format!("game.toml: map {} is declared twice", m.id));
            }
            rules.maps.push(glassbox::MapDef {
                id: m.id.clone(),
                decay: m.decay,
                diffuse: m.diffuse,
                min: m.min,
                max: m.max.unwrap_or(f64::INFINITY),
            });
            let init = match &m.init {
                None => MapInit::Value(0.0),
                Some(toml::Value::String(s)) => MapInit::Field(s.clone()),
                Some(v) => MapInit::Value(
                    v.as_float()
                        .or_else(|| v.as_integer().map(|i| i as f64))
                        .ok_or_else(|| {
                            format!("map {}: init must be a field name or a number", m.id)
                        })?,
                ),
            };
            map_init.push((init, m.scale.unwrap_or(1.0)));
        }
        for r in &g.resources {
            let scope = glassbox::Scope::parse(&r.scope).ok_or_else(|| {
                format!("resource {}: scope must be local, global or agent", r.id)
            })?;
            let i = rules.declare_resource(&r.id, scope)?;
            rules.resources[i].reach = r.reach;
            rules.resources[i].private = r.private;
        }
        for u in &g.units {
            rules.declare_unit(&u.id, u.name.as_deref().unwrap_or(&u.id));
        }
        let war = War::build(&mut rules, &g.war, &g.terrains, &g.troops)?;

        let mut factions = Vec::new();
        for f in &g.factions {
            let kind = Kind::parse(&f.kind).ok_or_else(|| {
                format!(
                    "faction {}: kind must be gang, realm, police or other",
                    f.id
                )
            })?;
            let start = amounts(&mut rules, &format!("faction {} start", f.id), &f.start)?;
            let income = amounts(&mut rules, &format!("faction {} income", f.id), &f.income)?;
            let mut start_units = Vec::new();
            for u in &f.start_units {
                start_units.push(
                    rules
                        .unit(u)
                        .ok_or_else(|| format!("faction {}: there is no unit {u:?}", f.id))?,
                );
            }
            let mut start_troops = Vec::new();
            for t in &f.start_troops {
                start_troops.push(
                    war.troop(t)
                        .ok_or_else(|| format!("faction {}: there is no troop {t:?}", f.id))?,
                );
            }
            let capitals = per_copy(&f.capital, f.count, &f.id, "capital")?;
            let teams: Vec<String> = match &f.team {
                None => Vec::new(),
                Some(toml::Value::String(s)) if f.count > 1 => {
                    s.split(',').map(|t| t.trim().to_string()).collect()
                }
                Some(toml::Value::String(s)) => vec![s.trim().to_string()],
                Some(toml::Value::Array(a)) => a
                    .iter()
                    .map(|v| v.as_str().map(|s| s.trim().to_string()))
                    .collect::<Option<_>>()
                    .ok_or_else(|| format!("faction {}: team must be names", f.id))?,
                Some(_) => return Err(format!("faction {}: team must be a name or a list", f.id)),
            };
            for i in 0..f.count {
                let (id, name) = if f.count == 1 {
                    (f.id.clone(), f.name.clone().unwrap_or_else(|| f.id.clone()))
                } else {
                    let base = f.name.clone().unwrap_or_else(|| f.id.clone());
                    (
                        format!("{}{}", f.id, i + 1),
                        f.names
                            .get(i as usize)
                            .cloned()
                            .unwrap_or_else(|| format!("{base} {}", i + 1)),
                    )
                };
                let color = if f.colors.is_empty() {
                    PALETTE[factions.len() % PALETTE.len()].to_string()
                } else {
                    f.colors[i as usize % f.colors.len()].clone()
                };
                factions.push(FactionDef {
                    id,
                    name,
                    kind,
                    color,
                    start: start.clone(),
                    income: income.clone(),
                    actions: f.actions,
                    noise: f.noise,
                    patience: f.patience,
                    choice: match f.choice.as_str() {
                        "weighted" => Choice::Weighted,
                        "best" => Choice::Best,
                        c => {
                            return Err(format!(
                                "faction {}: choice must be \"weighted\" or \"best\", not {c:?}",
                                f.id
                            ))
                        }
                    },
                    save_for_goal: match f.saving.as_str() {
                        "short" => false,
                        "goal" => true,
                        c => {
                            return Err(format!(
                                "faction {}: saving must be \"short\" or \"goal\", not {c:?}",
                                f.id
                            ))
                        }
                    },
                    start_units: start_units.clone(),
                    start_presence: f.start_presence,
                    start_radius: f.start_radius,
                    hq_field: f.hq_field.clone(),
                    hq_max: f.hq_max,
                    hq_spacing: f.hq_spacing.clamp(0.0, 1.0),
                    team: if teams.len() == 1 && f.count == 1 {
                        Some(teams[0].clone())
                    } else {
                        teams.get(i as usize).cloned()
                    }
                    .filter(|t| !t.is_empty()),
                    start_troops: start_troops.clone(),
                    capital: capitals
                        .get(i as usize)
                        .map(|c| {
                            c.split('|')
                                .map(|x| x.trim().to_string())
                                .filter(|x| !x.is_empty())
                                .collect()
                        })
                        .unwrap_or_default(),
                });
            }
        }

        let mut warnings = Vec::new();
        let mut script = Script::default();
        for file in &g.scripts {
            let text = params.substitute(file, &read(file)?)?;
            let (w, rest) = glassbox::compile(&mut rules, file, &text, &script::KINDS)
                .map_err(|e| e.to_string())?;
            warnings.extend(w);
            script::compile(&mut script, &mut rules, &war, file, &rest)
                .map_err(|e| e.to_string())?;
        }
        // Maps the scripts declared start at 0.
        while map_init.len() < rules.maps.len() {
            map_init.push((MapInit::Value(0.0), 1.0));
        }
        let violence_map = rules.declare_map(&g.control.violence_map);
        let police_map = g
            .control
            .police_map
            .as_deref()
            .map(|m| rules.declare_map(m));
        while map_init.len() < rules.maps.len() {
            map_init.push((MapInit::Value(0.0), 1.0));
        }
        if !matches!(g.control.on_capture.as_str(), "seize" | "destroy") {
            return Err("control.on_capture must be \"seize\" or \"destroy\"".into());
        }
        Ok(Game {
            name: g.name,
            description: g.description,
            dir,
            files,
            days: g.days,
            sample_every: g.sample_every.unwrap_or(10).max(1),
            city: g.city,
            params,
            rules,
            map_init,
            factions,
            control: g.control,
            violence_map,
            police_map,
            war,
            script,
            warnings,
        })
    }

    /// A short description of what the game declares.
    pub fn summary(&self) -> String {
        let r = &self.rules;
        let s = &self.script;
        let gangs = self.factions.iter().filter(|f| f.kind.holds()).count();
        format!(
            "{}: {} maps, {} resources, {} units, {} troops, {} terrains, {} unit rules, {} map rules, {} agent rules, {} events, {} decisions, {} modifiers, {} factions ({} hold territory), {} metrics",
            self.name,
            r.maps.len(),
            r.resources.len(),
            r.units.len(),
            self.war.troops.len(),
            self.war.terrains.len(),
            r.unit_rules.len(),
            r.map_rules.len(),
            r.agent_rules.len(),
            s.events.len(),
            s.decisions.len(),
            s.modifiers.len(),
            self.factions.len(),
            gangs,
            r.metrics.len()
        )
    }
}
