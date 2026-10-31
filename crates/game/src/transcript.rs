//! A conversation's lines as text: what `kjk dialogue run` writes to its
//! transcript and a headless play prints as it goes (`sim_host`).

use std::collections::BTreeMap;

use fmt_odyssey::dlg::Node;
use script_ncs::vm::ObjectId;
use sim::script::Library;
use sim::script::dialogue::{Line, Said};

/// A strref's text (`Installer::text`).
pub type Talk<'a> = &'a dyn Fn(u32) -> Option<String>;

/// A node's text: its own string, else its strref's.
pub fn text(talk: Talk, node: &Node) -> String {
    match node.text.strings.first() {
        Some((_, own)) if !own.is_empty() => own.iter().map(|&b| char::from(b)).collect(),
        _ if node.text.strref == u32::MAX => String::new(),
        _ => talk(node.text.strref).unwrap_or_else(String::new),
    }
}

/// The transcript's lines for one report line, text resolved and
/// `<CUSTOMn>` filled from `tokens`, the values the report's lines before
/// it set (`Line::Token`, which updates them and writes nothing), and the
/// player's own tokens from `player` (`<FirstName>`: the first seat's
/// player, the party's own character, whoever it drives).
pub fn transcript(
    talk: Talk,
    library: &Library,
    names: &BTreeMap<u32, String>,
    (tokens, player): (&mut BTreeMap<i32, String>, Option<&sim::persona::Persona>),
    line: &Line,
) -> Vec<String> {
    if let Line::Token { number, value, .. } = line {
        tokens.insert(*number, value.clone());
        return Vec::new();
    }
    let tokens = &*tokens;
    let talk = |strref| talk(strref).map(|text| filled(&text, tokens, player));
    let who = |id: ObjectId| {
        names
            .get(&id.0)
            .cloned()
            .unwrap_or_else(|| format!("#{}", id.0))
    };
    let node = |dialog: &str, entry: bool, index: u32| {
        library
            .dialogue(dialog)
            .and_then(|dlg| {
                let list = if entry { &dlg.entries } else { &dlg.replies };
                list.get(index as usize)
            })
            .map_or_else(String::new, |node| {
                filled(&text(&talk, node), tokens, player)
            })
    };
    let at = |ms: u64| format!("{:>8.2}s", ms as f64 / 1000.0);
    match line {
        Line::Entry {
            ms,
            dialog,
            entry,
            speaker,
            voice,
            ..
        } => vec![format!(
            "{} {}: {:?}  ({dialog} E{entry}{})",
            at(*ms),
            who(*speaker),
            node(dialog, true, *entry),
            voice
                .as_ref()
                .map_or_else(String::new, |voice| format!(", voice {voice}"))
        )],
        Line::Offered {
            dialog, replies, ..
        } => replies
            .iter()
            .enumerate()
            .map(|(n, reply)| {
                format!(
                    "{:>9}   {}. {:?}  (R{reply})",
                    "",
                    n + 1,
                    node(dialog, false, *reply)
                )
            })
            .collect(),
        Line::Picked {
            ms, dialog, reply, ..
        } => vec![format!(
            "{}   > {:?}  (R{reply})",
            at(*ms),
            node(dialog, false, *reply)
        )],
        Line::Marked {
            ms,
            dialog,
            seat,
            reply,
            ..
        } => vec![format!(
            "{}   seat {seat} marks {:?}  (R{reply})",
            at(*ms),
            node(dialog, false, *reply)
        )],
        Line::Bark {
            ms, who: by, text, ..
        } => {
            let said = match text {
                Said::Entry { dialog, entry } => node(dialog, true, *entry),
                Said::StrRef(strref) => talk(*strref).unwrap_or_else(String::new),
            };
            vec![format!("{} {} (bark): {said:?}", at(*ms), who(*by))]
        }
        Line::Ended {
            ms,
            dialog,
            aborted,
            ..
        } => vec![format!(
            "{} -- {dialog} ends{}",
            at(*ms),
            if *aborted { ", cut short" } else { "" }
        )],
        Line::Refused { ms, dialog, why } => {
            vec![format!("{} -- {dialog} not started: {why}", at(*ms))]
        }
        Line::CutShort { ms, id, why } => {
            vec![format!("{} -- conversation {id} cut short: {why}", at(*ms))]
        }
        Line::Asked { ms, id, by } => {
            vec![format!(
                "{} -- {} asks the host into conversation {id}",
                at(*ms),
                who(*by)
            )]
        }
        Line::Journal { ms, quest, entry } => {
            let entry = library
                .quests()
                .get(quest)
                .and_then(|of| of.entry(*entry as u32))
                .and_then(|entry| talk(entry.text))
                .map_or_else(|| entry.to_string(), |text| format!("{entry} {text:?}"));
            vec![format!("{} -- journal: {quest} at {entry}", at(*ms))]
        }
        Line::Token { .. } | Line::Cued { .. } => Vec::new(),
    }
}

