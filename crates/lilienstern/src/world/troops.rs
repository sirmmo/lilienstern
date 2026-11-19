//! Troops on the map, on the same daily clock as everything else.
//!
//! Every day, after the factions have decided (and given their orders):
//!
//! 1. **March.** Troops with orders move along their route, all at once, in rounds of one
//!    border each: a troop has `speed` to spend a day, and entering a place costs the distance
//!    (one per border, or km) times the terrain's cost for its movement type. A troop that
//!    enters a place with enemy troops stops there. Troops cannot enter terrain their movement
//!    type has no cost for, nor places held by a faction they are at peace with.
//! 2. **Battle.** Where troops of factions at war share a place, each troop hits the enemy it
//!    does the most damage to: its damage table times `damage_scale` times its health, less the
//!    target's cover (defence stars from terrain, forts and home ground, times
//!    `defense_per_star`, times the target's health). All hits are worked out first and then
//!    dealt, so the order of troops does not matter. Troops at 0 are destroyed.
//! 3. **Occupation.** A troop in a place without enemy troops adds `occupy` (times its health,
//!    divided by 1 + the fort map there) to its faction's presence there and takes as much from
//!    the presence of enemies; control then decides who holds the place. Allies share places and
//!    never take each other's.
//! 4. **Supply.** Troops lose `unpaid` health times the share of their upkeep their faction
//!    could not pay today (it pays first thing, after its income and events and before it
//!    decides anything: see [`World::pay`]). Troops in friendly places without enemies heal
//!    `heal`. Nothing caps an army but this: a realm may raise more troops than its land can
//!    feed, and empty its treasury keeping them.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use super::World;
use crate::war::Relation;

#[derive(Clone, Debug, PartialEq)]
pub struct Troop {
    /// Stable for the whole game.
    pub id: u32,
    /// Index into the game's troop kinds.
    pub kind: usize,
    pub faction: usize,
    pub place: usize,
    /// 0 to 100.
    pub hp: f64,
    /// Places still to enter, the next first.
    pub path: Vec<usize>,
    /// How much of the way into `path[0]` is done, in cost units.
    pub progress: f64,
}

/// A way to a place for a troop: the places it will enter, and what it costs.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub path: Vec<usize>,
    pub cost: f64,
    /// Days to get there at the troop's speed.
    pub days: u32,
}

/// A place and its cost, for a min-heap.
struct Item(f64, usize);

impl PartialEq for Item {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

impl World<'_> {
    pub(super) fn refresh_relations(&mut self) {
        self.at_war = self.holders.iter().all(|&a| {
            self.holders
                .iter()
                .all(|&b| a == b || self.relation[a][b] == Relation::War)
        });
    }

    /// Sets the relation between two factions (both ways).
    pub fn set_relation(&mut self, a: usize, b: usize, r: Relation) {
        if a == b {
            return;
        }
        self.relation[a][b] = r;
        self.relation[b][a] = r;
        self.refresh_relations();
    }

    /// Whether two holders at war both have more than the control's `contest` at `p`.
    pub(super) fn clash_here(&self, p: usize) -> bool {
        let c = self.game.control.contest;
        let active: Vec<usize> = self
            .holders
            .iter()
            .copied()
            .filter(|&h| self.presence[h][p] > c)
            .collect();
        active
            .iter()
            .any(|&a| active.iter().any(|&b| self.hostile(a, b)))
    }

    /// A new troop of kind `kind` for faction `f` at `p`, at full health.
    pub fn raise(&mut self, f: usize, kind: usize, p: usize) -> u32 {
        let id = self.next_troop;
        self.next_troop += 1;
        self.troops.push(Troop {
            id,
            kind,
            faction: f,
            place: p,
            hp: 100.0,
            path: Vec::new(),
            progress: 0.0,
        });
        self.strength[f][p] += 1.0;
        self.today.raised += 1.0;
        let k = self.bonus[f][kind].upkeep;
        for &(r, v) in &self.game.war.troops[kind].upkeep {
            self.due[f][r] += v * k;
        }
        id
    }

    /// Disbands faction `f`'s weakest troop at `p` (the newest of the weakest), if it has one.
    pub fn disband(&mut self, f: usize, p: usize) -> bool {
        let Some(i) = (0..self.troops.len())
            .filter(|&i| self.troops[i].faction == f && self.troops[i].place == p)
            .min_by(|&a, &b| {
                let (x, y) = (&self.troops[a], &self.troops[b]);
                x.hp.total_cmp(&y.hp).then(y.id.cmp(&x.id))
            })
        else {
            return false;
        };
        let t = self.troops.remove(i);
        self.strength[f][p] = (self.strength[f][p] - t.hp / 100.0).max(0.0);
        self.refresh_due();
        true
    }

