//! The first place two traces differ: read side by side, a line at a
//! time, the expected trace (`a`, the oracle's) and the actual (`b`).

use std::collections::VecDeque;
use std::fmt;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Records shown before a divergence.
fn context() -> usize {
    10
}

/// Where two traces part.
#[derive(Debug, Clone, PartialEq)]
pub struct Divergence {
    /// The scenario, or `-` before the first.
    pub scenario: String,
    /// The step's number in its scenario (its record's first word), or
    /// the line's when it has none.
    pub step: String,
    /// The first field that differs (`line` when the lines do not even
    /// share a shape: a header, a scenario start, one trace ending first).
    pub field: String,
    pub expected: String,
    pub actual: String,
    /// The line in each file (1-based).
    pub line: usize,
    /// Up to ten records before it, from the expected trace.
    pub before: Vec<String>,
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(
            f,
            "first divergence: scenario {} step {} field {} (line {})",
            self.scenario, self.step, self.field, self.line
        )?;
        writeln!(
            f,
            "  expected: {}{}",
            self.expected,
            decoded(&self.expected)
        )?;
        writeln!(f, "  actual:   {}{}", self.actual, decoded(&self.actual))?;
        if !self.before.is_empty() {
            writeln!(f, "the {} records before it (expected):", self.before.len())?;
            for line in &self.before {
                writeln!(f, "  {line}")?;
            }
        }
        Ok(())
    }
}

/// A value of float bits (eight hex digits, commas between) as the floats
/// they are, for reading: ` (= 1.5, -2)`; nothing for any other value.
fn decoded(value: &str) -> String {
    let floats: Option<Vec<String>> = value
        .split(',')
        .map(|word| {
            (word.len() == 8)
                .then(|| u32::from_str_radix(word, 16).ok())
                .flatten()
                .map(|bits| format!("{:?}", f32::from_bits(bits)))
        })
        .collect();
    match floats {
        Some(floats) if !floats.is_empty() => format!("  (= {})", floats.join(", ")),
        _ => String::new(),
    }
}

/// The first divergence between the traces at `a` and `b`, with how many
/// lines matched; `None` when they are the same.
pub fn diff(a: &Path, b: &Path) -> Result<(Option<Divergence>, usize), String> {
    let open = |path: &Path| {
        std::fs::File::open(path)
            .map(|file| BufReader::with_capacity(1 << 20, file).lines())
            .map_err(|e| format!("{}: {e}", path.display()))
    };
    let (mut left, mut right) = (open(a)?, open(b)?);
    let mut scenario = "-".to_string();
    let mut before: VecDeque<String> = VecDeque::with_capacity(context() + 1);
    let mut line = 0;
    loop {
        line += 1;
        let next = |lines: &mut std::io::Lines<BufReader<std::fs::File>>, path: &Path| {
            lines
                .next()
                .transpose()
                .map_err(|e| format!("{}: {e}", path.display()))
        };
        let (expected, actual) = (next(&mut left, a)?, next(&mut right, b)?);
        let (expected, actual) = match (expected, actual) {
            (None, None) => return Ok((None, line - 1)),
            (expected, actual) if expected == actual => {
                let text = expected.unwrap_or_else(String::new);
                if let Some(name) = text.strip_prefix("== ") {
                    scenario = name.to_string();
                    before.clear();
                } else if !text.starts_with('#') {
                    if before.len() == context() {
                        before.pop_front();
                    }
                    before.push_back(text);
                }
                continue;
            }
            (expected, actual) => (
                expected.unwrap_or_else(|| "(end of trace)".to_string()),
                actual.unwrap_or_else(|| "(end of trace)".to_string()),
            ),
        };
        return Ok((
            Some(divergence(
                scenario,
                &expected,
                &actual,
                line,
                before.into(),
            )),
            line - 1,
        ));
    }
}

/// How the lines `expected` and `actual` differ.
fn divergence(
    scenario: String,
    expected: &str,
    actual: &str,
    line: usize,
    before: Vec<String>,
) -> Divergence {
    let whole = |field: &str, step: &str| Divergence {
        scenario: scenario.clone(),
        step: step.to_string(),
        field: field.to_string(),
        expected: expected.to_string(),
        actual: actual.to_string(),
        line,
        before: before.clone(),
    };
    let record = |text: &str| -> Option<(String, Vec<(String, String)>)> {
        if text.starts_with('#') || text.starts_with("==") || text.starts_with('(') {
            return None;
        }
        let mut words = text.split(' ');
        let step = words.next()?.to_string();
        let fields = words
            .map(|word| {
                let (key, value) = word.split_once('=').unwrap_or((word, ""));
                (key.to_string(), value.to_string())
            })
            .collect();
        Some((step, fields))
    };
    let (Some((step_a, fields_a)), Some((step_b, fields_b))) = (record(expected), record(actual))
    else {
        return whole("line", &line.to_string());
    };
    if step_a != step_b {
        return whole("step", &step_a);
    }
    let missing = "(missing)".to_string();
    let longest = fields_a.len().max(fields_b.len());
    for at in 0..longest {
        let (key_a, value_a) = fields_a
            .get(at)
            .map_or((&missing, &missing), |(k, v)| (k, v));
        let (key_b, value_b) = fields_b
            .get(at)
            .map_or((&missing, &missing), |(k, v)| (k, v));
        if key_a != key_b {
            return Divergence {
                field: format!("{key_a} (actual has {key_b})"),
                ..whole("", &step_a)
            };
        }
        if value_a != value_b {
            return Divergence {
                field: key_a.clone(),
                expected: value_a.clone(),
                actual: value_b.clone(),
                ..whole("", &step_a)
            };
        }
    }
    whole("line", &step_a)
}
