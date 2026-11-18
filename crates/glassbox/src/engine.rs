//! One tick of GlassBox on a graph: unit rules, agents, map rules, then decay and diffusion.

use std::collections::VecDeque;

use crate::{Body, Rules, Scope};

#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub def: usize,
    pub place: usize,
    /// Account paying and earning the unit's global resources (0 is the public account).
    pub account: usize,
    /// Local and agent resources held, by resource.
    pub stock: Vec<f64>,
    /// Agent resources produced this tick, waiting to be sent.
    pub outbox: Vec<f64>,
    /// How well the unit's rules are met, smoothed (0 to 1).
    pub satisfaction: f64,
    pub alive: bool,
}

/// Everything GlassBox changes. Vectors are indexed by the ids of [`Rules`].
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub tick: u64,
    /// `maps[map][place]`.
    pub maps: Vec<Vec<f64>>,
    pub units: Vec<Unit>,
    /// `accounts[account][resource]`, global resources only.
    pub accounts: Vec<Vec<f64>>,
    /// Output factor of each place (1 by default): the host lowers it for a curfew, raises it
    /// for a boom. Inputs are not affected.
    pub factor: Vec<f64>,
    /// This tick's metrics.
    pub metrics: Vec<f64>,
    /// This tick's deliveries and agent resources nobody took, by resource.
    pub delivered: Vec<f64>,
    pub undelivered: Vec<f64>,
}

impl State {
    pub fn new(rules: &Rules, places: usize, accounts: usize) -> State {
        let mut s = State {
            tick: 0,
            maps: Vec::new(),
            units: Vec::new(),
            accounts: vec![Vec::new(); accounts.max(1)],
            factor: vec![1.0; places],
            metrics: Vec::new(),
            delivered: Vec::new(),
            undelivered: Vec::new(),
        };
        s.sync(rules, places);
        s
    }

    /// Grows the vectors to fit declarations added since the state was made.
    pub fn sync(&mut self, rules: &Rules, places: usize) {
        let nr = rules.resources.len();
        self.maps.resize(rules.maps.len(), vec![0.0; places]);
        for m in &mut self.maps {
            m.resize(places, 0.0);
        }
        self.factor.resize(places, 1.0);
        for a in &mut self.accounts {
            a.resize(nr, 0.0);
        }
        for u in &mut self.units {
            u.stock.resize(nr, 0.0);
            u.outbox.resize(nr, 0.0);
        }
        self.metrics.resize(rules.metrics.len(), 0.0);
        self.delivered.resize(nr, 0.0);
        self.undelivered.resize(nr, 0.0);
    }

    pub fn add_unit(&mut self, rules: &Rules, def: usize, place: usize, account: usize) -> usize {
        let nr = rules.resources.len();
        self.units.push(Unit {
            def,
            place,
            account,
            stock: vec![0.0; nr],
            outbox: vec![0.0; nr],
            satisfaction: 1.0,
            alive: true,
        });
        self.units.len() - 1
    }
}

/// Places within `radius` steps of `place`, with their distance, nearest first. Neighbours are
/// visited in the order the graph lists them, so the result is deterministic.
pub fn around(graph: &[Vec<usize>], place: usize, radius: u32) -> Vec<(usize, u32)> {
    let mut out = vec![(place, 0)];
    if radius == 0 {
        return out;
    }
    let mut seen = vec![false; graph.len()];
    seen[place] = true;
    let mut i = 0;
    while i < out.len() {
        let (p, d) = out[i];
        i += 1;
        if d >= radius {
            continue;
        }
        for &q in &graph[p] {
            if !seen[q] {
                seen[q] = true;
                out.push((q, d + 1));
            }
        }
    }
    out
}

/// Adds `amount / (1 + steps)` to every place within `radius` steps of `place`.
pub fn emit(graph: &[Vec<usize>], map: &mut [f64], place: usize, amount: f64, radius: u32) {
    if radius == 0 {
        map[place] += amount;
        return;
    }
    for (p, d) in around(graph, place, radius) {
        map[p] += amount / f64::from(1 + d);
    }
}

