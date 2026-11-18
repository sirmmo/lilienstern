//! Parameters: named values a game declares in `params.toml` and uses as `$name` anywhere in
//! its TOML and scripts. Runs, experiments and the command line override them, so every number
//! of a game can be varied without editing it.

use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params(pub BTreeMap<String, String>);

fn value_text(key: &str, v: &toml::Value) -> Result<String, String> {
    Ok(match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{f:.1}")
            } else {
                format!("{f}")
            }
        }
        toml::Value::Boolean(b) => b.to_string(),
        _ => {
            return Err(format!(
                "parameter {key}: only numbers, strings and true/false can be parameters"
            ))
        }
    })
}

impl Params {
    pub fn from_toml(text: &str) -> Result<Params, String> {
        let table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
        Params::from_table(&table)
    }

    pub fn from_table(table: &toml::Table) -> Result<Params, String> {
        let mut p = Params::default();
        for (k, v) in table {
            p.0.insert(k.clone(), value_text(k, v)?);
        }
        Ok(p)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// Overrides values. Keys the game does not declare are an error, to catch typos.
    pub fn apply(&mut self, other: &Params) -> Result<(), String> {
        for (k, v) in &other.0 {
            if !self.0.contains_key(k) {
                let known: Vec<&str> = self.0.keys().map(String::as_str).collect();
                return Err(format!(
                    "unknown parameter {k:?} (the game has: {})",
                    known.join(", ")
                ));
            }
            self.0.insert(k.clone(), v.clone());
        }
        Ok(())
    }

    /// Parses `key=value`.
    pub fn parse_assignment(text: &str) -> Result<(String, String), String> {
        let (k, v) = text
            .split_once('=')
            .ok_or_else(|| format!("expected key=value, found {text:?}"))?;
        Ok((k.trim().to_string(), v.trim().to_string()))
    }

    /// Replaces `$name` with the parameter's value (`$$` is a dollar sign). An unknown name is
    /// an error, except in a `#` comment.
    pub fn substitute(&self, file: &str, text: &str) -> Result<String, String> {
        let mut out = String::with_capacity(text.len());
        for (no, line) in text.split_inclusive('\n').enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let mut i = 0;
            let mut comment = false;
            let mut quoted = false;
            while i < chars.len() {
                let c = chars[i];
                if c == '"' {
                    quoted = !quoted;
                }
                if c == '#' && !quoted {
                    comment = true;
                }
                if c == '$' {
                    if chars.get(i + 1) == Some(&'$') {
                        out.push('$');
                        i += 2;
                        continue;
                    }
                    let start = i + 1;
                    let mut end = start;
                    while end < chars.len()
                        && (chars[end].is_ascii_alphanumeric() || chars[end] == '_')
                    {
                        end += 1;
                    }
                    if end > start {
                        let name: String = chars[start..end].iter().collect();
                        match self.0.get(&name) {
                            Some(v) => {
                                out.push_str(v);
                                i = end;
                                continue;
                            }
                            None if !comment => {
                                return Err(format!("{file}:{}: unknown parameter ${name}", no + 1))
                            }
                            None => {}
                        }
                    }
                }
                out.push(c);
                i += 1;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_and_overrides() {
        let mut p =
            Params::from_toml("gangs = 4\nrate = 0.5\nwhole = 2.0\nfile = \"city.geojson\"\n")
                .unwrap();
        assert_eq!(
            p.substitute(
                "f",
                "count = $gangs # $nothing\nx = \"$file\" $$5 $rate $whole"
            )
            .unwrap(),
            "count = 4 # $nothing\nx = \"city.geojson\" $5 0.5 2.0"
        );
        let e = p.substitute("f", "a\nb = $nope").unwrap_err();
        assert!(e.contains("f:2") && e.contains("$nope"), "{e}");
        let over = Params::from_toml("gangs = 8").unwrap();
        p.apply(&over).unwrap();
        assert_eq!(p.get("gangs"), Some("8"));
        assert!(p.apply(&Params::from_toml("gnags = 8").unwrap()).is_err());
    }
}
