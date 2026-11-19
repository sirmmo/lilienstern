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
//! requires = "chivalry"       # a tech the faction must know to raise it
//!
//! [research]
//! resource = "Learning"       # a global resource: each day, all of it goes into the current tech
//!
//! [[tech]]
//! id = "chivalry"
//! name = "Chivalry"
//! cost = 120                  # points of the research resource
//! requires = ["horses"]       # techs it needs
//! weight = 1                  # how much the AI wants it
//! damage = 10                 # bonuses, for the troops in `troops` (default: all): damage %,
//! defense = 1                 # defence stars, speed %, occupy %, upkeep % (negative: cheaper)
//! troops = ["fast"]
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
    /// A tech the faction must know to raise it.
    #[serde(default)]
    requires: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchToml {
    resource: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechToml {
    id: String,
    #[serde(default)]
    name: Option<String>,
    cost: f64,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default = "d_one")]
    weight: f64,
    #[serde(default)]
    description: String,
    #[serde(default)]
    damage: f64,
    #[serde(default)]
    defense: f64,
    #[serde(default)]
    speed: f64,
    #[serde(default)]
    occupy: f64,
    #[serde(default)]
    upkeep: f64,
    /// The troop kinds the bonuses apply to (default: all).
    #[serde(default)]
    troops: Vec<String>,
}

/// What a faction's techs add to one kind of its troops: multipliers, and defence stars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bonus {
    pub damage: f64,
    pub defense: f64,
    pub speed: f64,
    pub occupy: f64,
    pub upkeep: f64,
}

