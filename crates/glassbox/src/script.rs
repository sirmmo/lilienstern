//! GlassBox scripts: blocks from `<kind> <name>` to `end`, one statement per line.
//!
//! ```text
//! unitRule mustardFactory          # what a unit does
//!     appliesTo factory            # unit ids (defaults to the rule name)
//!     rate 10                      # every 10 ticks
//!     global Simoleans in 1        # from the unit's account
//!     local Mustard in 6           # needed, held by the unit
//!     agent Bottles out 1          # sent to units that need them
//!     map Pollution out 5 radius 3 # emitted around the unit
//!     map LandValue atLeast 2      # test at the unit's place
//!     log -timestamp=Time Output 1 # metric
//! end
//!
//! mapRule growGrass                # every place
//!     map Soil atLeast 20
//!     map Water in 10
//!     map Grass out 5
//! end
//!
//! agentRule fumes                  # every place an agent passes
//!     condition resource=Bottles
//!     repeatAfter 2
//!     area Pollution 0.5 1         # map, amount per unit carried, radius
//! end
//! ```
//!
//! `#` and `//` start comments. Blocks of other kinds can be handed back to the caller, which
//! is how a host engine adds its own (Lilienstern's events and decisions).

use crate::{
    AgentRule, Body, Cmp, Error, Flow, GlobalTest, MapFlow, MapRule, MapTest, Rules, Scope,
    UnitRule,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub no: usize,
    pub words: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: String,
    pub name: String,
    /// Line of the block's header.
    pub no: usize,
    pub lines: Vec<Line>,
}

/// Block kinds compiled by GlassBox. `zoneRule` is recognised and ignored: a graph has no zones.
pub const KINDS: [&str; 5] = ["rule", "unitRule", "mapRule", "agentRule", "zoneRule"];

pub fn tokenize(text: &str) -> Vec<Line> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let l = l
                .split('#')
                .next()
                .unwrap_or("")
                .split("//")
                .next()
                .unwrap_or("");
            let words: Vec<String> = l.split_whitespace().map(str::to_string).collect();
            (!words.is_empty()).then_some(Line { no: i + 1, words })
        })
        .collect()
}

pub fn split_blocks(file: &str, lines: Vec<Line>, kinds: &[&str]) -> Result<Vec<Block>, Error> {
    let mut out = Vec::new();
    let mut cur: Option<Block> = None;
    for line in lines {
        let head = line.words[0].as_str();
        match (&mut cur, head) {
            (None, k) if kinds.contains(&k) => {
                let name = line
                    .words
                    .get(1)
                    .cloned()
                    .ok_or_else(|| Error::new(file, line.no, format!("{head} needs a name")))?;
                cur = Some(Block {
                    kind: head.to_string(),
                    name,
                    no: line.no,
                    lines: Vec::new(),
                });
            }
            (None, _) => {
                return Err(Error::new(
                    file,
                    line.no,
                    format!("expected one of {}, found {head:?}", kinds.join(", ")),
                ))
            }
            (Some(_), "end") => out.push(cur.take().expect("open block")),
            (Some(b), k) if kinds.contains(&k) && line.words.len() == 2 && !matches!(k, "rule") => {
                return Err(Error::new(
                    file,
                    line.no,
                    format!("{} {} is missing `end` before this {k}", b.kind, b.name),
                ));
            }
            (Some(b), _) => b.lines.push(line),
        }
    }
    if let Some(b) = cur {
        return Err(Error::new(
            file,
            b.no,
            format!("{} {} is missing `end`", b.kind, b.name),
        ));
    }
    Ok(out)
}

