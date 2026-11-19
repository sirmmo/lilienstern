//! Lilienstern's script blocks, next to GlassBox's in the same files: `event`, `decision` and
//! `modifier`. Statements are one per line:
//!
//! ```text
//! decision expand                 # something a faction can do
//!     for gang                    # which kind of faction (gang, police, other, any)
//!     target adjacent             # none, owned, adjacent, present, contested, any
//!     every 2                     # considered every 2 days
//!     cost global Crew 3          # paid when taken; also a condition
//!     if map Police less 40       # conditions on the target place
//!     do presence add 25          # effects
//!     weight 5                    # how much the AI wants it
//!     weight map Wealth 0.1       # + 0.1 x Wealth at the target
//! end
//!
//! event shock                     # something that happens
//!     scope place                 # place (each place), faction (each faction) or world
//!     if day is 365
//!     if map x greater 0
//!     do map Wealth mul 0.6
//!     log Shocks 1
//! end
//!
//! modifier lockdown               # a timed state of a place, applied with `do modifier`
//!     output mul 0.3              # GlassBox output factor of the place
//!     daily map Heat add -1       # effect every day while it lasts
//! end
//! ```
//!
//! In a decision, `map`, `presence` and the other place conditions read the target (`target`
//! may be written before them for clarity); in a place event they read the place, and the
//! faction is the place's owner.

use glassbox::{number, Block, Cmp, Error, Line, Rules, Scope};

use crate::war::{Relation, War};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Gang,
    /// Holds territory like a gang: a kingdom, a corporation, a colony.
    Realm,
    Police,
    Other,
}

impl Kind {
    pub fn parse(word: &str) -> Option<Kind> {
        Some(match word {
            "gang" => Kind::Gang,
            "realm" => Kind::Realm,
            "police" => Kind::Police,
            "other" => Kind::Other,
            _ => return None,
        })
    }

    /// Whether factions of this kind hold places by presence.
    pub fn holds(self) -> bool {
        matches!(self, Kind::Gang | Kind::Realm)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Mul,
    Set,
}

impl Op {
    fn parse(word: &str) -> Option<Op> {
        Some(match word {
            "add" => Op::Add,
            "mul" => Op::Mul,
            "set" => Op::Set,
            _ => return None,
        })
    }