impl Default for Bonus {
    fn default() -> Self {
        Bonus {
            damage: 1.0,
            defense: 0.0,
            speed: 1.0,
            occupy: 1.0,
            upkeep: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TechDef {
    pub id: String,
    pub name: String,
    pub cost: f64,
    /// Techs it needs (indices).
    pub requires: Vec<usize>,
    pub weight: f64,
    pub description: String,
    /// Percent (damage, speed, occupy, upkeep) and stars (defense).
    pub damage: f64,
    pub defense: f64,
    pub speed: f64,
    pub occupy: f64,
    pub upkeep: f64,
    /// Troop kinds the bonuses apply to; empty: all.
    pub troops: Vec<usize>,
}

impl TechDef {
    pub fn applies(&self, kind: usize) -> bool {
        self.troops.is_empty() || self.troops.contains(&kind)
    }
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
    /// A tech the faction must know to raise it.
    pub requires: Option<usize>,
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
    /// The global resource research is made of, when the game has techs.
    pub research: Option<usize>,
    pub techs: Vec<TechDef>,
}

impl War {
    pub fn troop(&self, id: &str) -> Option<usize> {
        self.troops.iter().position(|t| t.id == id)
    }

    pub fn tech(&self, id: &str) -> Option<usize> {
        self.techs.iter().position(|t| t.id == id)
    }

    /// What the techs in `known` add to troops of kind `kind`.
    pub fn bonus(&self, known: &[bool], kind: usize) -> Bonus {
        let mut b = Bonus::default();
        let (mut damage, mut speed, mut occupy, mut upkeep) = (0.0, 0.0, 0.0, 0.0);
        for (t, def) in self.techs.iter().enumerate() {
            if known.get(t).copied().unwrap_or(false) && def.applies(kind) {
                damage += def.damage;
                b.defense += def.defense;
                speed += def.speed;
                occupy += def.occupy;
                upkeep += def.upkeep;
            }
        }
        b.damage = (1.0 + damage / 100.0).max(0.0);
        b.speed = (1.0 + speed / 100.0).max(0.05);
        b.occupy = (1.0 + occupy / 100.0).max(0.0);
        b.upkeep = (1.0 + upkeep / 100.0).max(0.0);
        b
    }

    /// Builds the troops, terrains and war settings, declaring the resources and maps they use.
    pub fn build(
        rules: &mut Rules,
        w: &WarToml,
        terrains: &[TerrainToml],
        troops: &[TroopToml],
        research: Option<&ResearchToml>,
        techs: &[TechToml],
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
                requires: None,
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
        // Techs: ids first, so that `requires` can name any of them.
        let mut tech_defs: Vec<TechDef> = Vec::new();
        for t in techs {
            if tech_defs.iter().any(|d| d.id == t.id) {
                return Err(format!("tech {} is declared twice", t.id));
            }
            if t.cost <= 0.0 {
                return Err(format!("tech {}: cost must be above 0", t.id));
            }
            tech_defs.push(TechDef {
                id: t.id.clone(),
                name: t.name.clone().unwrap_or_else(|| t.id.clone()),
                cost: t.cost,
                requires: Vec::new(),
                weight: t.weight.max(0.0),
                description: t.description.clone(),
                damage: t.damage,
                defense: t.defense,
                speed: t.speed,
                occupy: t.occupy,
                upkeep: t.upkeep,
                troops: Vec::new(),
            });
        }
        let ids: Vec<String> = tech_defs.iter().map(|d| d.id.clone()).collect();
        let tech_index = |what: &str, id: &str| {
            ids.iter()
                .position(|d| d == id)
                .ok_or_else(|| format!("{what}: there is no tech {id:?}"))
        };
        let mut requires = Vec::new();
        for t in techs {
            let mut r = Vec::new();
            for id in &t.requires {
                r.push(tech_index(&format!("tech {}", t.id), id)?);
            }
            let mut kinds = Vec::new();
            for id in &t.troops {
                kinds.push(
                    defs.iter()
                        .position(|d| d.id == *id)
                        .ok_or_else(|| format!("tech {}: there is no troop {id:?}", t.id))?,
                );
            }
            requires.push((r, kinds));
        }
        for (d, (r, k)) in tech_defs.iter_mut().zip(requires) {
            d.requires = r;
            d.troops = k;
        }
        for (i, t) in troops.iter().enumerate() {
            if let Some(id) = &t.requires {
                defs[i].requires = Some(tech_index(&format!("troop {}", t.id), id)?);
            }
        }
        // A tech that needs itself, through others, could never be learnt.
        for start in 0..tech_defs.len() {
            let mut seen = vec![false; tech_defs.len()];
            let mut stack = tech_defs[start].requires.clone();
            while let Some(t) = stack.pop() {
                if t == start {
                    return Err(format!("tech {} needs itself", tech_defs[start].id));
                }
                if !seen[t] {
                    seen[t] = true;
                    stack.extend(tech_defs[t].requires.iter().copied());
                }
            }
        }
        let research = match research {
            Some(r) => Some(rules.declare_resource(&r.resource, glassbox::Scope::Global)?),
            None if tech_defs.is_empty() => None,
            None => return Err("techs need a [research] resource".into()),
        };
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
            research,
            techs: tech_defs,
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
            research: Option<ResearchToml>,
            #[serde(default)]
            tech: Vec<TechToml>,
        }
        let t: T = toml::from_str(text).map_err(|e| e.to_string())?;
        War::build(
            &mut Rules::default(),
            &t.war,
            &t.terrain,
            &t.troop,
            t.research.as_ref(),
            &t.tech,
        )
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
        let techs = "[research]\nresource = \"L\"\n[[troop]]\nid = \"a\"\nspeed = 1\n";
        assert!(parse(&format!("{techs}[[tech]]\nid = \"x\"\ncost = 1\nrequires = [\"y\"]\n[[tech]]\nid = \"y\"\ncost = 1\nrequires = [\"x\"]\n"))
            .unwrap_err()
            .contains("needs itself"));
        assert!(parse("[[tech]]\nid = \"x\"\ncost = 1\n")
            .unwrap_err()
            .contains("[research]"));
    }

    #[test]
    fn techs_give_bonuses_and_unlock_troops() {
        let w = parse(
            "[research]\nresource = \"Learning\"\n\
             [[troop]]\nid = \"foot\"\nspeed = 1\n\
             [[troop]]\nid = \"horse\"\nspeed = 2\nrequires = \"riding\"\n\
             [[tech]]\nid = \"riding\"\ncost = 50\n\
             [[tech]]\nid = \"lances\"\ncost = 80\nrequires = [\"riding\"]\ndamage = 20\nspeed = 10\ntroops = [\"horse\"]\n\
             [[tech]]\nid = \"bank\"\ncost = 40\nupkeep = -25\ndefense = 1\n",
        )
        .unwrap();
        assert_eq!(w.troops[1].requires, Some(0));
        assert_eq!(w.techs[1].requires, vec![0]);
        let all = [true, true, true];
        let horse = w.bonus(&all, 1);
        assert!((horse.damage - 1.2).abs() < 1e-9 && (horse.speed - 1.1).abs() < 1e-9);
        assert_eq!((horse.upkeep, horse.defense), (0.75, 1.0));
        let foot = w.bonus(&all, 0);
        assert_eq!(
            (foot.damage, foot.speed),
            (1.0, 1.0),
            "lances are for horses"
        );
        assert_eq!(w.bonus(&[false; 3], 1), Bonus::default());
    }
}
