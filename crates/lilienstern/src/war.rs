//! Troops: what armies are made of, the ground they cross and how they fight. All of it is
//! data in `game.toml` (or a file it includes), so the same engine runs levies and knights or
//! militia and hovertanks.
//!
//! ```toml
//! [war]
//! distance = "km"             # "steps" (every border crossed costs 1) or "km" (centre to centre)
//! damage_scale = 0.4          # share of the damage table dealt per day of battle
//! defense_per_star = 10       # % of damage removed per defence star, scaled by the defender's health
//! home_defense = 1            # extra stars for troops in places their faction (or an ally) holds
//! fort_map = "Fort"           # a map whose value adds stars
//! luck = 10                   # a random 0..luck added to each hit, in table points
//! heal = 4                    # health (of 100) regained per day in friendly places
//! unpaid = 5                  # health lost per day when the upkeep cannot be paid
//! violence = 0.2              # added to the violence map per point of damage
//! relation = "war"            # between factions of different teams at the start: war or peace
//!
//! [[terrain]]                 # the first that matches a place is its terrain
//! id = "hills"
//! field = "elevation"         # a field of the place...
//! above = 0.65                # ...above (and/or below) a value; no field: matches every place
//! defense = 2
//! move_cost = { foot = 1.5, horse = 2 }   # per movement type; a missing type cannot enter
//!
//! [[troop]]
//! id = "heavy"
//! name = "Men-at-arms"
//! move = "foot"
//! speed = 25                  # per day, in `distance` units
//! cost = { Gold = 40, Men = 10 }
//! upkeep = { Gold = 0.5 }     # per day
//! damage = { light = 70, heavy = 50, fast = 40 }  # % of a full troop, by the target's id
//! occupy = 4                  # presence a full troop adds per day where no enemy troop is
//! ```

use std::collections::BTreeMap;

use glassbox::Rules;
use serde::Deserialize;

