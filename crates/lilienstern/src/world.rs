//! A running game. Every day, in this order:
//!
//! 1. **modifiers** set each place's output factor and apply their daily effects;
//! 2. **GlassBox** runs units, agents and map rules, then maps decay and diffuse;
//! 3. factions get their **income**;
//! 4. **events** fire;
//! 5. every faction's **AI** takes its best decisions;
//! 6. **control**: gangs clash where they meet, presence decays, places change hands;
//! 7. **metrics** are recorded.

use std::collections::BTreeMap;

use glassbox::{around, State};

use crate::city::{self, City};
use crate::game::{Choice, Game, MapInit};
use crate::rng::Rng;
use crate::script::{Cond, Effect, EventScope, Kind, Op, OwnerTest, Target, Term};

#[derive(Clone, Debug, PartialEq)]
pub struct Faction {
    pub account: usize,
    pub territory: usize,
    pub alive: bool,
    /// Headquarters, if the faction started with one.
    pub hq: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveModifier {
    pub modifier: usize,
    pub place: usize,
    /// The first day it no longer applies.
    pub until: u32,
}

/// A decision the AI may take: its score, index, target and whether it is affordable.
type Opt = (f64, usize, Option<usize>, bool);

#[derive(Clone, Copy, Debug)]
struct Ctx {
    faction: Option<usize>,
    place: Option<usize>,
}

/// Running sums for the time series and the summary.
#[derive(Clone, Debug, Default, PartialEq)]
struct Sums {
    violence: f64,
    clashes: f64,
    captures: f64,
    metrics: Vec<f64>,
}

impl Sums {
    fn add(&mut self, other: &Sums) {
        self.violence += other.violence;
        self.clashes += other.clashes;
        self.captures += other.captures;
        if self.metrics.len() < other.metrics.len() {
            self.metrics.resize(other.metrics.len(), 0.0);
        }
        for (a, b) in self.metrics.iter_mut().zip(&other.metrics) {
            *a += b;
        }
    }
}

/// A decision a faction can take, as listed by [`World::options`].
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub decision: usize,
    pub name: String,
    pub label: String,
    pub target: Option<usize>,
    pub affordable: bool,
    /// (resource, amount).
    pub costs: Vec<(String, f64)>,
    /// What the AI would score it.
    pub score: f64,
}

/// The state of the map on a day, for replaying a game.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub day: u32,
    /// Holder of each place (faction index), -1 for nobody.
    pub owner: Vec<i32>,
    /// `maps[map][place]`, to one decimal.
    pub maps: Vec<Vec<f32>>,
    /// Places held by each faction.
    pub territory: Vec<usize>,
}

/// A table of numbers by day.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Series {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<f64>>,
}

impl Series {
    pub fn to_csv(&self) -> String {
        let mut out = self.columns.join(",");
        out.push('\n');
        for r in &self.rows {
            let cells: Vec<String> = r.iter().map(|v| fmt_num(*v)).collect();
            out.push_str(&cells.join(","));
            out.push('\n');
        }
        out
    }
}

pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        format!("{v:.4}")
    }
}

pub struct World<'g> {
    pub game: &'g Game,
    pub seed: u64,
    pub city: City,
    pub gb: State,
    /// `presence[faction][place]`.
    pub presence: Vec<Vec<f64>>,
    pub owner: Vec<Option<usize>>,
    pub factions: Vec<Faction>,
    pub modifiers: Vec<ActiveModifier>,
    pub day: u32,
    pub rng: Rng,
    /// Living units at each place.
    pub units_at: Vec<Vec<usize>>,
    /// Indices of the gang factions.
    pub gangs: Vec<usize>,
    /// Places where two gangs clashed today.
    pub contested: usize,
    /// Places above the control's hotspot level today.
    pub hotspots: usize,
    /// Times each decision was taken.
    pub taken: Vec<u64>,
    /// Decisions in a row each faction has saved for.
    pub saving: Vec<u32>,
    /// Places that have been hotspots at some point.
    pub ever_hot: Vec<bool>,
    pub series: Series,
    /// Snapshots for replaying the game, when kept (see [`World::keep_history`]).
    pub history: Option<Vec<Frame>>,
    /// The faction a person plays: the AI leaves it alone, and [`World::take`] acts for it.
    pub player: Option<usize>,
    /// Decisions the player has taken today.
    pub player_actions: u32,
    /// While true, `chance` conditions hold without drawing (to list options).
    dry: bool,
    logs: Vec<f64>,
    today: Sums,
    period: Sums,
    total: Sums,
    /// Sums over days, for means: contested, hotspots, then each map's mean.
    means: Vec<f64>,
}