fn held(rules: &Rules, s: &State, r: usize, account: usize, unit: Option<usize>) -> f64 {
    match rules.resources[r].scope {
        Scope::Global => s.accounts[account][r],
        _ => unit.map_or(0.0, |u| s.units[u].stock[r]),
    }
}

fn take(rules: &Rules, s: &mut State, r: usize, account: usize, unit: Option<usize>, amount: f64) {
    match rules.resources[r].scope {
        Scope::Global => s.accounts[account][r] = (s.accounts[account][r] - amount).max(0.0),
        _ => {
            if let Some(u) = unit {
                s.units[u].stock[r] = (s.units[u].stock[r] - amount).max(0.0);
            }
        }
    }
}

fn give(rules: &Rules, s: &mut State, r: usize, account: usize, unit: Option<usize>, amount: f64) {
    match (rules.resources[r].scope, unit) {
        (Scope::Global, _) => s.accounts[account][r] += amount,
        (Scope::Agent, Some(u)) => s.units[u].outbox[r] += amount,
        (Scope::Local, Some(u)) => s.units[u].stock[r] += amount,
        _ => {}
    }
}

/// Runs a rule body at `place`. Returns the share of its needs that was met (0 when a test
/// failed).
fn run(
    rules: &Rules,
    graph: &[Vec<usize>],
    s: &mut State,
    b: &Body,
    place: usize,
    account: usize,
    unit: Option<usize>,
) -> f64 {
    if !b
        .map_tests
        .iter()
        .all(|t| t.cmp.test(s.maps[t.map][place], t.value))
    {
        return 0.0;
    }
    if !b
        .global_tests
        .iter()
        .all(|t| t.cmp.test(s.accounts[account][t.resource], t.value))
    {
        return 0.0;
    }
    let mut f = 1.0f64;
    for i in &b.inputs {
        if i.amount > 0.0 {
            f = f.min(held(rules, s, i.resource, account, unit) / i.amount);
        }
    }
    let f = f.clamp(0.0, 1.0);
    if f <= 0.0 {
        return 0.0;
    }
    for i in &b.inputs {
        take(rules, s, i.resource, account, unit, i.amount * f);
    }
    let out = f * s.factor[place];
    for o in &b.outputs {
        give(rules, s, o.resource, account, unit, o.amount * out);
    }
    for m in &b.map_in {
        let floor = rules.maps[m.map].min;
        for (p, d) in around(graph, place, m.radius) {
            let v = &mut s.maps[m.map][p];
            *v -= (m.amount * f / f64::from(1 + d)).min(*v - floor).max(0.0);
        }
    }
    for m in &b.map_out {
        emit(graph, &mut s.maps[m.map], place, m.amount * out, m.radius);
    }
    for &(k, v) in &b.logs {
        s.metrics[k] += v * out;
    }
    f
}

/// Advances the state by one tick.
pub fn step(rules: &Rules, graph: &[Vec<usize>], s: &mut State) {
    let n = graph.len();
    s.sync(rules, n);
    s.metrics.iter_mut().for_each(|m| *m = 0.0);
    s.delivered.iter_mut().for_each(|m| *m = 0.0);
    s.undelivered.iter_mut().for_each(|m| *m = 0.0);
    let by_unit = rules.rules_by_unit();

    // Units, in the order they were made.
    for i in 0..s.units.len() {
        if !s.units[i].alive {
            continue;
        }
        let (def, place, account) = (s.units[i].def, s.units[i].place, s.units[i].account);
        let (mut fired, mut met) = (0u32, 0.0);
        for &ri in &by_unit[def] {
            let rule = &rules.unit_rules[ri];
            if (s.tick + i as u64) % u64::from(rule.every.max(1)) != 0 {
                continue;
            }
            fired += 1;
            met += run(rules, graph, s, &rule.body, place, account, Some(i));
        }
        if fired > 0 {
            let u = &mut s.units[i];
            u.satisfaction = 0.8 * u.satisfaction + 0.2 * met / f64::from(fired);
        }
    }

    deliver(rules, graph, s, &by_unit);

    for (ri, rule) in rules.map_rules.iter().enumerate() {
        for p in 0..n {
            if (s.tick + (p + ri) as u64) % u64::from(rule.every.max(1)) == 0 {
                run(rules, graph, s, &rule.body, p, 0, None);
            }
        }
    }

    for (m, def) in rules.maps.iter().enumerate() {
        if def.diffuse > 0.0 {
            let old = s.maps[m].clone();
            let v = &mut s.maps[m];
            for p in 0..n {
                if graph[p].is_empty() || old[p] == 0.0 {
                    continue;
                }
                let share = old[p] * def.diffuse;
                v[p] -= share;
                let each = share / graph[p].len() as f64;
                for &q in &graph[p] {
                    v[q] += each;
                }
            }
        }
        let keep = 1.0 - def.decay;
        for x in &mut s.maps[m] {
            *x = (*x * keep).clamp(def.min, def.max);
        }
    }
    s.tick += 1;
}

