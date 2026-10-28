//! Scenario files: plain text, one directive a line, words split by
//! spaces, `#` to the end of a line a comment. `scenario NAME` starts a
//! scenario; every line up to the next belongs to it, and the package's
//! driver reads them in order (`docs/EQUIVALENCE.md` lists each package's
//! directives). Numbers are decimal; a float may also be written as its
//! bits, `0x` and eight hex digits, so a generated scenario replays
//! exactly.

/// One named scenario: its directives in order, each a line's words.
#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    pub name: String,
    pub lines: Vec<Line>,
}

/// A directive: its words, and the line it came from (0 when generated).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub words: Vec<String>,
    pub at: usize,
}

impl Line {
    pub fn new(words: Vec<String>) -> Line {
        Line { words, at: 0 }
    }

    /// The directive's name.
    pub fn verb(&self) -> &str {
        self.words.first().map_or("", String::as_str)
    }

    /// The words after the directive's name.
    pub fn args(&self) -> &[String] {
        self.words.get(1..).unwrap_or(&[])
    }

    /// Where it came from, for errors.
    pub fn place(&self) -> String {
        if self.at == 0 {
            format!("`{}`", self.words.join(" "))
        } else {
            format!("line {} `{}`", self.at, self.words.join(" "))
        }
    }
}

impl Scenario {
    pub fn new(name: impl Into<String>) -> Scenario {
        Scenario {
            name: name.into(),
            lines: Vec::new(),
        }
    }

    /// Adds a directive from its words.
    pub fn push(&mut self, words: &[&str]) {
        self.lines
            .push(Line::new(words.iter().map(|w| w.to_string()).collect()));
    }

    /// Adds a directive already split.
    pub fn push_owned(&mut self, words: Vec<String>) {
        self.lines.push(Line::new(words));
    }

    /// The scenario as a scenario file's text.
    pub fn to_text(&self) -> String {
        let mut out = format!("scenario {}\n", self.name);
        for line in &self.lines {
            out.push_str(&line.words.join(" "));
            out.push('\n');
        }
        out
    }
}

/// The scenarios in a file's text.
pub fn parse(text: &str) -> Result<Vec<Scenario>, String> {
    let mut out: Vec<Scenario> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("");
        let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        if words.is_empty() {
            continue;
        }
        if words[0] == "scenario" {
            let Some(name) = words.get(1) else {
                return Err(format!("line {}: a scenario needs a name", index + 1));
            };
            if out.iter().any(|s| &s.name == name) {
                return Err(format!("line {}: scenario {name} twice", index + 1));
            }
            out.push(Scenario::new(name.clone()));
            continue;
        }
        let Some(scenario) = out.last_mut() else {
            return Err(format!(
                "line {}: `{}` before any `scenario`",
                index + 1,
                words.join(" ")
            ));
        };
        scenario.lines.push(Line {
            words,
            at: index + 1,
        });
    }
    Ok(out)
}

/// A float word: decimal, or `0x` and its bits.
pub fn float(word: &str) -> Result<f32, String> {
    if let Some(hex) = word.strip_prefix("0x") {
        u32::from_str_radix(hex, 16)
            .map(f32::from_bits)
            .map_err(|_| format!("bad float bits {word}"))
    } else {
        word.parse::<f32>()
            .map_err(|_| format!("bad number {word}"))
    }
}

/// An integer word of any type.
pub fn int<T: std::str::FromStr>(word: &str) -> Result<T, String> {
    word.parse::<T>().map_err(|_| format!("bad integer {word}"))
}

/// Three float words.
pub fn vec3(words: &[String]) -> Result<[f32; 3], String> {
    if words.len() < 3 {
        return Err("needs three numbers".to_string());
    }
    Ok([float(&words[0])?, float(&words[1])?, float(&words[2])?])
}

/// `key=value` words as pairs (a word without `=` is a key with an empty
/// value).
pub fn pairs(words: &[String]) -> Vec<(&str, &str)> {
    words
        .iter()
        .map(|word| word.split_once('=').unwrap_or((word.as_str(), "")))
        .collect()
}
