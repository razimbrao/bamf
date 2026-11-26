//! Minimal parser for flat `key = value` config files (a tiny TOML subset).
//!
//! Supported: comments starting with `#`, blank lines, bare or double-quoted
//! string values, and integers. Sections and arrays are not supported.

use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug, Default)]
pub struct Config {
    values: HashMap<String, String>,
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut values = HashMap::new();
        for (n, raw) in text.lines().enumerate() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("line {}: expected `key = value`", n + 1))?;
            let key = key.trim();
            if key.is_empty() {
                return Err(format!("line {}: empty key", n + 1));
            }
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value);
            values.insert(key.to_string(), value.to_string());
        }
        Ok(Self { values })
    }

    pub fn load(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
        Self::parse(&text).map_err(|e| format!("{path}: {e}"))
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn required(&self, key: &str) -> Result<&str, String> {
        self.str(key)
            .ok_or_else(|| format!("missing config key `{key}`"))
    }

    pub fn parse_or<T: FromStr>(&self, key: &str, default: T) -> Result<T, String> {
        match self.str(key) {
            None => Ok(default),
            Some(v) => v
                .parse()
                .map_err(|_| format!("invalid value for `{key}`: {v}")),
        }
    }

    /// Like [`Config::parse_or`], but `None` when the key is absent.
    pub fn parse_opt<T: FromStr>(&self, key: &str) -> Result<Option<T>, String> {
        self.str(key)
            .map(|v| {
                v.parse()
                    .map_err(|_| format!("invalid value for `{key}`: {v}"))
            })
            .transpose()
    }
}

fn strip_comment(line: &str) -> &str {
    let mut in_quotes = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes => return &line[..i],
            _ => {}
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_values() {
        let c = Config::parse(
            "# comment\nlisten = \"0.0.0.0:24242\" # trailing\n\nwidth=1920\nname = \"a # b\"\n",
        )
        .unwrap();
        assert_eq!(c.str("listen"), Some("0.0.0.0:24242"));
        assert_eq!(c.parse_or("width", 0u32).unwrap(), 1920);
        assert_eq!(c.parse_or("height", 1080u32).unwrap(), 1080);
        assert_eq!(c.str("name"), Some("a # b"));
        assert!(c.required("missing").is_err());
        assert!(c.parse_or::<u32>("listen", 0).is_err());
        assert_eq!(c.parse_opt::<u32>("width").unwrap(), Some(1920));
        assert_eq!(c.parse_opt::<i32>("height").unwrap(), None);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Config::parse("just text").is_err());
        assert!(Config::parse(" = 3").is_err());
    }
}