    pub fn apply(self, v: f64, x: f64) -> f64 {
        match self {
            Op::Add => v + x,
            Op::Mul => v * x,
            Op::Set => x,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerTest {
    /// Owned by the faction in scope.
    Me,
    /// Owned by another faction.
    Other,
    None,
    Any,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cond {
    Map {
        map: usize,
        cmp: Cmp,
        value: f64,
    },
    /// The faction's presence at the place.
    Presence {
        cmp: Cmp,
        value: f64,
    },
    /// The strongest other gang's presence at the place.
    Rival {
        cmp: Cmp,
        value: f64,
    },
    Owner(OwnerTest),
    /// Units at the place (of one kind, or any).
    Units {
        unit: Option<usize>,
        cmp: Cmp,
        value: f64,
    },
    /// The faction's global resource.
    Global {
        resource: usize,
        cmp: Cmp,
        value: f64,
    },
    /// Places the faction owns.
    Territory {
        cmp: Cmp,
        value: f64,
    },
    /// The faction's rank by territory among factions of its kind (1 is the largest).
    Rank {
        cmp: Cmp,
        value: f64,
    },
    Day {
        cmp: Cmp,
        value: f64,
    },
    Chance(f64),
    /// A troop-based count: see [`Count`].
    Count {
        what: Count,
        cmp: Cmp,
        value: f64,
    },
    /// The faction's relation with the place's owner (false when nobody holds it, or the
    /// faction does).
    Relation(Relation),
}

/// Numbers about troops, for conditions and weights. Strengths count a full troop as 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    /// The faction's troops at the place.
    Troops,
    /// Enemy troops (at war with the faction) at the place.
    Foes,
    /// Enemy troops at the place and next to it.
    Threat,
    /// How many troops the faction has.
    Army,
    /// How many of them have no orders.
    Idle,
}

impl Count {
    fn parse(w: &str) -> Option<Count> {
        Some(match w {
            "troops" => Count::Troops,
            "foes" => Count::Foes,
            "threat" => Count::Threat,
            "army" => Count::Army,
            "idle" => Count::Idle,
            _ => return None,
        })
    }

    fn needs_place(self) -> bool {
        matches!(self, Count::Troops | Count::Foes | Count::Threat)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Map {
        map: usize,
        op: Op,
        value: f64,
        radius: u32,
    },
    Presence {
        op: Op,
        value: f64,
    },
    /// Every other gang's presence at the place.
    Rivals {
        op: Op,
        value: f64,
    },
    /// Every gang's presence at the place.
    AllPresence {
        op: Op,
        value: f64,
    },
    /// The faction's presence in every place it owns.
    Turf {
        op: Op,
        value: f64,
    },
    Global {
        resource: usize,
        op: Op,
        value: f64,
    },
    Modifier {
        modifier: usize,
        days: u32,
    },
    /// A new unit of the faction at the place.
    Build {
        unit: usize,
    },
    /// Other factions' units at the place become the faction's.
    Seize,
    /// Units at the place are removed: other factions' (`rival`) or all.
    Destroy {
        rival_only: bool,
    },
    Log {
        metric: usize,
        value: f64,
    },
    /// A new troop of the faction at the place (in a decision, its cost is added to the
    /// decision's).
    Raise {
        troop: usize,
    },
    /// Sends up to `count` (0: all) of the faction's troops without orders to the place, the
    /// closest first.
    March {
        count: u32,
    },
    /// Sets the relation between the faction and the place's owner.
    SetRelation(Relation),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    Const,
    Map(usize),
    Presence,
    Rival,
    Global(usize),
    /// 1 when the place has no owner.
    Unowned,
    /// Units at the place.
    Units(Option<usize>),
    Count(Count),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Weight {
    pub term: Term,
    pub k: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The faction itself, no place.
    None,
    Owned,
    /// Places next to the faction's territory that it does not own.
    Adjacent,
    /// Places where the faction has some presence.
    Present,
    /// Places where the faction and a rival both have more than the control's `contest`.
    Contested,
    Any,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub name: String,
    /// What a player sees (`label ...`; the name by default).
    pub label: String,
    pub who: Option<Kind>,
    pub target: Target,
    pub every: u32,
    pub conds: Vec<Cond>,
    pub costs: Vec<(usize, f64)>,
    pub effects: Vec<Effect>,
    pub weights: Vec<Weight>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventScope {
    Place,
    Faction,
    World,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub name: String,
    pub scope: EventScope,
    pub who: Option<Kind>,
    pub every: u32,
    pub conds: Vec<Cond>,
    pub effects: Vec<Effect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModifierDef {
    pub name: String,
    pub output: f64,
    pub daily: Vec<Effect>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    pub events: Vec<Event>,
    pub decisions: Vec<Decision>,
    pub modifiers: Vec<ModifierDef>,
}

pub const KINDS: [&str; 3] = ["event", "decision", "modifier"];

struct Ctx<'a> {
    file: &'a str,
    line: &'a Line,
}

impl Ctx<'_> {
    fn err(&self, m: impl Into<String>) -> Error {
        Error::new(self.file, self.line.no, m)
    }
    fn word(&self, i: usize) -> Result<&str, Error> {
        self.line
            .words
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| self.err(format!("{} is incomplete", self.line.words.join(" "))))
    }
    fn num(&self, i: usize) -> Result<f64, Error> {
        number(self.file, self.line, i)
    }
    fn cmp(&self, i: usize) -> Result<Cmp, Error> {
        let w = self.word(i)?;
        Cmp::parse(w).ok_or_else(|| {
            self.err(format!(
                "expected atLeast, atMost, greater, less or is, found {w:?}"
            ))
        })
    }
    fn op(&self, i: usize) -> Result<Op, Error> {
        let w = self.word(i)?;
        Op::parse(w).ok_or_else(|| self.err(format!("expected add, mul or set, found {w:?}")))
    }
}

/// Skips an optional `target` before a place condition, effect or weight.
fn skip_target(c: &Ctx, i: usize) -> usize {
    if c.line.words.get(i).map(String::as_str) == Some("target") {
        i + 1
    } else {
        i
    }
}

fn global(rules: &mut Rules, c: &Ctx, id: &str) -> Result<usize, Error> {
    rules
        .declare_resource(id, Scope::Global)
        .map_err(|m| c.err(m))
}

fn unit(rules: &Rules, c: &Ctx, id: &str) -> Result<usize, Error> {
    rules
        .unit(id)
        .ok_or_else(|| c.err(format!("there is no unit {id:?}")))
}

fn cond(rules: &mut Rules, c: &Ctx, i: usize) -> Result<Cond, Error> {
    let i = skip_target(c, i);
    Ok(match c.word(i)? {
        "map" => Cond::Map {
            map: rules.declare_map(c.word(i + 1)?),
            cmp: c.cmp(i + 2)?,
            value: c.num(i + 3)?,
        },
        "presence" => Cond::Presence {
            cmp: c.cmp(i + 1)?,
            value: c.num(i + 2)?,
        },
        "rival" => Cond::Rival {
            cmp: c.cmp(i + 1)?,
            value: c.num(i + 2)?,
        },
        "owner" => {
            let w = if c.word(i + 1)? == "is" {
                c.word(i + 2)?
            } else {
                c.word(i + 1)?
            };
            Cond::Owner(match w {
                "self" | "me" => OwnerTest::Me,
                "other" => OwnerTest::Other,
                "none" => OwnerTest::None,
                "any" => OwnerTest::Any,
                _ => {
                    return Err(c.err(format!(
                        "owner: expected self, other, none or any, found {w:?}"
                    )))
                }
            })
        }
        "units" => match Cmp::parse(c.word(i + 1)?) {
            Some(cmp) => Cond::Units {
                unit: None,
                cmp,
                value: c.num(i + 2)?,
            },
            None => Cond::Units {
                unit: Some(unit(rules, c, c.word(i + 1)?)?),
                cmp: c.cmp(i + 2)?,
                value: c.num(i + 3)?,
            },
        },
        "global" => Cond::Global {
            resource: global(rules, c, c.word(i + 1)?)?,
            cmp: c.cmp(i + 2)?,
            value: c.num(i + 3)?,
        },
        "territory" => Cond::Territory {
            cmp: c.cmp(i + 1)?,
            value: c.num(i + 2)?,
        },
        "rank" => Cond::Rank {
            cmp: c.cmp(i + 1)?,
            value: c.num(i + 2)?,
        },
        "day" => Cond::Day {
            cmp: c.cmp(i + 1)?,
            value: c.num(i + 2)?,
        },
        "chance" => Cond::Chance(c.num(i + 1)?.clamp(0.0, 1.0)),
        "relation" => Cond::Relation(relation(c, i + 1)?),
        w => match Count::parse(w) {
            Some(what) => Cond::Count {
                what,
                cmp: c.cmp(i + 1)?,
                value: c.num(i + 2)?,
            },
            None => return Err(c.err(format!("unknown condition {w:?}"))),
        },
    })
}

fn relation(c: &Ctx, i: usize) -> Result<Relation, Error> {
    let w = c.word(i)?;
    Relation::parse(w).ok_or_else(|| c.err(format!("expected war, peace or ally, found {w:?}")))
}

fn effect(
    rules: &mut Rules,
    script: &Script,
    war: &War,
    c: &Ctx,
    i: usize,
) -> Result<Effect, Error> {
    let i = skip_target(c, i);
    Ok(match c.word(i)? {
        "map" => {
            let radius = match c.line.words.iter().position(|w| w == "radius") {
                Some(r) => c.num(r + 1)?.max(0.0) as u32,
                None => 0,
            };
            Effect::Map {
                map: rules.declare_map(c.word(i + 1)?),
                op: c.op(i + 2)?,
                value: c.num(i + 3)?,
                radius,
            }
        }
        "presence" => Effect::Presence {
            op: c.op(i + 1)?,
            value: c.num(i + 2)?,
        },
        "rivals" => Effect::Rivals {
            op: c.op(i + 1)?,
            value: c.num(i + 2)?,
        },
        "all" if c.word(i + 1)? == "presence" => Effect::AllPresence {
            op: c.op(i + 2)?,
            value: c.num(i + 3)?,
        },
        "turf" => Effect::Turf {
            op: c.op(i + 1)?,
            value: c.num(i + 2)?,
        },
        "global" => Effect::Global {
            resource: global(rules, c, c.word(i + 1)?)?,
            op: c.op(i + 2)?,
            value: c.num(i + 3)?,
        },
        "modifier" => {
            let name = c.word(i + 1)?;
            let modifier = script
                .modifiers
                .iter()
                .position(|m| m.name == name)
                .ok_or_else(|| c.err(format!("there is no modifier {name:?}")))?;
            Effect::Modifier {
                modifier,
                days: c.num(i + 2)?.max(1.0) as u32,
            }
        }
        "build" => Effect::Build {
            unit: unit(rules, c, c.word(i + 1)?)?,
        },
        "seize" => Effect::Seize,
        "destroy" => match c.word(i + 1)? {
            "rival" => Effect::Destroy { rival_only: true },
            "all" => Effect::Destroy { rival_only: false },
            w => return Err(c.err(format!("destroy: expected rival or all, found {w:?}"))),
        },
        "log" => Effect::Log {
            metric: rules.metric(c.word(i + 1)?),
            value: if c.line.words.len() > i + 2 {
                c.num(i + 2)?
            } else {
                1.0
            },
        },
        "raise" => {
            let id = c.word(i + 1)?;
            Effect::Raise {
                troop: war
                    .troop(id)
                    .ok_or_else(|| c.err(format!("there is no troop {id:?}")))?,
            }
        }
        "march" => Effect::March {
            count: match c.line.words.get(i + 1).map(String::as_str) {
                None => 1,
                Some("all") => 0,
                Some(_) => c.num(i + 1)?.max(1.0) as u32,
            },
        },
        "relation" => Effect::SetRelation(relation(c, i + 1)?),
        w => return Err(c.err(format!("unknown effect {w:?}"))),
    })
}

fn weight(rules: &mut Rules, c: &Ctx) -> Result<Weight, Error> {
    let w = &c.line.words;
    if w.len() == 2 {
        return Ok(Weight {
            term: Term::Const,
            k: c.num(1)?,
        });
    }
    let i = skip_target(c, 1);
    let (term, k) = match c.word(i)? {
        "map" => (Term::Map(rules.declare_map(c.word(i + 1)?)), c.num(i + 2)?),
        "presence" => (Term::Presence, c.num(i + 1)?),
        "rival" => (Term::Rival, c.num(i + 1)?),
        "unowned" => (Term::Unowned, c.num(i + 1)?),
        "global" => (
            Term::Global(global(rules, c, c.word(i + 1)?)?),
            c.num(i + 2)?,
        ),
        "units" => match c.line.words.get(i + 2) {
            Some(_) => (
                Term::Units(Some(unit(rules, c, c.word(i + 1)?)?)),
                c.num(i + 2)?,
            ),
            None => (Term::Units(None), c.num(i + 1)?),
        },
        t => match Count::parse(t) {
            Some(what) => (Term::Count(what), c.num(i + 1)?),
            None => return Err(c.err(format!("unknown weight {t:?}"))),
        },
    };
    Ok(Weight { term, k })
}

fn who(c: &Ctx) -> Result<Option<Kind>, Error> {
    let w = c.word(1)?;
    if w == "any" {
        return Ok(None);
    }
    Kind::parse(w).map(Some).ok_or_else(|| {
        c.err(format!(
            "expected gang, realm, police, other or any, found {w:?}"
        ))
    })
}

fn every(c: &Ctx) -> Result<u32, Error> {
    let n = c.num(1)?;
    if n < 1.0 {
        return Err(c.err("every must be at least 1"));
    }
    Ok(n.round() as u32)
}

/// Compiles `event`, `decision` and `modifier` blocks. Modifiers are compiled first, so they can
/// be used anywhere in the files.
pub fn compile(
    script: &mut Script,
    rules: &mut Rules,
    war: &War,
    file: &str,
    blocks: &[Block],
) -> Result<(), Error> {
    for b in blocks.iter().filter(|b| b.kind == "modifier") {
        if script.modifiers.iter().any(|m| m.name == b.name) {
            return Err(Error::new(
                file,
                b.no,
                format!("modifier {} is defined twice", b.name),
            ));
        }
        script.modifiers.push(ModifierDef {
            name: b.name.clone(),
            output: 1.0,
            daily: Vec::new(),
        });
    }
    for b in blocks.iter().filter(|b| b.kind == "modifier") {
        let mi = script
            .modifiers
            .iter()
            .position(|m| m.name == b.name)
            .expect("declared");
        for l in &b.lines {
            let c = Ctx { file, line: l };
            match l.words[0].as_str() {
                "output" => {
                    let op = c.op(1)?;
                    let v = c.num(2)?;
                    let m = &mut script.modifiers[mi];
                    m.output = op.apply(m.output, v);
                }
                "daily" | "do" => {
                    let e = effect(rules, script, war, &c, 1)?;
                    script.modifiers[mi].daily.push(e);
                }
                w => return Err(c.err(format!("modifier {}: unknown statement {w:?}", b.name))),
            }
        }
    }
    for b in blocks.iter().filter(|b| b.kind != "modifier") {
        let mut conds = Vec::new();
        let mut effects = Vec::new();
        let mut weights = Vec::new();
        let mut costs = Vec::new();
        let mut kind_of: Option<Kind> = None;
        let mut target = Target::None;
        let mut scope = EventScope::Place;
        let mut every_n = 1;
        let mut label = b.name.replace('_', " ");
        let decision = b.kind == "decision";
        for l in &b.lines {
            let c = Ctx { file, line: l };
            match l.words[0].as_str() {
                "if" => conds.push(cond(rules, &c, 1)?),
                "chance" => conds.push(Cond::Chance(c.num(1)?.clamp(0.0, 1.0))),
                "do" => {
                    let e = effect(rules, script, war, &c, 1)?;
                    if let (true, Effect::Raise { troop }) = (decision, &e) {
                        costs.extend(war.troops[*troop].cost.iter().copied());
                    }
                    effects.push(e);
                }
                "log" => effects.push(effect(rules, script, war, &c, 0)?),
                "every" => every_n = every(&c)?,
                "for" => kind_of = who(&c)?,
                "label" if decision => label = l.words[1..].join(" "),
                "weight" if decision => weights.push(weight(rules, &c)?),
                "cost" if decision => {
                    let (id, i) = if c.word(1)? == "global" { (c.word(2)?, 3) } else { (c.word(1)?, 2) };
                    costs.push((global(rules, &c, id)?, c.num(i)?));
                }
                "target" if decision => {
                    target = match c.word(1)? {
                        "none" | "self" => Target::None,
                        "owned" => Target::Owned,
                        "adjacent" => Target::Adjacent,
                        "present" => Target::Present,
                        "contested" => Target::Contested,
                        "any" => Target::Any,
                        w => return Err(c.err(format!("target: expected none, owned, adjacent, present, contested or any, found {w:?}"))),
                    }
                }
                "scope" if !decision => {
                    scope = match c.word(1)? {
                        "place" => EventScope::Place,
                        "faction" => EventScope::Faction,
                        "world" => EventScope::World,
                        w => return Err(c.err(format!("scope: expected place, faction or world, found {w:?}"))),
                    }
                }
                w => return Err(c.err(format!("{} {}: unknown statement {w:?}", b.kind, b.name))),
            }
        }
        let placeless = if decision {
            target == Target::None
        } else {
            scope != EventScope::Place
        };
        if placeless {
            let place_cond = conds.iter().any(|c| {
                matches!(
                    c,
                    Cond::Map { .. }
                        | Cond::Presence { .. }
                        | Cond::Rival { .. }
                        | Cond::Owner(_)
                        | Cond::Units { .. }
                        | Cond::Relation(_)
                ) || matches!(c, Cond::Count { what, .. } if what.needs_place())
            });
            let place_effect = effects.iter().any(|e| {
                matches!(
                    e,
                    Effect::Map { .. }
                        | Effect::Presence { .. }
                        | Effect::Rivals { .. }
                        | Effect::AllPresence { .. }
                        | Effect::Modifier { .. }
                        | Effect::Build { .. }
                        | Effect::Seize
                        | Effect::Destroy { .. }
                        | Effect::Raise { .. }
                        | Effect::March { .. }
                        | Effect::SetRelation(_)
                )
            });
            if place_cond || place_effect {
                return Err(Error::new(
                    file,
                    b.no,
                    format!(
                        "{} {} has no place: it cannot test or change one",
                        b.kind, b.name
                    ),
                ));
            }
        }
        if decision {
            script.decisions.push(Decision {
                name: b.name.clone(),
                label,
                who: kind_of,
                target,
                every: every_n,
                conds,
                costs,
                effects,
                weights,
            });
        } else {
            script.events.push(Event {
                name: b.name.clone(),
                scope,
                who: kind_of,
                every: every_n,
                conds,
                effects,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(text: &str) -> Result<(Script, Rules), Error> {
        let mut rules = Rules::default();
        rules.declare_unit("corner", "Corner");
        let (_, rest) = glassbox::compile(&mut rules, "t.lil", text, &KINDS)?;
        let mut s = Script::default();
        compile(&mut s, &mut rules, &War::default(), "t.lil", &rest)?;
        Ok((s, rules))
    }

    #[test]
    fn compiles_decisions_events_and_modifiers() {
        let (s, rules) = build(
            "decision expand\n for gang\n target adjacent\n every 2\n cost global Crew 3\n if target map Police less 40\n if units corner less 1\n do presence add 25\n do modifier lockdown 5\n weight 5\n weight map Wealth 0.1\n weight unowned 3\nend\n\
             event shock\n scope place\n if day is 365\n chance 0.5\n do map Wealth mul 0.6 radius 1\n log Shocks\nend\n\
             modifier lockdown\n output mul 0.3\n daily map Heat add -1\nend\n",
        )
        .unwrap();
        let d = &s.decisions[0];
        assert_eq!(
            (d.who, d.target, d.every),
            (Some(Kind::Gang), Target::Adjacent, 2)
        );
        assert_eq!(d.costs, vec![(rules.resource("Crew").unwrap(), 3.0)]);
        assert_eq!(d.conds.len(), 2);
        assert_eq!(
            d.effects[1],
            Effect::Modifier {
                modifier: 0,
                days: 5
            }
        );
        assert_eq!(d.weights.len(), 3);
        let e = &s.events[0];
        assert_eq!(e.conds[1], Cond::Chance(0.5));
        assert_eq!(
            e.effects[0],
            Effect::Map {
                map: rules.map("Wealth").unwrap(),
                op: Op::Mul,
                value: 0.6,
                radius: 1
            }
        );
        assert_eq!(s.modifiers[0].output, 0.3);
    }

    #[test]
    fn rejects_mistakes() {
        assert!(build("decision d\n target nowhere\nend\n")
            .unwrap_err()
            .message
            .contains("nowhere"));
        assert!(build("event e\n scope faction\n do presence add 1\nend\n")
            .unwrap_err()
            .message
            .contains("no place"));
        assert!(build("event e\n do modifier nope 3\nend\n")
            .unwrap_err()
            .message
            .contains("nope"));
        let e = build("event e\n\n if map Heat sideways 3\nend\n").unwrap_err();
        assert_eq!(e.line, 3);
    }
}