/// Agents: every unit with agent resources to send gives them to the nearest units that need
/// them (breadth first along the graph, up to the resource's reach), and agent rules emit along
/// the way. A unit takes up to twice what its rules need per run, less what it holds.
fn deliver(rules: &Rules, graph: &[Vec<usize>], s: &mut State, by_unit: &[Vec<usize>]) {
    let n = graph.len();
    let mut stamp = vec![0u32; n];
    let mut parent = vec![usize::MAX; n];
    let mut depth = vec![0u32; n];
    let mut gen = 0u32;
    let mut queue = VecDeque::new();
    for r in 0..rules.resources.len() {
        let def = &rules.resources[r];
        if def.scope != Scope::Agent {
            continue;
        }
        let mut demand: Vec<f64> = s
            .units
            .iter()
            .map(|u| {
                if !u.alive {
                    return 0.0;
                }
                let need: f64 = by_unit[u.def]
                    .iter()
                    .flat_map(|&ri| rules.unit_rules[ri].body.inputs.iter())
                    .filter(|f| f.resource == r)
                    .map(|f| f.amount)
                    .sum();
                (need * 2.0 - u.stock[r]).max(0.0)
            })
            .collect();
        let mut at: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, u) in s.units.iter().enumerate() {
            if demand[i] > 0.0 {
                at[u.place].push(i);
            }
        }
        let ars: Vec<_> = rules
            .agent_rules
            .iter()
            .filter(|a| a.resource == r)
            .collect();
        for i in 0..s.units.len() {
            let mut left = s.units[i].outbox[r];
            if left <= 0.0 {
                continue;
            }
            s.units[i].outbox[r] = 0.0;
            if !s.units[i].alive {
                continue;
            }
            let (start, account) = (s.units[i].place, s.units[i].account);
            gen += 1;
            queue.clear();
            queue.push_back(start);
            stamp[start] = gen;
            parent[start] = usize::MAX;
            depth[start] = 0;
            'search: while let Some(p) = queue.pop_front() {
                for &c in &at[p] {
                    if c == i || demand[c] <= 0.0 || (def.private && s.units[c].account != account)
                    {
                        continue;
                    }
                    let amount = left.min(demand[c]);
                    demand[c] -= amount;
                    s.units[c].stock[r] += amount;
                    s.delivered[r] += amount;
                    left -= amount;
                    if !ars.is_empty() {
                        let mut path = vec![p];
                        let mut q = p;
                        while parent[q] != usize::MAX {
                            q = parent[q];
                            path.push(q);
                        }
                        path.reverse();
                        for a in &ars {
                            let every = a.every.max(1) as usize;
                            for (h, &pl) in path.iter().enumerate() {
                                if h % every == 0 {
                                    for area in &a.areas {
                                        emit(
                                            graph,
                                            &mut s.maps[area.map],
                                            pl,
                                            area.amount * amount,
                                            area.radius,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    if left <= 1e-12 {
                        break 'search;
                    }
                }
                if depth[p] < def.reach {
                    for &q in &graph[p] {
                        if stamp[q] != gen {
                            stamp[q] = gen;
                            parent[q] = p;
                            depth[q] = depth[p] + 1;
                            queue.push_back(q);
                        }
                    }
                }
            }
            if left > 1e-12 {
                s.undelivered[r] += left;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, MapDef};

    /// A line of places 0 - 1 - 2 - 3 - 4.
    fn line(n: usize) -> Vec<Vec<usize>> {
        (0..n)
            .map(|i| {
                [i.checked_sub(1), (i + 1 < n).then_some(i + 1)]
                    .into_iter()
                    .flatten()
                    .collect()
            })
            .collect()
    }

    fn setup(text: &str) -> Rules {
        let mut r = Rules::default();
        r.declare_unit("farm", "Farm");
        r.declare_unit("mill", "Mill");
        compile(&mut r, "t.gbx", text, &[]).unwrap();
        r
    }

    #[test]
    fn rules_take_give_and_gate() {
        let r = setup("unitRule farm\n global Cash in 2\n global Grain out 5\n map Soil atLeast 1\n map Dust out 1\n log Harvests 1\nend\n");
        let g = line(3);
        let mut s = State::new(&r, 3, 2);
        let cash = r.resource("Cash").unwrap();
        let grain = r.resource("Grain").unwrap();
        let soil = r.map("Soil").unwrap();
        s.accounts[1][cash] = 1.0;
        s.maps[soil][0] = 2.0;
        s.add_unit(&r, 0, 0, 1);
        s.add_unit(&r, 0, 2, 1); // no soil: gated
        step(&r, &g, &mut s);
        // Half the cash it needs: half a harvest.
        assert_eq!(s.accounts[1][grain], 2.5);
        assert_eq!(s.accounts[1][cash], 0.0);
        assert_eq!(s.metrics[0], 0.5);
        assert_eq!(s.maps[r.map("Dust").unwrap()], vec![0.5, 0.0, 0.0]);
    }

    #[test]
    fn agents_travel_to_the_nearest_taker_and_emit_on_the_way() {
        let r = setup("unitRule farm\n agent Flour out 4\nend\nunitRule mill\n agent Flour in 1\nend\nagentRule carts\n condition resource=Flour\n area Dust 1 0\nend\n");
        let g = line(5);
        let mut s = State::new(&r, 5, 1);
        s.add_unit(&r, 0, 0, 0); // farm at 0
        let near = s.add_unit(&r, 1, 2, 0); // mills at 2 and 4
        let far = s.add_unit(&r, 1, 4, 0);
        step(&r, &g, &mut s);
        let flour = r.resource("Flour").unwrap();
        // Each mill takes twice its need: 2, then 2.
        assert_eq!(s.units[near].stock[flour], 2.0);
        assert_eq!(s.units[far].stock[flour], 2.0);
        assert_eq!(s.delivered[flour], 4.0);
        // Dust per unit carried on each place passed: 2 on 0..=2, 2 on 0..=4.
        assert_eq!(
            s.maps[r.map("Dust").unwrap()],
            vec![4.0, 4.0, 4.0, 2.0, 2.0]
        );
    }

    #[test]
    fn diffusion_keeps_the_total_and_decay_lowers_it() {
        let mut r = Rules::default();
        r.maps.push(MapDef {
            diffuse: 0.5,
            ..MapDef::new("A")
        });
        r.maps.push(MapDef {
            decay: 0.5,
            ..MapDef::new("B")
        });
        let g = line(4);
        let mut s = State::new(&r, 4, 1);
        s.maps[0][1] = 8.0;
        s.maps[1][1] = 8.0;
        step(&r, &g, &mut s);
        assert_eq!(s.maps[0], vec![2.0, 4.0, 2.0, 0.0]);
        assert_eq!(s.maps[1][1], 4.0);
    }

    #[test]
    fn around_lists_places_by_distance() {
        let g = line(5);
        assert_eq!(around(&g, 2, 1), vec![(2, 0), (1, 1), (3, 1)]);
        assert_eq!(around(&g, 0, 9).len(), 5);
    }
}