    /// What each faction's troops cost a day, with its techs.
    pub(super) fn refresh_due(&mut self) {
        let war = &self.game.war;
        for d in &mut self.due {
            d.iter_mut().for_each(|v| *v = 0.0);
        }
        for t in &self.troops {
            let k = self.bonus[t.faction][t.kind].upkeep;
            for &(r, v) in &war.troops[t.kind].upkeep {
                self.due[t.faction][r] += v * k;
            }
        }
    }

    pub fn troop(&self, id: u32) -> Option<&Troop> {
        self.troops.iter().find(|t| t.id == id)
    }

    fn edge(&self, a: usize, b: usize) -> f64 {
        if self.game.war.km {
            self.city.distance(a, b) / 1000.0
        } else {
            1.0
        }
    }

    /// Whether `f` must stay out of `q` (held by a faction it is at peace with).
    fn blocked(&self, f: usize, q: usize) -> bool {
        self.owner[q].is_some_and(|o| o != f && self.relation[f][o] == Relation::Peace)
    }

    /// What it costs troops of faction `f` moving by `mt` to enter `q` from `p`.
    fn enter_cost(&self, f: usize, mt: usize, p: usize, q: usize) -> Option<f64> {
        if self.blocked(f, q) {
            return None;
        }
        let k = match self.terrain[q] {
            Some(t) => self.game.war.terrains[t].cost.get(mt).copied().flatten()?,
            None => 1.0,
        };
        Some(self.edge(p, q) * k)
    }

    /// The cost from every place to `to` for troops of `f` moving by `mt`, and the next place
    /// on the way.
    fn towards(&self, f: usize, mt: usize, to: usize) -> (Vec<f64>, Vec<usize>) {
        let n = self.city.len();
        let mut dist = vec![f64::INFINITY; n];
        let mut next = vec![usize::MAX; n];
        let mut heap = BinaryHeap::new();
        dist[to] = 0.0;
        heap.push(Item(0.0, to));
        while let Some(Item(d, q)) = heap.pop() {
            if d > dist[q] {
                continue;
            }
            for &p in &self.city.neighbours[q] {
                if let Some(c) = self.enter_cost(f, mt, p, q) {
                    let nd = d + c;
                    if nd < dist[p] {
                        dist[p] = nd;
                        next[p] = q;
                        heap.push(Item(nd, p));
                    }
                }
            }
        }
        (dist, next)
    }

    fn follow(next: &[usize], from: usize, to: usize) -> Vec<usize> {
        let mut path = Vec::new();
        let mut p = from;
        while p != to && next[p] != usize::MAX {
            p = next[p];
            path.push(p);
        }
        path
    }

    /// The cheapest way for troop `id` to `to`, if there is one.
    pub fn route(&self, id: u32, to: usize) -> Option<Route> {
        let t = self.troop(id)?;
        if to >= self.city.len() {
            return None;
        }
        let def = &self.game.war.troops[t.kind];
        let (dist, next) = self.towards(t.faction, def.move_type, to);
        let cost = dist[t.place];
        if !cost.is_finite() {
            return None;
        }
        // Part of the first step may be done already.
        let mut path = Self::follow(&next, t.place, to);
        let mut cost = cost;
        if t.progress > 0.0 && t.path.first() == path.first() {
            cost -= t.progress;
        }
        if to == t.place {
            path.clear();
            cost = 0.0;
        }
        let speed = def.speed * self.bonus[t.faction][t.kind].speed;
        Some(Route {
            days: (cost / speed).ceil().max(0.0) as u32,
            path,
            cost,
        })
    }

    /// Orders troop `id` of faction `f` to `to` (its own place: to stop).
    pub fn order(&mut self, f: usize, id: u32, to: usize) -> Result<Route, String> {
        let i = self
            .troops
            .iter()
            .position(|t| t.id == id)
            .ok_or("there is no such troop")?;
        if self.troops[i].faction != f {
            return Err("that troop is not yours".into());
        }
        let r = self.route(id, to).ok_or("your troop cannot get there")?;
        let t = &mut self.troops[i];
        if t.path.first() != r.path.first() {
            t.progress = 0.0;
        }
        t.path = r.path.clone();
        Ok(r)
    }