impl<'g> World<'g> {
    pub fn new(game: &'g Game, seed: u64) -> Result<World<'g>, String> {
        let city = match game.city.import_file() {
            Some(f) => city::load_with(f, &|n: &str| game.files.read(n), &game.city, seed)?,
            None => city::generate(&game.city, seed),
        };
        Ok(World::with_city(game, seed, city))
    }

    pub fn with_city(game: &'g Game, seed: u64, city: City) -> World<'g> {
        let n = city.len();
        let nf = game.factions.len();
        let rules = &game.rules;
        let mut gb = State::new(rules, n, nf + 1);
        for (m, (init, scale)) in game.map_init.iter().enumerate() {
            let def = &rules.maps[m];
            for p in 0..n {
                let v = match init {
                    MapInit::Value(v) => *v,
                    MapInit::Field(f) => city.field(p, f),
                };
                gb.maps[m][p] = (v * scale).clamp(def.min, def.max);
            }
        }
        let factions: Vec<Faction> = (0..nf)
            .map(|i| Faction {
                account: i + 1,
                territory: 0,
                alive: true,
                hq: None,
            })
            .collect();
        for (i, f) in game.factions.iter().enumerate() {
            for &(r, v) in &f.start {
                gb.accounts[i + 1][r] += v;
            }
        }
        let mut w = World {
            game,
            seed,
            gangs: (0..nf)
                .filter(|&i| game.factions[i].kind == Kind::Gang)
                .collect(),
            presence: vec![vec![0.0; n]; nf],
            owner: vec![None; n],
            factions,
            modifiers: Vec::new(),
            day: 0,
            rng: Rng::new(seed ^ 0x6A9E_5EED),
            units_at: vec![Vec::new(); n],
            contested: 0,
            hotspots: 0,
            taken: vec![0; game.script.decisions.len()],
            saving: vec![0; nf],
            ever_hot: vec![false; n],
            series: Series::default(),
            history: None,
            player: None,
            player_actions: 0,
            dry: false,
            logs: vec![0.0; rules.metrics.len()],
            today: Sums::default(),
            period: Sums::default(),
            total: Sums::default(),
            means: vec![0.0; 2 + rules.maps.len()],
            city,
            gb,
        };
        w.place_headquarters();
        w.update_owners(false);
        w.series.columns = w.columns();
        w
    }

    fn place_headquarters(&mut self) {
        let g = self.game;
        let n = self.city.len();
        let mut chosen: Vec<usize> = Vec::new();
        for (f, def) in g.factions.iter().enumerate() {
            if def.start_presence <= 0.0 && def.start_units.is_empty() {
                continue;
            }
            let ok = |p: usize| {
                def.hq_field
                    .as_ref()
                    .map_or(true, |field| self.city.field(p, field) <= def.hq_max)
            };
            let mut cands: Vec<usize> = (0..n).filter(|&p| ok(p) && !chosen.contains(&p)).collect();
            if cands.is_empty() {
                cands = (0..n).filter(|p| !chosen.contains(p)).collect();
            }
            if cands.is_empty() {
                continue;
            }
            let hq = if chosen.is_empty() {
                cands[self.rng.below(cands.len())]
            } else {
                let dist: Vec<f64> = cands
                    .iter()
                    .map(|&p| {
                        chosen
                            .iter()
                            .map(|&c| self.city.distance(p, c))
                            .fold(f64::INFINITY, f64::min)
                    })
                    .collect();
                let far = dist.iter().copied().fold(0.0, f64::max);
                let ok: Vec<usize> = (0..cands.len())
                    .filter(|&i| dist[i] >= def.hq_spacing * far)
                    .collect();
                cands[ok[self.rng.below(ok.len())]]
            };
            chosen.push(hq);
            self.factions[f].hq = Some(hq);
            for (p, d) in around(&self.city.neighbours, hq, def.start_radius) {
                self.presence[f][p] += def.start_presence * if d == 0 { 1.0 } else { 0.6 };
            }
            for &u in &def.start_units {
                let id = self.gb.add_unit(&g.rules, u, hq, self.factions[f].account);
                self.units_at[hq].push(id);
            }
        }
    }

    /// Runs `days` days.
    pub fn run(&mut self, days: u32) {
        for _ in 0..days {
            self.step();
        }
    }

    /// Advances one day.
    pub fn step(&mut self) {
        let g = self.game;
        self.logs.iter_mut().for_each(|v| *v = 0.0);
        self.today = Sums::default();

        // 1. Modifiers.
        self.gb.factor.iter_mut().for_each(|f| *f = 1.0);
        let day = self.day;
        self.modifiers.retain(|m| m.until > day);
        for i in 0..self.modifiers.len() {
            let (mi, p) = (self.modifiers[i].modifier, self.modifiers[i].place);
            let def = &g.script.modifiers[mi];
            self.gb.factor[p] *= def.output;
            for e in &def.daily {
                self.apply(
                    e,
                    Ctx {
                        faction: self.owner[p],
                        place: Some(p),
                    },
                );
            }
        }

        // 2. GlassBox.
        glassbox::step(&g.rules, &self.city.neighbours, &mut self.gb);

        // 3. Income.
        for (f, def) in g.factions.iter().enumerate() {
            if self.factions[f].alive {
                for &(r, v) in &def.income {
                    self.gb.accounts[self.factions[f].account][r] += v;
                }
            }
        }

        // 4. Events.
        for e in &g.script.events {
            if self.day % e.every != 0 {
                continue;
            }
            let fits = |w: &World, f: Option<usize>| {
                e.who.map_or(true, |k| {
                    f.is_some_and(|f| g.factions[f].kind == k && w.factions[f].alive)
                })
            };
            match e.scope {
                EventScope::Place => {
                    for p in 0..self.city.len() {
                        let ctx = Ctx {
                            faction: self.owner[p],
                            place: Some(p),
                        };
                        if fits(self, ctx.faction) && self.check_all(&e.conds, ctx) {
                            for x in &e.effects {
                                self.apply(x, ctx);
                            }
                        }
                    }
                }
                EventScope::Faction => {
                    for f in 0..self.factions.len() {
                        let ctx = Ctx {
                            faction: Some(f),
                            place: None,
                        };
                        if self.factions[f].alive
                            && fits(self, Some(f))
                            && self.check_all(&e.conds, ctx)
                        {
                            for x in &e.effects {
                                self.apply(x, ctx);
                            }
                        }
                    }
                }
                EventScope::World => {
                    let ctx = Ctx {
                        faction: None,
                        place: None,
                    };
                    if self.check_all(&e.conds, ctx) {
                        for x in &e.effects {
                            self.apply(x, ctx);
                        }
                    }
                }
            }
        }

        // 5. Decisions.
        self.decide();

        // 6. Control.
        self.control();

        // 7. Metrics.
        self.day += 1;
        self.player_actions = 0;
        self.record();
    }

    // ---------- playing ----------

    fn is_target(&self, f: usize, t: Target, p: usize) -> bool {
        let contest = self.game.control.contest;
        match t {
            Target::None => false,
            Target::Owned => self.owner[p] == Some(f),
            Target::Adjacent => {
                self.owner[p] != Some(f)
                    && self.city.neighbours[p]
                        .iter()
                        .any(|&q| self.owner[q] == Some(f))
            }
            Target::Present => self.presence[f][p] > 0.5,
            Target::Contested => self.presence[f][p] > contest && self.rival(Some(f), p) > contest,
            Target::Any => true,
        }
    }

    /// The decisions faction `f` can take now on `place` (or, with no place, on itself), with
    /// their costs and whether it can pay them. `chance` conditions are taken as met.
    pub fn options(&mut self, f: usize, place: Option<usize>) -> Vec<Offer> {
        let g = self.game;
        let kind = g.factions[f].kind;
        let acct = self.factions[f].account;
        let mut out = Vec::new();
        self.dry = true;
        for (di, d) in g.script.decisions.iter().enumerate() {
            if d.who.is_some_and(|k| k != kind) {
                continue;
            }
            let fits = match place {
                None => d.target == Target::None,
                Some(p) => self.is_target(f, d.target, p),
            };
            if !fits {
                continue;
            }
            let ctx = Ctx {
                faction: Some(f),
                place,
            };
            if !self.check_all(&d.conds, ctx) {
                continue;
            }
            let score: f64 = d
                .weights
                .iter()
                .map(|w| w.k * self.term(&w.term, ctx))
                .sum();
            out.push(Offer {
                decision: di,
                name: d.name.clone(),
                label: d.label.clone(),
                target: place,
                affordable: d.costs.iter().all(|&(r, v)| self.gb.accounts[acct][r] >= v),
                costs: d
                    .costs
                    .iter()
                    .map(|&(r, v)| (g.rules.resources[r].id.clone(), v))
                    .collect(),
                score,
            });
        }
        self.dry = false;
        out
    }

    /// Takes a decision for faction `f`, as the AI would: it must be one of its
    /// [`options`](World::options), affordable, and (for the player) within its actions today.
    pub fn take(&mut self, f: usize, decision: usize, place: Option<usize>) -> Result<(), String> {
        let g = self.game;
        let d = g.script.decisions.get(decision).ok_or("no such decision")?;
        if self.player == Some(f) && self.player_actions >= g.factions[f].actions {
            return Err(format!(
                "no actions left today (you have {} a day)",
                g.factions[f].actions
            ));
        }
        if !self
            .options(f, place)
            .iter()
            .any(|o| o.decision == decision)
        {
            return Err(format!("{} is not possible there", d.name));
        }
        let acct = self.factions[f].account;
        if let Some(&(r, v)) = d
            .costs
            .iter()
            .find(|&&(r, v)| self.gb.accounts[acct][r] < v)
        {
            return Err(format!("{} needs {v} {}", d.name, g.rules.resources[r].id));
        }
        let ctx = Ctx {
            faction: Some(f),
            place,
        };
        for &(r, v) in &d.costs {
            self.gb.accounts[acct][r] -= v;
        }
        if self.check_all(&d.conds, ctx) {
            for e in &d.effects {
                self.apply(e, ctx);
            }
        }
        self.taken[decision] += 1;
        if self.player == Some(f) {
            self.player_actions += 1;
        }
        self.update_owners(true);
        Ok(())
    }

    // ---------- queries ----------

    /// The strongest presence at `p` of a gang other than `f`.
    pub fn rival(&self, f: Option<usize>, p: usize) -> f64 {
        self.gangs
            .iter()
            .filter(|&&h| Some(h) != f)
            .map(|&h| self.presence[h][p])
            .fold(0.0, f64::max)
    }

    /// 1 for the faction with the most territory among those of its kind.
    pub fn rank(&self, f: usize) -> usize {
        let kind = self.game.factions[f].kind;
        let t = self.factions[f].territory;
        1 + (0..self.factions.len())
            .filter(|&o| {
                o != f
                    && self.game.factions[o].kind == kind
                    && (self.factions[o].territory > t
                        || (self.factions[o].territory == t && o < f))
            })
            .count()
    }

    fn units_count(&self, p: usize, unit: Option<usize>) -> usize {
        self.units_at[p]
            .iter()
            .filter(|&&u| unit.map_or(true, |d| self.gb.units[u].def == d))
            .count()
    }

    fn check_all(&mut self, conds: &[Cond], ctx: Ctx) -> bool {
        conds.iter().all(|c| self.check(c, ctx))
    }

    fn check(&mut self, c: &Cond, ctx: Ctx) -> bool {
        let fp = ctx.faction.zip(ctx.place);
        match *c {
            Cond::Map { map, cmp, value } => ctx
                .place
                .is_some_and(|p| cmp.test(self.gb.maps[map][p], value)),
            Cond::Presence { cmp, value } => {
                fp.is_some_and(|(f, p)| cmp.test(self.presence[f][p], value))
            }
            Cond::Rival { cmp, value } => ctx
                .place
                .is_some_and(|p| cmp.test(self.rival(ctx.faction, p), value)),
            Cond::Owner(t) => ctx.place.is_some_and(|p| {
                let o = self.owner[p];
                match t {
                    OwnerTest::Me => o.is_some() && o == ctx.faction,
                    OwnerTest::Other => o.is_some() && o != ctx.faction,
                    OwnerTest::None => o.is_none(),
                    OwnerTest::Any => o.is_some(),
                }
            }),
            Cond::Units { unit, cmp, value } => ctx
                .place
                .is_some_and(|p| cmp.test(self.units_count(p, unit) as f64, value)),
            Cond::Global {
                resource,
                cmp,
                value,
            } => ctx.faction.is_some_and(|f| {
                cmp.test(self.gb.accounts[self.factions[f].account][resource], value)
            }),
            Cond::Territory { cmp, value } => ctx
                .faction
                .is_some_and(|f| cmp.test(self.factions[f].territory as f64, value)),
            Cond::Rank { cmp, value } => ctx
                .faction
                .is_some_and(|f| cmp.test(self.rank(f) as f64, value)),
            Cond::Day { cmp, value } => cmp.test(f64::from(self.day), value),
            Cond::Chance(p) => self.dry || self.rng.f64() < p,
        }
    }

    fn term(&self, t: &Term, ctx: Ctx) -> f64 {
        match *t {
            Term::Const => 1.0,
            Term::Map(m) => ctx.place.map_or(0.0, |p| self.gb.maps[m][p]),
            Term::Presence => ctx
                .faction
                .zip(ctx.place)
                .map_or(0.0, |(f, p)| self.presence[f][p]),
            Term::Rival => ctx.place.map_or(0.0, |p| self.rival(ctx.faction, p)),
            Term::Global(r) => ctx
                .faction
                .map_or(0.0, |f| self.gb.accounts[self.factions[f].account][r]),
            Term::Unowned => ctx
                .place
                .map_or(0.0, |p| if self.owner[p].is_none() { 1.0 } else { 0.0 }),
            Term::Units(u) => ctx.place.map_or(0.0, |p| self.units_count(p, u) as f64),
        }
    }

    // ---------- effects ----------

    fn apply(&mut self, e: &Effect, ctx: Ctx) {
        let g = self.game;
        let fp = ctx.faction.zip(ctx.place);
        match *e {
            Effect::Map {
                map,
                op,
                value,
                radius,
            } => {
                if let Some(p) = ctx.place {
                    let def = &g.rules.maps[map];
                    for (q, d) in around(&self.city.neighbours, p, radius) {
                        let x = if op == Op::Add {
                            value / f64::from(1 + d)
                        } else {
                            value
                        };
                        let v = &mut self.gb.maps[map][q];
                        *v = op.apply(*v, x).clamp(def.min, def.max);
                    }
                }
            }
            Effect::Presence { op, value } => {
                if let Some((f, p)) = fp {
                    let v = &mut self.presence[f][p];
                    *v = op.apply(*v, value).max(0.0);
                }
            }
            Effect::Rivals { op, value } | Effect::AllPresence { op, value } => {
                if let Some(p) = ctx.place {
                    let all = matches!(e, Effect::AllPresence { .. });
                    for i in 0..self.gangs.len() {
                        let h = self.gangs[i];
                        if all || Some(h) != ctx.faction {
                            let v = &mut self.presence[h][p];
                            *v = op.apply(*v, value).max(0.0);
                        }
                    }
                }
            }
            Effect::Turf { op, value } => {
                if let Some(f) = ctx.faction {
                    for p in 0..self.city.len() {
                        if self.owner[p] == Some(f) {
                            let v = &mut self.presence[f][p];
                            *v = op.apply(*v, value).max(0.0);
                        }
                    }
                }
            }
            Effect::Global {
                resource,
                op,
                value,
            } => {
                if let Some(f) = ctx.faction {
                    let v = &mut self.gb.accounts[self.factions[f].account][resource];
                    *v = op.apply(*v, value).max(0.0);
                }
            }
            Effect::Modifier { modifier, days } => {
                if let Some(p) = ctx.place {
                    let until = self.day + days;
                    match self
                        .modifiers
                        .iter_mut()
                        .find(|m| m.modifier == modifier && m.place == p)
                    {
                        Some(m) => m.until = m.until.max(until),
                        None => self.modifiers.push(ActiveModifier {
                            modifier,
                            place: p,
                            until,
                        }),
                    }
                }
            }
            Effect::Build { unit } => {
                if let Some((f, p)) = fp {
                    let id = self
                        .gb
                        .add_unit(&g.rules, unit, p, self.factions[f].account);
                    self.units_at[p].push(id);
                }
            }
            Effect::Seize => {
                if let Some((f, p)) = fp {
                    let mine = self.factions[f].account;
                    for &u in &self.units_at[p] {
                        let a = &mut self.gb.units[u].account;
                        if *a != 0 {
                            *a = mine;
                        }
                    }
                }
            }
            Effect::Destroy { rival_only } => {
                if let Some(p) = ctx.place {
                    let mine = ctx.faction.map(|f| self.factions[f].account);
                    let units = &mut self.gb.units;
                    self.units_at[p].retain(|&u| {
                        let kill = !rival_only || Some(units[u].account) != mine;
                        if kill {
                            units[u].alive = false;
                        }
                        !kill
                    });
                }
            }
            Effect::Log { metric, value } => self.logs[metric] += value,
        }
    }

    // ---------- decisions ----------

    fn targets(&self, f: usize, t: Target) -> Vec<Option<usize>> {
        let n = self.city.len();
        let contest = self.game.control.contest;
        match t {
            Target::None => vec![None],
            Target::Owned => (0..n)
                .filter(|&p| self.owner[p] == Some(f))
                .map(Some)
                .collect(),
            Target::Adjacent => {
                let mut mark = vec![false; n];
                for p in 0..n {
                    if self.owner[p] == Some(f) {
                        for &q in &self.city.neighbours[p] {
                            if self.owner[q] != Some(f) {
                                mark[q] = true;
                            }
                        }
                    }
                }
                (0..n).filter(|&p| mark[p]).map(Some).collect()
            }
            Target::Present => (0..n)
                .filter(|&p| self.presence[f][p] > 0.5)
                .map(Some)
                .collect(),
            Target::Contested => (0..n)
                .filter(|&p| self.presence[f][p] > contest && self.rival(Some(f), p) > contest)
                .map(Some)
                .collect(),
            Target::Any => (0..n).map(Some).collect(),
        }
    }

    /// Each faction takes up to `actions` decisions a day. Every decision it may take is scored
    /// at its best target; the AI then picks one (in proportion to the scores, or the best, by
    /// the faction's `choice`). When the pick is unaffordable, the faction saves for it: it only
    /// takes decisions that do not spend the resources it is short of, for up to `patience`
    /// decisions in a row.
    fn decide(&mut self) {
        let g = self.game;
        let mut opts: Vec<Opt> = Vec::new();
        for f in 0..self.factions.len() {
            if !self.factions[f].alive || self.player == Some(f) {
                continue;
            }
            let def = &g.factions[f];
            let acct = self.factions[f].account;
            for _ in 0..def.actions {
                opts.clear();
                for (di, d) in g.script.decisions.iter().enumerate() {
                    if d.who.is_some_and(|k| k != def.kind) || (self.day + f as u32) % d.every != 0
                    {
                        continue;
                    }
                    let affordable = d.costs.iter().all(|&(r, v)| self.gb.accounts[acct][r] >= v);
                    if !affordable && def.patience == 0 {
                        continue;
                    }
                    let mut best: Option<(f64, Option<usize>)> = None;
                    for t in self.targets(f, d.target) {
                        let ctx = Ctx {
                            faction: Some(f),
                            place: t,
                        };
                        if !self.check_all(&d.conds, ctx) {
                            continue;
                        }
                        let mut score: f64 = d
                            .weights
                            .iter()
                            .map(|w| w.k * self.term(&w.term, ctx))
                            .sum();
                        if def.noise > 0.0 {
                            score *= 1.0 + def.noise * (self.rng.f64() * 2.0 - 1.0);
                        }
                        if score > 0.0 && best.map_or(true, |b| score > b.0) {
                            best = Some((score, t));
                        }
                    }
                    if let Some((score, t)) = best {
                        opts.push((score, di, t, affordable));
                    }
                }
                let first = self.choose(def.choice, &opts, &|_| true);
                let chosen = match first {
                    None => None,
                    Some(i) if opts[i].3 => {
                        self.saving[f] = 0;
                        Some(i)
                    }
                    Some(i) if self.saving[f] < def.patience => {
                        self.saving[f] += 1;
                        let short: Vec<usize> = g.script.decisions[opts[i].1]
                            .costs
                            .iter()
                            .filter(|&&(r, v)| self.gb.accounts[acct][r] < v)
                            .map(|&(r, _)| r)
                            .collect();
                        self.choose(def.choice, &opts, &|o| {
                            o.3 && g.script.decisions[o.1]
                                .costs
                                .iter()
                                .all(|(r, _)| !short.contains(r))
                        })
                    }
                    Some(_) => {
                        self.saving[f] = 0;
                        self.choose(def.choice, &opts, &|o| o.3)
                    }
                };
                let Some(i) = chosen else { break };
                let (_, di, t, _) = opts[i];
                let d = &g.script.decisions[di];
                for &(r, v) in &d.costs {
                    self.gb.accounts[acct][r] -= v;
                }
                let ctx = Ctx {
                    faction: Some(f),
                    place: t,
                };
                for e in &d.effects {
                    self.apply(e, ctx);
                }
                self.taken[di] += 1;
            }
        }
    }

    fn choose(&mut self, choice: Choice, opts: &[Opt], ok: &dyn Fn(&Opt) -> bool) -> Option<usize> {
        let idx: Vec<usize> = (0..opts.len()).filter(|&i| ok(&opts[i])).collect();
        if idx.is_empty() {
            return None;
        }
        match choice {
            Choice::Best => idx.into_iter().fold(None, |b: Option<usize>, i| {
                if b.map_or(true, |b| opts[i].0 > opts[b].0) {
                    Some(i)
                } else {
                    b
                }
            }),
            Choice::Weighted => {
                let total: f64 = idx.iter().map(|&i| opts[i].0).sum();
                let mut r = self.rng.f64() * total;
                for &i in &idx {
                    r -= opts[i].0;
                    if r < 0.0 {
                        return Some(i);
                    }
                }
                idx.last().copied()
            }
        }
    }

    // ---------- control ----------

    fn control(&mut self) {
        let g = self.game;
        let c = &g.control;
        let vmax = g.rules.maps[g.violence_map].max;
        let mut contested = 0;
        let mut losses = vec![0.0; self.gangs.len()];
        for p in 0..self.city.len() {
            let (mut total, mut top, mut active) = (0.0, 0.0f64, 0);
            for &h in &self.gangs {
                let v = self.presence[h][p];
                if v > c.contest {
                    total += v;
                    top = top.max(v);
                    active += 1;
                }
            }
            if active >= 2 {
                contested += 1;
                for (i, &h) in self.gangs.iter().enumerate() {
                    let v = self.presence[h][p];
                    losses[i] = if v > c.contest {
                        c.attrition * (total - v)
                    } else {
                        0.0
                    };
                }
                for (i, &h) in self.gangs.iter().enumerate() {
                    self.presence[h][p] = (self.presence[h][p] - losses[i]).max(0.0);
                }
                let v = c.violence * (total - top);
                let m = &mut self.gb.maps[g.violence_map][p];
                *m = (*m + v).min(vmax);
                self.today.violence += v;
                self.today.clashes += 1.0;
            }
            let police = g.police_map.map_or(0.0, |m| self.gb.maps[m][p]);
            let keep = (1.0 - c.decay) * (1.0 - c.police_attrition * police / 100.0).max(0.0);
            for &h in &self.gangs {
                self.presence[h][p] *= keep;
            }
        }
        self.contested = contested;
        self.update_owners(true);
        let hot = c.hotspot;
        self.hotspots = 0;
        for (p, &v) in self.gb.maps[g.violence_map].iter().enumerate() {
            if v > hot {
                self.hotspots += 1;
                self.ever_hot[p] = true;
            }
        }
    }

    fn update_owners(&mut self, count: bool) {
        let g = self.game;
        let c = &g.control;
        for p in 0..self.city.len() {
            let (mut top, mut tv, mut second) = (None, 0.0, 0.0);
            for &h in &self.gangs {
                let v = self.presence[h][p];
                if v > tv {
                    second = tv;
                    tv = v;
                    top = Some(h);
                } else if v > second {
                    second = v;
                }
            }
            let cur = self.owner[p];
            let new = match cur {
                Some(o)
                    if self.presence[o][p] >= c.hold * 0.5
                        && !(top != Some(o)
                            && tv >= c.hold
                            && tv >= c.margin * self.presence[o][p]) =>
                {
                    Some(o)
                }
                _ if tv >= c.hold && tv >= c.margin * second => top,
                _ => None,
            };
            if new == cur {
                continue;
            }
            self.owner[p] = new;
            if let (Some(old), Some(winner)) = (cur, new) {
                if count {
                    self.today.captures += 1.0;
                }
                let (from, to) = (self.factions[old].account, self.factions[winner].account);
                let destroy = c.on_capture == "destroy";
                let units = &mut self.gb.units;
                self.units_at[p].retain(|&u| {
                    if units[u].account != from {
                        return true;
                    }
                    if destroy {
                        units[u].alive = false;
                        false
                    } else {
                        units[u].account = to;
                        true
                    }
                });
            }
        }
        for f in &mut self.factions {
            f.territory = 0;
        }
        for o in self.owner.iter().flatten() {
            self.factions[*o].territory += 1;
        }
        for &h in &self.gangs {
            let f = &mut self.factions[h];
            f.alive = f.territory > 0 || self.presence[h].iter().sum::<f64>() > 1.0;
        }
    }

    // ---------- metrics ----------

    pub fn gangs_alive(&self) -> usize {
        self.gangs
            .iter()
            .filter(|&&h| self.factions[h].alive)
            .count()
    }

    /// Herfindahl index of the gangs' territory (1: one gang holds everything held).
    pub fn hhi(&self) -> f64 {
        let total: usize = self.gangs.iter().map(|&h| self.factions[h].territory).sum();
        if total == 0 {
            return 0.0;
        }
        self.gangs
            .iter()
            .map(|&h| (self.factions[h].territory as f64 / total as f64).powi(2))
            .sum()
    }

    /// Share of the city held by the largest gang.
    pub fn top_share(&self) -> f64 {
        self.gangs
            .iter()
            .map(|&h| self.factions[h].territory)
            .max()
            .unwrap_or(0) as f64
            / self.city.len().max(1) as f64
    }

    /// Share of the city held by any gang.
    pub fn held(&self) -> f64 {
        self.owner.iter().filter(|o| o.is_some()).count() as f64 / self.city.len().max(1) as f64
    }

    fn map_mean(&self, m: usize) -> f64 {
        self.gb.maps[m].iter().sum::<f64>() / self.city.len().max(1) as f64
    }

    fn columns(&self) -> Vec<String> {
        let g = self.game;
        let mut c: Vec<String> = [
            "day",
            "violence",
            "clashes",
            "captures",
            "contested",
            "hotspots",
            "gangs_alive",
            "hhi",
            "top_share",
            "held",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        c.extend(g.rules.maps.iter().map(|m| m.id.clone()));
        c.extend(g.rules.metrics.iter().map(|m| format!("m.{m}")));
        for &h in &self.gangs {
            c.push(format!("territory.{}", g.factions[h].id));
        }
        for (f, def) in g.factions.iter().enumerate() {
            for (r, res) in g.rules.resources.iter().enumerate() {
                if res.scope == glassbox::Scope::Global
                    && (def.start.iter().any(|s| s.0 == r)
                        || def.income.iter().any(|s| s.0 == r)
                        || self.uses(f, r))
                {
                    c.push(format!("{}.{}", res.id, def.id));
                }
            }
        }
        c
    }

    /// Whether a faction's units produce or consume a global resource.
    fn uses(&self, f: usize, r: usize) -> bool {
        let rules = &self.game.rules;
        let by = rules.rules_by_unit();
        self.game.factions[f].start_units.iter().any(|&u| {
            by[u].iter().any(|&ri| {
                let b = &rules.unit_rules[ri].body;
                b.inputs.iter().chain(&b.outputs).any(|x| x.resource == r)
            })
        }) || self.game.factions[f].kind == Kind::Gang
            && rules
                .unit_rules
                .iter()
                .any(|ur| ur.body.outputs.iter().any(|x| x.resource == r))
    }

    fn record(&mut self) {
        let g = self.game;
        let mut metrics = self.gb.metrics.clone();
        metrics.resize(g.rules.metrics.len(), 0.0);
        for (a, b) in metrics.iter_mut().zip(&self.logs) {
            *a += b;
        }
        self.today.metrics = metrics;
        let today = self.today.clone();
        self.period.add(&today);
        self.total.add(&today);
        self.means[0] += self.contested as f64;
        self.means[1] += self.hotspots as f64;
        for m in 0..g.rules.maps.len() {
            self.means[2 + m] += self.map_mean(m);
        }
        if self.day % g.sample_every == 0 || self.day == 1 {
            let row = self.row();
            self.series.rows.push(row);
            self.period = Sums::default();
            if self.history.is_some() {
                let f = self.frame();
                if let Some(h) = &mut self.history {
                    h.push(f);
                }
            }
        }
    }

    /// Keeps a snapshot of owners and maps every time a row of the series is recorded, for
    /// replaying the game in the viewer.
    pub fn keep_history(&mut self) {
        if self.history.is_none() {
            self.history = Some(vec![self.frame()]);
        }
    }

    fn frame(&self) -> Frame {
        Frame {
            day: self.day,
            owner: self
                .owner
                .iter()
                .map(|o| o.map_or(-1, |f| f as i32))
                .collect(),
            maps: self
                .gb
                .maps
                .iter()
                .map(|m| {
                    m.iter()
                        .map(|&v| (v * 10.0).round() as f32 / 10.0)
                        .collect()
                })
                .collect(),
            territory: self.factions.iter().map(|f| f.territory).collect(),
        }
    }

    fn row(&self) -> Vec<f64> {
        let g = self.game;
        let mut r = vec![
            f64::from(self.day),
            self.period.violence,
            self.period.clashes,
            self.period.captures,
            self.contested as f64,
            self.hotspots as f64,
            self.gangs_alive() as f64,
            self.hhi(),
            self.top_share(),
            self.held(),
        ];
        r.extend((0..g.rules.maps.len()).map(|m| self.map_mean(m)));
        let mut metrics = self.period.metrics.clone();
        metrics.resize(g.rules.metrics.len(), 0.0);
        r.extend(metrics);
        r.extend(
            self.gangs
                .iter()
                .map(|&h| self.factions[h].territory as f64),
        );
        for col in &self.series.columns[r.len()..] {
            let (res, fid) = col.split_once('.').unwrap_or_default();
            let f = g.factions.iter().position(|d| d.id == fid);
            let ri = g.rules.resource(res);
            r.push(f.zip(ri).map_or(0.0, |(f, ri)| {
                self.gb.accounts[self.factions[f].account][ri]
            }));
        }
        r
    }

    /// Final and total values of a run, by name.
    pub fn summary(&self) -> BTreeMap<String, f64> {
        let g = self.game;
        let days = f64::from(self.day.max(1));
        let mut s = BTreeMap::new();
        s.insert("days".into(), f64::from(self.day));
        s.insert("places".into(), self.city.len() as f64);
        s.insert("gangs_alive".into(), self.gangs_alive() as f64);
        s.insert("hhi".into(), self.hhi());
        s.insert("top_share".into(), self.top_share());
        s.insert("held".into(), self.held());
        s.insert("violence".into(), self.total.violence);
        s.insert("clashes".into(), self.total.clashes);
        s.insert("captures".into(), self.total.captures);
        s.insert("contested".into(), self.means[0] / days);
        s.insert("hotspots".into(), self.means[1] / days);
        let places = self.city.len().max(1) as f64;
        s.insert(
            "hotspot_places".into(),
            self.ever_hot.iter().filter(|&&h| h).count() as f64,
        );
        s.insert(
            "hotspot_spread".into(),
            self.ever_hot.iter().filter(|&&h| h).count() as f64 / places,
        );
        s.insert("contested_share".into(), self.means[0] / days / places);
        s.insert("captures_share".into(), self.total.captures / places);
        for (m, def) in g.rules.maps.iter().enumerate() {
            s.insert(format!("map.{}", def.id), self.means[2 + m] / days);
        }
        for (i, m) in g.rules.metrics.iter().enumerate() {
            s.insert(
                format!("m.{m}"),
                self.total.metrics.get(i).copied().unwrap_or(0.0),
            );
        }
        for (i, d) in g.script.decisions.iter().enumerate() {
            s.insert(format!("decision.{}", d.name), self.taken[i] as f64);
        }
        s
    }
}