fn d_steps() -> String {
    "steps".into()
}
fn d_one() -> f64 {
    1.0
}
fn d_ten() -> f64 {
    10.0
}
fn d_war() -> String {
    "war".into()
}
fn d_foot() -> String {
    "foot".into()
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarToml {
    #[serde(default = "d_steps")]
    distance: String,
    #[serde(default = "d_one")]
    damage_scale: f64,
    #[serde(default = "d_ten")]
    defense_per_star: f64,
    #[serde(default)]
    home_defense: f64,
    #[serde(default)]
    fort_map: Option<String>,
    #[serde(default)]
    luck: f64,
    #[serde(default)]
    heal: f64,
    #[serde(default)]
    unpaid: f64,
    #[serde(default)]
    violence: f64,
    #[serde(default = "d_war")]
    relation: String,
}

impl Default for WarToml {
    /// The defaults of every field, as for an empty `[war]`.
    fn default() -> Self {
        toml::from_str("").expect("every field of [war] has a default")
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainToml {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    above: Option<f64>,
    #[serde(default)]
    below: Option<f64>,
    #[serde(default)]
    defense: f64,
    #[serde(default)]
    move_cost: BTreeMap<String, f64>,
    /// A colour for viewers.
    #[serde(default)]
    color: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TroopToml {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default = "d_foot")]
    #[serde(rename = "move")]
    move_type: String,
    speed: f64,
    #[serde(default)]
    cost: toml::Table,
    #[serde(default)]
    upkeep: toml::Table,
    #[serde(default)]
    damage: BTreeMap<String, f64>,
    #[serde(default)]
    occupy: f64,
    #[serde(default)]
    description: String,
    /// A short mark for viewers (a letter or a symbol).
    #[serde(default)]
    icon: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Relation {
    /// Shares places, never fights, does not take the other's places.
    Ally,
    /// Does not fight, and stays out of the other's places.
    Peace,
    #[default]
    War,
}

impl Relation {
    pub fn parse(w: &str) -> Option<Relation> {
        Some(match w {
            "ally" | "alliance" => Relation::Ally,
            "peace" | "truce" => Relation::Peace,
            "war" => Relation::War,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Relation::Ally => "ally",
            Relation::Peace => "peace",
            Relation::War => "war",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerrainDef {
    pub id: String,
    pub name: String,
    pub field: Option<String>,
    pub above: Option<f64>,
    pub below: Option<f64>,
    pub defense: f64,
    /// By movement type (index into [`War::moves`]); `None` cannot enter.
    pub cost: Vec<Option<f64>>,
    pub color: Option<String>,
}

impl TerrainDef {
    /// Whether a place with these fields has this terrain.
    pub fn matches(&self, fields: &BTreeMap<String, f64>) -> bool {
        let Some(f) = &self.field else { return true };
        let v = fields.get(f).copied().unwrap_or(0.0);
        self.above.map_or(true, |a| v > a) && self.below.map_or(true, |b| v < b)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TroopDef {
    pub id: String,
    pub name: String,
    /// Index into [`War::moves`].
    pub move_type: usize,
    pub speed: f64,
    /// (global resource, amount).
    pub cost: Vec<(usize, f64)>,
    pub upkeep: Vec<(usize, f64)>,
    /// Damage against each troop kind, in percent of a full troop (0: cannot hit it).
    pub damage: Vec<f64>,
    pub occupy: f64,
    pub description: String,
    pub icon: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct War {
    /// Whether distances are in km between place centres (else one per border crossed).
    pub km: bool,
    pub damage_scale: f64,
    pub defense_per_star: f64,
    pub home_defense: f64,
    pub fort_map: Option<usize>,
    pub luck: f64,
    pub heal: f64,
    pub unpaid: f64,
    pub violence: f64,
    /// Between factions of different teams at the start.
    pub relation: Relation,
    /// Movement types, by index.
    pub moves: Vec<String>,
    pub terrains: Vec<TerrainDef>,
    pub troops: Vec<TroopDef>,
}

impl War {
    pub fn troop(&self, id: &str) -> Option<usize> {
        self.troops.iter().position(|t| t.id == id)
    }

    /// Builds the troops, terrains and war settings, declaring the resources and maps they use.
    pub fn build(
        rules: &mut Rules,
        w: &WarToml,
        terrains: &[TerrainToml],
        troops: &[TroopToml],
    ) -> Result<War, String> {
        let km = match w.distance.as_str() {
            "steps" => false,
            "km" => true,
            d => {
                return Err(format!(
                    "war.distance must be \"steps\" or \"km\", not {d:?}"
                ))
            }
        };
        let relation = match Relation::parse(&w.relation) {
            Some(r @ (Relation::War | Relation::Peace)) => r,
            _ => {
                return Err(format!(
                    "war.relation must be \"war\" or \"peace\", not {:?}",
                    w.relation
                ))
            }
        };
        let mut moves: Vec<String> = Vec::new();
        for t in troops {
            if !moves.contains(&t.move_type) {
                moves.push(t.move_type.clone());
            }
        }
        let amounts = |rules: &mut Rules, what: &str, t: &toml::Table| {
            let mut out = Vec::new();
            for (k, v) in t {
                let x = v
                    .as_float()
                    .or_else(|| v.as_integer().map(|i| i as f64))
                    .ok_or_else(|| format!("{what}.{k}: expected a number"))?;
                out.push((rules.declare_resource(k, glassbox::Scope::Global)?, x));
            }
            Ok::<_, String>(out)
        };
        let mut defs = Vec::new();
        for t in troops {
            if defs.iter().any(|d: &TroopDef| d.id == t.id) {
                return Err(format!("troop {} is declared twice", t.id));
            }
            if t.speed <= 0.0 {
                return Err(format!("troop {}: speed must be above 0", t.id));
            }
            defs.push(TroopDef {
                id: t.id.clone(),
                name: t.name.clone().unwrap_or_else(|| t.id.clone()),
                move_type: moves.iter().position(|m| *m == t.move_type).unwrap_or(0),
                speed: t.speed,
                cost: amounts(rules, &format!("troop {} cost", t.id), &t.cost)?,
                upkeep: amounts(rules, &format!("troop {} upkeep", t.id), &t.upkeep)?,
                damage: Vec::new(),
                occupy: t.occupy,
                description: t.description.clone(),
                icon: t.icon.clone().unwrap_or_else(|| {
                    t.name
                        .as_deref()
                        .unwrap_or(&t.id)
                        .chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_default()
                }),
            });
        }
        for (i, t) in troops.iter().enumerate() {
            let mut dmg = vec![0.0; troops.len()];
            for (target, v) in &t.damage {
                let j = defs.iter().position(|d| d.id == *target).ok_or_else(|| {
                    format!(
                        "troop {}: damage against {target:?}, which is no troop",
                        t.id
                    )
                })?;
                dmg[j] = v.max(0.0);
            }
            defs[i].damage = dmg;
        }
        let mut tdefs = Vec::new();
        for t in terrains {
            if tdefs.iter().any(|d: &TerrainDef| d.id == t.id) {
                return Err(format!("terrain {} is declared twice", t.id));
            }
            for m in t.move_cost.keys() {
                if !moves.contains(m) {
                    // A movement type no troop uses yet: harmless, but probably a typo.
                    if troops.is_empty() {
                        continue;
                    }
                    return Err(format!(
                        "terrain {}: no troop moves by {m:?} (troops move by {})",
                        t.id,
                        moves.join(", ")
                    ));
                }
            }
            tdefs.push(TerrainDef {
                id: t.id.clone(),
                name: t.name.clone().unwrap_or_else(|| t.id.clone()),
                field: t.field.clone(),
                above: t.above,
                below: t.below,
                defense: t.defense,
                cost: moves
                    .iter()
                    .map(|m| t.move_cost.get(m).copied().filter(|c| *c > 0.0))
                    .collect(),
                color: t.color.clone(),
            });
        }
        Ok(War {
            km,
            damage_scale: w.damage_scale,
            defense_per_star: w.defense_per_star,
            home_defense: w.home_defense,
            fort_map: w.fort_map.as_deref().map(|m| rules.declare_map(m)),
            luck: w.luck.max(0.0),
            heal: w.heal.max(0.0),
            unpaid: w.unpaid.max(0.0),
            violence: w.violence,
            relation,
            moves,
            terrains: tdefs,
            troops: defs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<War, String> {
        #[derive(Deserialize)]
        struct T {
            #[serde(default)]
            war: WarToml,
            #[serde(default)]
            terrain: Vec<TerrainToml>,
            #[serde(default)]
            troop: Vec<TroopToml>,
        }
        let t: T = toml::from_str(text).map_err(|e| e.to_string())?;
        War::build(&mut Rules::default(), &t.war, &t.terrain, &t.troop)
    }

    #[test]
    fn builds_troops_and_terrain() {
        let w = parse(
            "[war]\ndistance = \"km\"\n\
             [[terrain]]\nid = \"hills\"\nfield = \"elevation\"\nabove = 0.6\ndefense = 2\nmove_cost = { foot = 2 }\n\
             [[terrain]]\nid = \"plain\"\nmove_cost = { foot = 1, horse = 1 }\n\
             [[troop]]\nid = \"levy\"\nspeed = 20\ncost = { Gold = 10 }\ndamage = { levy = 50, knights = 10 }\n\
             [[troop]]\nid = \"knights\"\nmove = \"horse\"\nspeed = 40\ndamage = { levy = 90 }\n",
        )
        .unwrap();
        assert!(w.km);
        assert_eq!(w.moves, vec!["foot", "horse"]);
        assert_eq!(w.troops[0].damage, vec![50.0, 10.0]);
        assert_eq!(w.troops[1].damage, vec![90.0, 0.0]);
        assert_eq!(w.terrains[0].cost, vec![Some(2.0), None]);
        let hill = BTreeMap::from([("elevation".to_string(), 0.7)]);
        assert!(w.terrains[0].matches(&hill));
        assert!(!w.terrains[0].matches(&BTreeMap::new()));
        assert!(w.terrains[1].matches(&BTreeMap::new()));
        assert_eq!(w.troops[1].icon, "K");
    }

    #[test]
    fn rejects_mistakes() {
        assert!(
            parse("[[troop]]\nid = \"a\"\nspeed = 1\ndamage = { b = 3 }\n")
                .unwrap_err()
                .contains("no troop")
        );
        assert!(parse("[[troop]]\nid = \"a\"\nspeed = 0\n")
            .unwrap_err()
            .contains("speed"));
        assert!(parse("[[troop]]\nid = \"a\"\nspeed = 1\n[[terrain]]\nid = \"t\"\nmove_cost = { hover = 1 }\n")
            .unwrap_err()
            .contains("hover"));
        assert!(parse("[war]\nrelation = \"ally\"\n").is_err());
    }
}
