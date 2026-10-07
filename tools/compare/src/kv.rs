//! Minimal reader and writer for the flat TOML subset the capture files use: `[section]` headers and
//! `key = value` lines, where a value is a quoted string, a number, or a bare word. Nothing else.

use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kv {
    /// Keys are `section.key`, or `key` before the first section.
    pub map: BTreeMap<String, String>,
}

impl Kv {
    pub fn parse(text: &str) -> Result<Kv, String> {
        let mut kv = Kv::default();
        let mut section = String::new();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(s) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                section = s.trim().to_string();
                continue;
            }
            let (k, v) = line.split_once('=').ok_or_else(|| format!("line {}: expected key = value", n + 1))?;
            let k = k.trim();
            let mut v = v.trim();
            if let Some(i) = v.find(" #") {
                if !v.starts_with('"') {
                    v = v[..i].trim();
                }
            }
            let v = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(v);
            let key = if section.is_empty() { k.to_string() } else { format!("{section}.{k}") };
            kv.map.insert(key, v.to_string());
        }
        Ok(kv)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    pub fn req(&self, key: &str) -> Result<&str, String> {
        self.get(key).ok_or_else(|| format!("missing key `{key}`"))
    }

    pub fn f32(&self, key: &str) -> Result<f32, String> {
        let v = self.req(key)?;
        v.parse::<f32>().map_err(|e| format!("`{key}` = `{v}`: {e}"))
    }

    pub fn u32(&self, key: &str) -> Result<u32, String> {
        let v = self.req(key)?;
        v.parse::<u32>().map_err(|e| format!("`{key}` = `{v}`: {e}"))
    }

    /// Keys under `section.`, with the prefix removed.
    pub fn section(&self, section: &str) -> impl Iterator<Item = (&str, &str)> {
        let prefix = format!("{section}.");
        self.map.iter().filter_map(move |(k, v)| k.strip_prefix(&prefix).map(|k| (k, v.as_str())))
    }
}
