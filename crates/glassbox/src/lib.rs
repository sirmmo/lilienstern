//! GlassBox on a graph.
//!
//! The rule language Maxis presented for SimCity's GlassBox engine (Andrew Willmott, *Inside
//! GlassBox*, GDC 2012), with the metrics and agent rules of the Extended GlassBox model for
//! urban digital twins (Costagliola, Montanari, Bellavista, *Interconnecting Urban Networks: a
//! Novel Approach to Digital Twins through GlassBox Adaptation*), run on a graph of places
//! instead of a tile grid: provinces, neighbourhoods, districts.
//!
//! * **Units** sit in places and run **unit rules**: they take resources in and put them out.
//! * **Resources** are `local` (held by the unit), `global` (held by the unit's account: a
//!   treasury, a faction) or `agent` (sent to units that need them along the graph).
//! * **Maps** are values per place (pollution, land value, heat). Rules read them, emit into
//!   them and take from them; maps decay and diffuse to neighbouring places.
//! * **Map rules** run on every place, **agent rules** on every place an agent passes.
//! * **Metrics** (`log`) are counted every tick.
//!
//! The engine is deterministic: no randomness, no hash-ordered iteration, no transcendental
//! functions. Two runs of the same rules on the same state give the same result bit for bit.

mod engine;
mod script;

pub use engine::{around, emit, step, State, Unit};
pub use script::{compile, number, option, plain, split_blocks, tokenize, Block, Line, KINDS};

use std::fmt;

/// An error in a rule file, with the line it was found on (0 when it is about the whole file).
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub file: String,
    pub line: usize,
    pub message: String,
}

impl Error {
    pub fn new(file: &str, line: usize, message: impl Into<String>) -> Error {
        Error {
            file: file.to_string(),
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "{}:{}: {}", self.file, self.line, self.message)
        } else {
            write!(f, "{}: {}", self.file, self.message)
        }
    }
}

impl std::error::Error for Error {}

/// Where a resource is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// In the unit.
    Local,
    /// In the unit's account (a treasury).
    Global,
    /// Sent by agents along the graph to the units that need it.
    Agent,
}