    /// Sends up to `count` (0: all) of `f`'s troops without orders, and not fighting, to `to`:
    /// the ones that get there soonest.
    pub(super) fn march(&mut self, f: usize, to: usize, count: u32) {
        let war = &self.game.war;
        let idle: Vec<usize> = (0..self.troops.len())
            .filter(|&i| {
                let t = &self.troops[i];
                t.faction == f
                    && t.path.is_empty()
                    && t.place != to
                    && self.foes(Some(f), t.place) <= 0.0
            })
            .collect();
        if idle.is_empty() {
            return;
        }
        let mut cands: Vec<(f64, u32, usize, Vec<usize>)> = Vec::new();
        for mt in 0..war.moves.len() {
            let mine: Vec<usize> = idle
                .iter()
                .copied()
                .filter(|&i| war.troops[self.troops[i].kind].move_type == mt)
                .collect();
            if mine.is_empty() {
                continue;
            }
            let (dist, next) = self.towards(f, mt, to);
            for i in mine {
                let t = &self.troops[i];
                if dist[t.place].is_finite() {
                    let eta =
                        dist[t.place] / (war.troops[t.kind].speed * self.bonus[f][t.kind].speed);
                    cands.push((eta, t.id, i, Self::follow(&next, t.place, to)));
                }
            }
        }
        cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let take = if count == 0 {
            cands.len()
        } else {
            count as usize
        };
        for (_, _, i, path) in cands.into_iter().take(take) {
            self.troops[i].path = path;
            self.troops[i].progress = 0.0;
        }
    }

    pub(super) fn rebuild_strength(&mut self) {
        for s in &mut self.strength {
            s.iter_mut().for_each(|v| *v = 0.0);
        }
        for t in &self.troops {
            self.strength[t.faction][t.place] += t.hp / 100.0;
        }
    }

    pub(super) fn war(&mut self) {
        self.advance();
        self.rebuild_strength();
        self.battle();
        self.rebuild_strength();
        self.occupy();
        self.supply();
        self.rebuild_strength();
    }

    fn advance(&mut self) {
        let war = &self.game.war;
        let mut budget: Vec<f64> = self
            .troops
            .iter()
            .map(|t| {
                if t.path.is_empty() {
                    0.0
                } else {
                    war.troops[t.kind].speed * self.bonus[t.faction][t.kind].speed
                }
            })
            .collect();
        let mut moved = Vec::new();
        loop {
            moved.clear();
            for (i, left) in budget.iter_mut().enumerate() {
                if *left <= 0.0 || self.troops[i].path.is_empty() {
                    continue;
                }
                let t = &self.troops[i];
                let cost =
                    self.enter_cost(t.faction, war.troops[t.kind].move_type, t.place, t.path[0]);
                let t = &mut self.troops[i];
                match cost {
                    None => {
                        t.path.clear();
                        t.progress = 0.0;
                    }
                    Some(c) => {
                        let need = (c - t.progress).max(0.0);
                        if *left >= need {
                            *left -= need;
                            t.progress = 0.0;
                            t.place = t.path.remove(0);
                            moved.push(i);
                        } else {
                            t.progress += *left;
                            *left = 0.0;
                        }
                    }
                }
            }
            if moved.is_empty() {
                break;
            }
            // Troops that walked into enemies stop there.
            for &i in &moved {
                let (f, p) = (self.troops[i].faction, self.troops[i].place);
                let met = self
                    .troops
                    .iter()
                    .any(|o| o.place == p && self.hostile(f, o.faction));
                if met {
                    let t = &mut self.troops[i];
                    t.path.clear();
                    t.progress = 0.0;
                    budget[i] = 0.0;
                }
            }
        }
    }

    /// Defence stars of a troop of faction `f` and kind `kind` at `p`.
    fn stars(&self, f: usize, kind: usize, p: usize) -> f64 {
        let war = &self.game.war;
        let mut s =
            self.terrain[p].map_or(0.0, |t| war.terrains[t].defense) + self.bonus[f][kind].defense;
        if let Some(m) = war.fort_map {
            s += self.gb.maps[m][p];
        }
        if self.owner[p].is_some_and(|o| self.relation[f][o] == Relation::Ally) {
            s += war.home_defense;
        }
        s
    }