/// The `<CUSTOMn>` values `lines` set, the last of each.
pub fn tokens(lines: &[Line]) -> BTreeMap<i32, String> {
    let mut tokens = BTreeMap::new();
    for line in lines {
        if let Line::Token { number, value, .. } = line {
            tokens.insert(*number, value.clone());
        }
    }
    tokens
}

/// `text` with each `<CUSTOMn>` that `tokens` holds filled in, and the
/// player's tokens `player` answers (`dialogue_view::player_token`).
fn filled(
    text: &str,
    tokens: &BTreeMap<i32, String>,
    player: Option<&sim::persona::Persona>,
) -> String {
    let text = tokens
        .iter()
        .fold(text.to_string(), |text, (number, value)| {
            text.replace(&format!("<CUSTOM{number}>"), value)
        });
    let Some(pc) = player else {
        return text;
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let token = &rest[open..];
        match token.find('>') {
            Some(close) => {
                match crate::dialogue_view::player_token(&token[1..close], pc) {
                    Some(value) => out.push_str(&value),
                    None => out.push_str(&token[..=close]),
                }
                rest = &token[close + 1..];
            }
            None => {
                out.push_str(token);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Each object's name for the transcript: its tag, the player "player".
pub fn speakers(snapshot: &sim::Snapshot, level: &sim::Level) -> BTreeMap<u32, String> {
    let objects = sim::Instances::in_snapshot(level, snapshot);
    snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let name = match entity.kind {
                sim::Kind::Avatar(_) => "player".to_string(),
                _ => objects.of_entity(entity)?.tag.clone(),
            };
            Some((entity.id.0, name))
        })
        .collect()
}

/// The player's tokens filled in a transcript's words (`filled`): the
/// first name and a gender's pair from the first seat's player, after the
/// scripts' `<CUSTOMn>`; without a player they stand as they are.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    let exile = sim::persona::Persona {
        seat: sim::Seat(0),
        appearance: 0,
        gender: 0,
        portrait: 0,
        name: "Exile".into(),
    };
    let tokens = BTreeMap::from([(5, "Telos".to_string())]);
    [
        (
            "transcript_first_name",
            "I'm ready to go. What happened with <FirstName>?",
            Some(&exile),
            "I'm ready to go. What happened with Exile?",
        ),
        (
            "transcript_gender_pair",
            "Yes, <sir/madam>. To <CUSTOM5>, <FullName>.",
            Some(&exile),
            "Yes, sir. To Telos, Exile.",
        ),
        (
            "transcript_no_player",
            "What happened with <FirstName>?",
            None,
            "What happened with <FirstName>?",
        ),
    ]
    .into_iter()
    .map(|(name, text, player, wanted)| {
        let got = filled(text, &tokens, player);
        (name, sim::probe::check(got == wanted, format!("{got:?}")))
    })
    .collect()
}
