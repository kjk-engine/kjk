//! What a script does, for the script viewer: its source where the install
//! has it (`.nss`), and from its compiled form (`.ncs`) the engine routines
//! it calls, the strings it uses and its listing (`script_ncs::disasm`).
//!
//! The games ship their scripts compiled. KOTOR 1's install also holds
//! the source of most of them, but a module's own scripts are often
//! compiled only. The summary of calls and strings tells what such a script
//! does at a glance; the listing tells it all.

use std::collections::BTreeMap;
use std::path::Path;

use bevy_egui::egui;
use fmt_odyssey::ResType;
use script_ncs::Catalog;
use vfs::resolver::Resolver;

/// One script, read for the viewer.
pub struct ScriptView {
    /// The source and the layer it came from.
    pub source: Option<(String, String)>,
    /// The layer the compiled script came from; `None` when there is none.
    pub compiled_from: Option<String>,
    /// The engine routines it calls, each with how often, in the order of
    /// their first call: the routine's prototype from `nwscript.nss`.
    pub calls: Vec<(String, usize)>,
    /// The strings it holds (tags, templates, globals' names), each once.
    pub strings: Vec<String>,
    pub listing: Option<String>,
    /// Why the compiled script does not read.
    pub problem: Option<String>,
}

/// A file's text in the games' code page: every byte one character.
fn text(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}

/// The game's routines and constants, from its `nwscript.nss`.
pub fn catalog(resolver: &Resolver) -> Option<Catalog> {
    let bytes = resolver.read("nwscript", ResType::NSS)?;
    Catalog::parse(&text(&bytes)).ok()
}

/// The layer name the viewer gives the project's folder.
const PROJECT: &str = "the project";

/// Reads `name`, the project's folder `project` first (read now, so a
/// script written or compiled since the editor opened shows), then
/// through `resolver`, its routines named from `catalog`.
pub fn read(
    resolver: &Resolver,
    catalog: Option<&Catalog>,
    project: &Path,
    name: &str,
) -> ScriptView {
    let own = |extension: &str| std::fs::read(project.join(format!("{name}.{extension}"))).ok();
    let layer_of = |restype| {
        resolver
            .find(name, restype)
            .map(|found| found.layer.name.clone())
    };
    let source = match own("nss") {
        Some(bytes) => Some((text(&bytes), PROJECT.to_string())),
        None => resolver.read(name, ResType::NSS).map(|bytes| {
            (
                text(&bytes),
                layer_of(ResType::NSS).unwrap_or_else(String::new),
            )
        }),
    };
    let compiled = own("ncs");
    let compiled_from = match compiled {
        Some(_) => Some(PROJECT.to_string()),
        None => layer_of(ResType::NCS),
    };
    let mut view = ScriptView {
        source,
        compiled_from,
        calls: Vec::new(),
        strings: Vec::new(),
        listing: None,
        problem: None,
    };
    let Some(bytes) = compiled.or_else(|| resolver.read(name, ResType::NCS)) else {
        return view;
    };
    let ncs = match fmt_odyssey::ncs::Ncs::parse(&bytes) {
        Ok(ncs) => ncs,
        Err(error) => {
            view.problem = Some(error.to_string());
            return view;
        }
    };
    let mut order: Vec<u16> = Vec::new();
    let mut counts: BTreeMap<u16, usize> = BTreeMap::new();
    for instruction in &ncs.code {
        match &instruction.op {
            fmt_odyssey::ncs::Op::Action { routine, .. } => {
                if !counts.contains_key(routine) {
                    order.push(*routine);
                }
                *counts.entry(*routine).or_insert(0) += 1;
            }
            fmt_odyssey::ncs::Op::Const(fmt_odyssey::ncs::Const::String(bytes)) => {
                let string = text(bytes);
                if !string.is_empty() && !view.strings.contains(&string) {
                    view.strings.push(string);
                }
            }
            _ => {}
        }
    }
    view.calls = order
        .into_iter()
        .map(|routine| {
            let named = catalog
                .and_then(|catalog| catalog.routine_by_id(routine))
                .map_or_else(|| format!("routine {routine}"), |found| found.to_string());
            (named, counts[&routine])
        })
        .collect();
    view.listing = Some(script_ncs::disasm::listing(&ncs, catalog));
    view
}

