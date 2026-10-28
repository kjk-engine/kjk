//! Link overrides: our own data, one text file per module
//! (`data/links/<game>/<module>.links`, built in), that give a door or
//! trigger a destination the game does not, or another one.
//!
//! One entry per line, `#` to the end of a line is a comment:
//!
//! ```text
//! <tag> <module> [<arrival tag>]
//! ```
//!
//! Every door and trigger with that tag leads to `module`, arriving at the
//! door or waypoint tagged `<arrival tag>` (the module's entry without
//! one). Tags and modules are matched without case.

include!(concat!(env!("OUT_DIR"), "/links.rs"));

/// One override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkOverride {
    pub line: usize,
    pub tag: String,
    pub module: String,
    pub arrival: String,
}

/// The overrides built in for `module` of `game` (`k1`, `tsl`); none when
/// it has no file.
pub fn built_in(game: &str, module: &str) -> Result<Vec<LinkOverride>, String> {
    let Some((_, _, text)) = FILES
        .iter()
        .find(|(g, m, _)| g.eq_ignore_ascii_case(game) && m.eq_ignore_ascii_case(module))
    else {
        return Ok(Vec::new());
    };
    parse(text).map_err(|error| format!("data/links/{game}/{module}.links: {error}"))
}

/// Every built-in file: (game, module).
pub fn all() -> impl Iterator<Item = (&'static str, &'static str)> {
    FILES.iter().map(|&(game, module, _)| (game, module))
}

pub fn parse(text: &str) -> Result<Vec<LinkOverride>, String> {
    let mut links = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let content = line.split('#').next().unwrap_or("");
        let words: Vec<&str> = content.split_whitespace().collect();
        let (tag, module, arrival) = match words.as_slice() {
            [] => continue,
            [tag, module] => (tag, module, ""),
            [tag, module, arrival] => (tag, module, *arrival),
            _ => {
                return Err(format!(
                    "line {line_number}: use <tag> <module> [<arrival tag>]"
                ));
            }
        };
        links.push(LinkOverride {
            line: line_number,
            tag: tag.to_ascii_lowercase(),
            module: module.to_ascii_lowercase(),
            arrival: arrival.to_ascii_lowercase(),
        });
    }
    Ok(links)
}