    fn battle(&mut self) {
        let g = self.game;
        let war = &g.war;
        let n = self.troops.len();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| (self.troops[i].place, i));
        let mut dmg = vec![0.0; n];
        let mut start = 0;
        while start < n {
            let p = self.troops[order[start]].place;
            let mut end = start;
            while end < n && self.troops[order[end]].place == p {
                end += 1;
            }
            let group = &order[start..end];
            start = end;
            if group.len() < 2 {
                continue;
            }
            let mut dealt = 0.0;
            for &i in group {
                let a = &self.troops[i];
                let da = &war.troops[a.kind];
                let mut best: Option<(f64, usize)> = None;
                for &j in group {
                    let b = &self.troops[j];
                    let base = da.damage[b.kind];
                    if base > 0.0
                        && self.hostile(a.faction, b.faction)
                        && best.map_or(true, |(v, _)| base > v)
                    {
                        best = Some((base, j));
                    }
                }
                let Some((base, j)) = best else { continue };
                let roll = if war.luck > 0.0 {
                    self.rng.f64() * war.luck
                } else {
                    0.0
                };
                let b = &self.troops[j];
                let cover = (self.stars(b.faction, b.kind, p) * war.defense_per_star * b.hp
                    / 100.0)
                    .clamp(0.0, 100.0);
                let a = &self.troops[i];
                let d = (base + roll) * war.damage_scale * a.hp / 100.0
                    * self.bonus[a.faction][a.kind].damage
                    * (100.0 - cover)
                    / 100.0;
                dmg[j] += d;
                dealt += d;
            }
            if dealt > 0.0 {
                self.today.battles += 1.0;
                if war.violence != 0.0 {
                    let vm = g.violence_map;
                    let max = g.rules.maps[vm].max;
                    let v = &mut self.gb.maps[vm][p];
                    *v = (*v + war.violence * dealt).min(max);
                    self.today.violence += war.violence * dealt;
                }
            }
        }
        for (t, d) in self.troops.iter_mut().zip(&dmg) {
            let loss = d.min(t.hp);
            t.hp -= loss;
            self.today.losses += loss;
        }
        self.bury();
    }

    /// Removes troops with no health left.
    fn bury(&mut self) {
        let before = self.troops.len();
        self.troops.retain(|t| t.hp >= 0.5);
        self.today.destroyed += (before - self.troops.len()) as f64;
    }

    fn occupy(&mut self) {
        let war = &self.game.war;
        for i in 0..self.troops.len() {
            let t = &self.troops[i];
            let (f, p) = (t.faction, t.place);
            // Castles hold out longer.
            let fort = war.fort_map.map_or(0.0, |m| self.gb.maps[m][p].max(0.0));
            let k = war.troops[t.kind].occupy * self.bonus[f][t.kind].occupy * t.hp
                / 100.0
                / (1.0 + fort);
            if k <= 0.0 || self.foes(Some(f), p) > 0.0 {
                continue;
            }
            if self.owner[p].is_some_and(|o| o != f && !self.hostile(f, o)) {
                continue;
            }
            if self.game.factions[f].kind.holds() {
                self.presence[f][p] += k;
            }
            for hi in 0..self.holders.len() {
                let h = self.holders[hi];
                if self.hostile(f, h) {
                    let v = &mut self.presence[h][p];
                    *v = (*v - k).max(0.0);
                }
            }
        }
    }

    /// Each faction pays its troops' upkeep, as much of it as it has (the share it could not
    /// pay, for the worst of its resources, is kept for [`World::supply`]).
    pub(super) fn pay(&mut self) {
        let nf = self.factions.len();
        let nr = self.game.rules.resources.len();
        self.refresh_due();
        let mut army = vec![0u32; nf];
        for t in &self.troops {
            army[t.faction] += 1;
        }
        // The share of the upkeep each faction could not pay (the worst of its resources).
        self.short.iter_mut().for_each(|s| *s = 0.0);
        self.source = super::SRC_UPKEEP;
        for (f, &n) in army.iter().enumerate() {
            let acct = self.factions[f].account;
            for r in 0..nr {
                let d = self.due[f][r];
                if d <= 0.0 {
                    continue;
                }
                let have = &mut self.gb.accounts[acct][r];
                let paid = d.min(have.max(0.0));
                *have -= paid;
                self.short[f] = self.short[f].max((d - paid) / d);
                self.book(f, r, -paid, None, n);
            }
            self.unpaid[f] = self.short[f] > 0.0;
            if self.unpaid[f] {
                self.today.unpaid += 1.0;
            }
        }
        self.source = super::SRC_OTHER;
    }

    fn supply(&mut self) {
        let war = &self.game.war;
        for i in 0..self.troops.len() {
            let (f, p) = (self.troops[i].faction, self.troops[i].place);
            let home = self.owner[p].is_some_and(|o| self.relation[f][o] == Relation::Ally);
            let heal = if home && self.foes(Some(f), p) <= 0.0 {
                war.heal
            } else {
                0.0
            };
            let lose = war.unpaid * self.short[f];
            let t = &mut self.troops[i];
            t.hp = (t.hp + heal - lose).min(100.0);
        }
        self.bury();
    }

    /// Where every troop is: (id, faction, kind, place, next place or -1, share of the way
    /// there, health), as in a replay frame.
    pub fn troop_positions(&self) -> Vec<(u32, u16, u16, u32, i32, f32, u8)> {
        let war = &self.game.war;
        self.troops
            .iter()
            .map(|t| {
                let (next, share) = match t.path.first() {
                    Some(&q) => {
                        let c = self
                            .enter_cost(t.faction, war.troops[t.kind].move_type, t.place, q)
                            .unwrap_or(1.0);
                        (q as i32, (t.progress / c.max(1e-9)).clamp(0.0, 1.0) as f32)
                    }
                    None => (-1, 0.0),
                };
                (
                    t.id,
                    t.faction as u16,
                    t.kind as u16,
                    t.place as u32,
                    next,
                    share,
                    t.hp.round() as u8,
                )
            })
            .collect()
    }
}
