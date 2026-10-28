use std::collections::HashMap;
use std::path::Path;

/// `KEY=VALUE` lines from a `.env` file; `#` comments and blank lines are
/// skipped, and one pair of surrounding quotes is removed. A missing file is
/// an empty map.
pub fn load(path: &Path) -> HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|inner| inner.strip_suffix('"'))
                .unwrap_or(value);
            (key.trim().to_string(), value.to_string())
        })
        .collect()
}