/// NWScript's words, coloured as keywords.
const KEYWORDS: [&str; 26] = [
    "void", "int", "float", "string", "object", "effect", "event", "location", "talent", "vector",
    "action", "struct", "const", "if", "else", "while", "for", "do", "switch", "case", "default",
    "break", "continue", "return", "#include", "#define",
];

/// `source` laid out in a fixed-width font, its comments, strings,
/// keywords and numbers coloured.
pub fn highlighted(source: &str, dark: bool) -> egui::text::LayoutJob {
    let font = egui::FontId::monospace(12.0);
    let colour = |kind: Token| match (kind, dark) {
        (Token::Comment, _) => egui::Color32::from_rgb(110, 160, 100),
        (Token::String, _) => egui::Color32::from_rgb(214, 157, 120),
        (Token::Keyword, true) => egui::Color32::from_rgb(110, 160, 230),
        (Token::Keyword, false) => egui::Color32::from_rgb(30, 80, 170),
        (Token::Number, _) => egui::Color32::from_rgb(180, 200, 140),
        (Token::Plain, true) => egui::Color32::from_gray(220),
        (Token::Plain, false) => egui::Color32::from_gray(30),
    };
    let mut job = egui::text::LayoutJob::default();
    for (token, text) in tokens(source) {
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: colour(token),
                ..Default::default()
            },
        );
    }
    job
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    Comment,
    String,
    Keyword,
    Number,
    Plain,
}

/// The plain text of `source` from `from` to `to`, if any.
fn plain_run<'s>(out: &mut Vec<(Token, &'s str)>, source: &'s str, from: usize, to: usize) {
    if from < to {
        out.push((Token::Plain, &source[from..to]));
    }
}

/// `source` cut into tokens for colouring: comments (`//`, `/* */`),
/// strings, words and the rest, each piece of the text once, in order.
fn tokens(source: &str) -> Vec<(Token, &str)> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    let mut plain = 0;
    while at < bytes.len() {
        let rest = &source[at..];
        let (token, len) = if rest.starts_with("//") {
            (Token::Comment, rest.find('\n').unwrap_or(rest.len()))
        } else if rest.starts_with("/*") {
            (
                Token::Comment,
                rest.find("*/").map_or(rest.len(), |end| end + 2),
            )
        } else if bytes[at] == b'"' {
            let mut end = 1;
            while end < rest.len() {
                match rest.as_bytes()[end] {
                    b'\\' => end += 2,
                    b'"' => {
                        end += 1;
                        break;
                    }
                    b'\n' => break,
                    _ => end += 1,
                }
            }
            (Token::String, end.min(rest.len()))
        } else if bytes[at].is_ascii_alphabetic() || bytes[at] == b'_' || bytes[at] == b'#' {
            let end = rest
                .char_indices()
                .skip(1)
                .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
                .map_or(rest.len(), |(end, _)| end);
            let word = &rest[..end];
            if KEYWORDS.contains(&word) {
                (Token::Keyword, end)
            } else {
                at += end;
                continue;
            }
        } else if bytes[at].is_ascii_digit() {
            let end = rest
                .char_indices()
                .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '.'))
                .map_or(rest.len(), |(end, _)| end);
            (Token::Number, end)
        } else {
            at += rest.chars().next().map_or(1, char::len_utf8);
            continue;
        };
        plain_run(&mut out, source, plain, at);
        out.push((token, &source[at..at + len]));
        at += len;
        plain = at;
    }
    plain_run(&mut out, source, plain, source.len());
    out
}