impl Scope {
    pub fn parse(word: &str) -> Option<Scope> {
        Some(match word {
            "local" => Scope::Local,
            "global" => Scope::Global,
            "agent" => Scope::Agent,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Scope::Local => "local",
            Scope::Global => "global",
            Scope::Agent => "agent",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResourceDef {
    pub id: String,
    pub scope: Scope,
    /// Agent resources: how many steps along the graph an agent travels.
    pub reach: u32,
    /// Agent resources: delivered only to units of the same account.
    pub private: bool,
}

#[derive(Clone, Debug)]
pub struct MapDef {
    pub id: String,
    /// Fraction lost every tick.
    pub decay: f64,
    /// Fraction spread evenly to the neighbouring places every tick.
    pub diffuse: f64,
    pub min: f64,
    pub max: f64,
}

impl MapDef {
    pub fn new(id: &str) -> MapDef {
        MapDef {
            id: id.to_string(),
            decay: 0.0,
            diffuse: 0.0,
            min: 0.0,
            max: f64::INFINITY,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnitDef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    AtLeast,
    AtMost,
    Greater,
    Less,
    Is,
}

impl Cmp {
    pub fn parse(word: &str) -> Option<Cmp> {
        Some(match word {
            "atLeast" => Cmp::AtLeast,
            "atMost" => Cmp::AtMost,
            "greater" => Cmp::Greater,
            "less" => Cmp::Less,
            "is" => Cmp::Is,
            _ => return None,
        })
    }

    pub fn test(self, a: f64, b: f64) -> bool {
        match self {
            Cmp::AtLeast => a >= b,
            Cmp::AtMost => a <= b,
            Cmp::Greater => a > b,
            Cmp::Less => a < b,
            Cmp::Is => (a - b).abs() < 1e-9,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Flow {
    pub resource: usize,
    pub amount: f64,
}

#[derive(Clone, Debug)]
pub struct MapFlow {
    pub map: usize,
    pub amount: f64,
    /// Places within this many steps get `amount / (1 + steps)`.
    pub radius: u32,
}

#[derive(Clone, Debug)]
pub struct MapTest {
    pub map: usize,
    pub cmp: Cmp,
    pub value: f64,
}

#[derive(Clone, Debug)]
pub struct GlobalTest {
    pub resource: usize,
    pub cmp: Cmp,
    pub value: f64,
}

/// What a rule tests, takes and gives. Tests are gates: a rule whose test fails does nothing.
/// Inputs scale the rule: with half of what it needs, it takes that half and gives half.
#[derive(Clone, Debug, Default)]
pub struct Body {
    pub inputs: Vec<Flow>,
    pub outputs: Vec<Flow>,
    pub map_in: Vec<MapFlow>,
    pub map_out: Vec<MapFlow>,
    pub map_tests: Vec<MapTest>,
    pub global_tests: Vec<GlobalTest>,
    pub logs: Vec<(usize, f64)>,
}

#[derive(Clone, Debug, Default)]
pub struct UnitRule {
    pub name: String,
    pub units: Vec<usize>,
    /// Runs every `every` ticks (`rate`).
    pub every: u32,
    pub body: Body,
}

#[derive(Clone, Debug, Default)]
pub struct MapRule {
    pub name: String,
    pub every: u32,
    pub body: Body,
}

#[derive(Clone, Debug)]
pub struct AgentRule {
    pub name: String,
    pub resource: usize,
    /// Emits every `every` steps travelled (`repeatAfter`), counting the start.
    pub every: u32,
    /// Emitted per unit carried.
    pub areas: Vec<MapFlow>,
}

/// Declarations and compiled rules.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    pub resources: Vec<ResourceDef>,
    pub maps: Vec<MapDef>,
    pub units: Vec<UnitDef>,
    pub unit_rules: Vec<UnitRule>,
    pub map_rules: Vec<MapRule>,
    pub agent_rules: Vec<AgentRule>,
    pub metrics: Vec<String>,
}

impl Rules {
    pub fn resource(&self, id: &str) -> Option<usize> {
        self.resources.iter().position(|r| r.id == id)
    }

    pub fn map(&self, id: &str) -> Option<usize> {
        self.maps.iter().position(|m| m.id == id)
    }

    pub fn unit(&self, id: &str) -> Option<usize> {
        self.units.iter().position(|u| u.id == id)
    }

    /// The metric's index, declaring it on first use.
    pub fn metric(&mut self, id: &str) -> usize {
        match self.metrics.iter().position(|m| m == id) {
            Some(i) => i,
            None => {
                self.metrics.push(id.to_string());
                self.metrics.len() - 1
            }
        }
    }

    /// The map's index, declaring it (no decay, no diffusion) on first use.
    pub fn declare_map(&mut self, id: &str) -> usize {
        match self.map(id) {
            Some(i) => i,
            None => {
                self.maps.push(MapDef::new(id));
                self.maps.len() - 1
            }
        }
    }

    /// The resource's index, declaring it on first use. Using a resource with another scope
    /// than the one it was declared with is an error.
    pub fn declare_resource(&mut self, id: &str, scope: Scope) -> Result<usize, String> {
        match self.resource(id) {
            Some(i) if self.resources[i].scope == scope => Ok(i),
            Some(i) => Err(format!(
                "{id} is a {} resource, used here as {}",
                self.resources[i].scope.name(),
                scope.name()
            )),
            None => {
                self.resources.push(ResourceDef {
                    id: id.to_string(),
                    scope,
                    reach: 6,
                    private: false,
                });
                Ok(self.resources.len() - 1)
            }
        }
    }

    pub fn declare_unit(&mut self, id: &str, name: &str) -> usize {
        match self.unit(id) {
            Some(i) => i,
            None => {
                self.units.push(UnitDef {
                    id: id.to_string(),
                    name: name.to_string(),
                });
                self.units.len() - 1
            }
        }
    }

    /// For every unit definition, the unit rules that apply to it.
    pub fn rules_by_unit(&self) -> Vec<Vec<usize>> {
        let mut out = vec![Vec::new(); self.units.len()];
        for (i, r) in self.unit_rules.iter().enumerate() {
            for &u in &r.units {
                out[u].push(i);
            }
        }
        out
    }
}