/// The number at `words[i]`.
pub fn number(file: &str, line: &Line, i: usize) -> Result<f64, Error> {
    line.words
        .get(i)
        .and_then(|w| w.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .ok_or_else(|| {
            let after = line.words[..i.min(line.words.len())].join(" ");
            Error::new(file, line.no, format!("expected a number after {after:?}"))
        })
}

/// `-key=value` or `-key value`.
pub fn option(words: &[String], key: &str) -> Option<String> {
    let flag = format!("-{key}");
    let eq = format!("{flag}=");
    let mut it = words.iter();
    while let Some(w) = it.next() {
        if let Some(v) = w.strip_prefix(&eq) {
            return Some(v.to_string());
        }
        if *w == flag {
            return it.next().cloned();
        }
    }
    None
}

/// The words that are not `-options` (nor their values).
pub fn plain(words: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for w in words {
        if skip {
            skip = false;
            continue;
        }
        if w.starts_with('-') && w.parse::<f64>().is_err() {
            skip = !w.contains('=')
                && matches!(
                    w.as_str(),
                    "-sendTo" | "-via" | "-using" | "-count" | "-id" | "-map" | "-timestamp"
                );
            continue;
        }
        out.push(w.as_str());
    }
    out
}

/// Compiles the GlassBox blocks of a script into `rules`. Blocks whose kind is in `extra` are
/// returned for the caller to compile. Also returns warnings about ignored statements.
pub fn compile(
    rules: &mut Rules,
    file: &str,
    text: &str,
    extra: &[&str],
) -> Result<(Vec<String>, Vec<Block>), Error> {
    let mut kinds: Vec<&str> = KINDS.to_vec();
    kinds.extend_from_slice(extra);
    let mut warnings = Vec::new();
    let mut rest = Vec::new();
    for b in split_blocks(file, tokenize(text), &kinds)? {
        if KINDS.contains(&b.kind.as_str()) {
            compile_block(rules, file, &b, &mut warnings)?;
        } else {
            rest.push(b);
        }
    }
    Ok((warnings, rest))
}

fn rate(file: &str, b: &Block) -> Result<u32, Error> {
    let mut every = 1;
    for l in &b.lines {
        if matches!(l.words[0].as_str(), "rate" | "repeatAfter") {
            let n = number(file, l, 1)?;
            if n < 1.0 {
                return Err(Error::new(
                    file,
                    l.no,
                    format!("{} must be at least 1", l.words[0]),
                ));
            }
            every = n.round() as u32;
        }
    }
    Ok(every)
}

fn compile_block(
    rules: &mut Rules,
    file: &str,
    b: &Block,
    warnings: &mut Vec<String>,
) -> Result<(), Error> {
    let every = rate(file, b)?;
    match b.kind.as_str() {
        "rule" | "unitRule" => {
            let mut body = Body::default();
            let mut applies: Vec<String> = Vec::new();
            for l in &b.lines {
                let w = &l.words;
                match w[0].as_str() {
                    "rate" => {}
                    "appliesTo" => {
                        for x in &w[1..] {
                            if x == "via" || x == "using" {
                                break;
                            }
                            applies.push(x.clone());
                        }
                    }
                    "options" | "successEvent" | "onFail" => {
                        warnings.push(format!("{file}:{}: {} is not simulated", l.no, w[0]))
                    }
                    "test" => statement(rules, file, l, 1, &mut body, true)?,
                    _ => statement(rules, file, l, 0, &mut body, true)?,
                }
            }
            if applies.is_empty() {
                applies.push(b.name.clone());
            }
            let mut units = Vec::new();
            for a in applies {
                let u = rules.unit(&a).ok_or_else(|| {
                    Error::new(
                        file,
                        b.no,
                        format!("{} {}: there is no unit {a:?}", b.kind, b.name),
                    )
                })?;
                units.push(u);
            }
            rules.unit_rules.push(UnitRule {
                name: b.name.clone(),
                units,
                every,
                body,
            });
        }
        "mapRule" => {
            let mut body = Body::default();
            for l in &b.lines {
                match l.words[0].as_str() {
                    "rate" => {}
                    "successEvent" | "onFail" => {
                        warnings.push(format!("{file}:{}: {} is not simulated", l.no, l.words[0]))
                    }
                    "test" => statement(rules, file, l, 1, &mut body, false)?,
                    _ => statement(rules, file, l, 0, &mut body, false)?,
                }
            }
            rules.map_rules.push(MapRule {
                name: b.name.clone(),
                every,
                body,
            });
        }
        "agentRule" => {
            let mut resource = None;
            let mut areas = Vec::new();
            for l in &b.lines {
                let w = &l.words;
                match w[0].as_str() {
                    "repeatAfter" => {}
                    "condition" => {
                        for x in &w[1..] {
                            if let Some(v) = x.strip_prefix("resource=") {
                                resource = Some(
                                    rules
                                        .declare_resource(v, Scope::Agent)
                                        .map_err(|m| Error::new(file, l.no, m))?,
                                );
                            }
                        }
                    }
                    "area" => {
                        let p = plain(w);
                        // `area -map=AirQuality pm10 1 5` (map AirQuality.pm10) or `area Heat 1 5`.
                        let (map, rest) = match option(w, "map") {
                            Some(m) => (
                                format!("{m}.{}", p.get(1).copied().unwrap_or("")),
                                p.get(2..).unwrap_or(&[]),
                            ),
                            None => (
                                p.get(1).copied().unwrap_or("").to_string(),
                                p.get(2..).unwrap_or(&[]),
                            ),
                        };
                        if map.is_empty() || map.ends_with('.') {
                            return Err(Error::new(file, l.no, "area needs a map"));
                        }
                        let amount = rest
                            .first()
                            .and_then(|v| v.parse::<f64>().ok())
                            .ok_or_else(|| Error::new(file, l.no, "area needs an amount"))?;
                        let radius = rest
                            .get(1)
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(0.0)
                            .max(0.0) as u32;
                        areas.push(MapFlow {
                            map: rules.declare_map(&map),
                            amount,
                            radius,
                        });
                    }
                    other => warnings.push(format!("{file}:{}: {other} is not simulated", l.no)),
                }
            }
            let resource = resource.ok_or_else(|| {
                Error::new(
                    file,
                    b.no,
                    format!("agentRule {} needs `condition resource=<R>`", b.name),
                )
            })?;
            rules.agent_rules.push(AgentRule {
                name: b.name.clone(),
                resource,
                every,
                areas,
            });
        }
        "zoneRule" => warnings.push(format!(
            "{file}:{}: zoneRule {} ignored: places have no zones",
            b.no, b.name
        )),
        _ => unreachable!(),
    }
    Ok(())
}

/// One statement of a rule body; `off` skips a leading `test`.
fn statement(
    rules: &mut Rules,
    file: &str,
    l: &Line,
    off: usize,
    body: &mut Body,
    resources: bool,
) -> Result<(), Error> {
    let w = &l.words[off..];
    let e = |m: String| Error::new(file, l.no, m);
    let at = |i: usize| -> Result<&str, Error> {
        w.get(i)
            .map(String::as_str)
            .ok_or_else(|| e(format!("{} is incomplete", w.join(" "))))
    };
    let num = |i: usize| number(file, l, off + i);
    let radius = || -> Result<u32, Error> {
        match w.iter().position(|x| x == "radius") {
            Some(i) => Ok(number(file, l, off + i + 1)?.max(0.0) as u32),
            None => Ok(0),
        }
    };
    let head = w[0].as_str();
    match head {
        "map" => {
            let map = rules.declare_map(at(1)?);
            let op = at(2)?;
            if let Some(cmp) = Cmp::parse(op) {
                body.map_tests.push(MapTest {
                    map,
                    cmp,
                    value: num(3)?,
                });
            } else if op == "in" || op == "out" {
                if off > 0 {
                    return Err(e("`test` only takes conditions".into()));
                }
                let f = MapFlow {
                    map,
                    amount: num(3)?,
                    radius: radius()?,
                };
                if op == "in" {
                    body.map_in.push(f);
                } else {
                    body.map_out.push(f);
                }
            } else {
                return Err(e(format!(
                    "map {}: expected in, out, atLeast, atMost, greater, less or is, found {op:?}",
                    w[1]
                )));
            }
        }
        "local" | "global" | "agent" if resources => {
            let scope = Scope::parse(head).expect("scope");
            let id = at(1)?;
            let op = at(2)?;
            if let Some(cmp) = Cmp::parse(op) {
                if scope != Scope::Global {
                    return Err(e(format!(
                        "{head} {id}: only global resources can be tested"
                    )));
                }
                let resource = rules.declare_resource(id, scope).map_err(e)?;
                body.global_tests.push(GlobalTest {
                    resource,
                    cmp,
                    value: num(3)?,
                });
            } else {
                flow(rules, scope, id, op, num(3)?, body).map_err(e)?;
            }
        }
        "log" => {
            let p = plain(w);
            let metric = p.get(1).ok_or_else(|| e("log needs a metric".into()))?;
            let n = match p.get(2) {
                Some(v) => v
                    .parse::<f64>()
                    .map_err(|_| e(format!("log {metric}: {v:?} is not a number")))?,
                None => 1.0,
            };
            body.logs.push((rules.metric(metric), n));
        }
        _ if resources && w.len() >= 3 && (w[1] == "in" || w[1] == "out") => {
            flow(rules, Scope::Local, head, &w[1], num(2)?, body).map_err(e)?;
        }
        _ => return Err(e(format!("unknown statement {:?}", w.join(" ")))),
    }
    Ok(())
}

fn flow(
    rules: &mut Rules,
    scope: Scope,
    id: &str,
    op: &str,
    amount: f64,
    body: &mut Body,
) -> Result<(), String> {
    let resource = rules.declare_resource(id, scope)?;
    let f = Flow { resource, amount };
    match op {
        "in" => body.inputs.push(f),
        "out" => body.outputs.push(f),
        _ => return Err(format!("{id}: expected in or out, found {op:?}")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        let mut r = Rules::default();
        r.declare_unit("factory", "Factory");
        r
    }

    #[test]
    fn compiles_the_gdc_example() {
        let mut r = rules();
        let text = "unitRule mustardFactory\n appliesTo factory\n rate 10\n global Simoleans in 1\n local YellowMustard in 6\n BottleOfMustard out 1\n agent Goods out 2\n map Pollution out 5 radius 3\n map LandValue atLeast 2\n log -timestamp=Time Shipments 1\n successEvent effect smokePuff\nend\n";
        let (warnings, rest) = compile(&mut r, "t.gbx", text, &[]).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(rest.is_empty());
        let u = &r.unit_rules[0];
        assert_eq!(u.every, 10);
        assert_eq!(u.body.inputs.len(), 2);
        assert_eq!(u.body.outputs.len(), 2);
        assert_eq!(u.body.map_out[0].radius, 3);
        assert_eq!(u.body.map_tests[0].cmp, Cmp::AtLeast);
        assert_eq!(r.metrics, vec!["Shipments"]);
        assert_eq!(
            r.resources[r.resource("Goods").unwrap()].scope,
            Scope::Agent
        );
        assert_eq!(
            r.resources[r.resource("Simoleans").unwrap()].scope,
            Scope::Global
        );
    }

    #[test]
    fn agent_rules_take_both_area_forms() {
        let mut r = rules();
        let text = "agentRule a\n condition resource=Car\n repeatAfter 10\n area -timestamp=Time -map=AirQuality pm10 1 5\n area Noise 0.8\nend\n";
        compile(&mut r, "t.gbx", text, &[]).unwrap();
        let a = &r.agent_rules[0];
        assert_eq!(a.every, 10);
        assert_eq!(r.maps[a.areas[0].map].id, "AirQuality.pm10");
        assert_eq!(a.areas[0].radius, 5);
        assert_eq!(r.maps[a.areas[1].map].id, "Noise");
    }

    #[test]
    fn errors_name_the_line() {
        let mut r = rules();
        let e = compile(
            &mut r,
            "t.gbx",
            "\n\nunitRule x\n appliesTo factory\n map Heat sideways 3\nend\n",
            &[],
        )
        .unwrap_err();
        assert_eq!(e.line, 5);
        let e = compile(
            &mut r,
            "t.gbx",
            "unitRule x\n appliesTo nowhere\nend\n",
            &[],
        )
        .unwrap_err();
        assert!(e.message.contains("nowhere"), "{e}");
        let e = compile(&mut r, "t.gbx", "unitRule x\n appliesTo factory\n", &[]).unwrap_err();
        assert!(e.message.contains("missing `end`"), "{e}");
        let e = compile(
            &mut r,
            "t.gbx",
            "unitRule x\n appliesTo factory\n local Cash in 1\n global Cash out 1\nend\n",
            &[],
        )
        .unwrap_err();
        assert!(e.message.contains("local resource"), "{e}");
    }

    #[test]
    fn hands_back_other_blocks() {
        let mut r = rules();
        let (_, rest) = compile(
            &mut r,
            "t.lil",
            "event riot\n if map Heat greater 3\nend\nmapRule m\n map A out 1\nend\n",
            &["event"],
        )
        .unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].kind, "event");
        assert_eq!(r.map_rules.len(), 1);
    }
}
