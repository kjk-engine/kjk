//! `kjk rules <game>`: the wiki for playtesters. The rulebook (every rule
//! the engine plays by, `data/rules/`, the `rules` crate) and, beside it,
//! the game's own things as the engine reads them (`session::codex`: every
//! weapon, item, Force power, grenade and consumable, feat, skill, class
//! and the experience table, the rulebook's table changes in), each with
//! the rules that shape it (`rules::applying` its tags). One page,
//! `index.html`, searchable, and `codex.json` with the same facts, in
//! `kjk-artifacts/rules/<game>/` (or `--out`). Nothing in it is written by
//! hand: run it again after any change and it says what the game plays.
//!
//! Every rule shows its path (`combat.damage.pace`): a playtester asks for
//! "combat.damage.pace to 3" and that one line of `data/rules/` changes.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use fmt_odyssey::ResType;
use kjk_game::modules::ResolverSource;
use rules::{Entry, Kind};
use serde_json::{Value, json};
use session::codex::{self, Codex, Row};
use sim::combat::gear::Roll;
use sim::combat::sheet::Weapon;
use vfs::resolver::{RULEBOOK, Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

/// The damage type bits of `baseitems.2da` `damageflags`, by bit.
const DAMAGE_FLAGS: [&str; 13] = [
    "bludgeoning",
    "piercing",
    "slashing",
    "universal",
    "acid",
    "cold",
    "light side",
    "electrical",
    "fire",
    "dark side",
    "sonic",
    "ion",
    "energy",
];

pub fn run(discovery: &Discovery, game: &str, out: Option<PathBuf>) -> ExitCode {
    let Some(game) = Game::from_key(game).filter(|game| *game != Game::JediAcademy) else {
        eprintln!("rules: use k1 or tsl");
        return ExitCode::from(2);
    };
    let Some(install) = discovery.get(game) else {
        eprintln!("{}: not found (set {})", game.key(), game.env_var());
        return ExitCode::FAILURE;
    };
    let opened = DirIndex::build(&install.data_root)
        .map_err(|error| error.to_string())
        .and_then(|index| {
            Resolver::base(game, &index, TextureQuality::High)
                .map(|resolver| (index, resolver))
                .map_err(|error| error.to_string())
        });
    let (index, resolver) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let codex = codex::codex(&ResolverSource(&resolver));
    let bestiary = bestiary(&index, &resolver, game.key());
    let changes = table_changes(&resolver, game.key());
    let dir = out.unwrap_or_else(|| PathBuf::from("kjk-artifacts/rules").join(game.key()));
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("{}: {error}", dir.display());
        return ExitCode::FAILURE;
    }
    let page = page(game, &codex, &changes, &bestiary);
    let facts = facts(game, &codex, &changes, &bestiary);
    let written = std::fs::write(dir.join("index.html"), page).and_then(|()| {
        std::fs::write(
            dir.join("codex.json"),
            serde_json::to_string_pretty(&facts).unwrap_or_else(|_| String::new()),
        )
    });
    if let Err(error) = written {
        eprintln!("{}: {error}", dir.display());
        return ExitCode::FAILURE;
    }
    for unknown in unknown_keys() {
        eprintln!("rulebook: {unknown} names no control");
    }
    for problem in resolver.problems.iter().filter(|p| p.source == RULEBOOK) {
        eprintln!("rulebook: {}", problem.problem);
    }
    outln!(
        "rules {}: {} rules, {} weapons, {} items, {} spells, {} feats, {} table changes → {}",
        game.key(),
        rules::BOOK.iter().filter(|entry| entry.is_rule()).count(),
        codex.weapons.len(),
        codex.items.len(),
        codex.spells.len(),
        codex.feats.len(),
        changes.len(),
        dir.join("index.html").display()
    );
    ExitCode::SUCCESS
}

/// A module and the creatures it places.
type Module = (String, Vec<codex::Placed>);

/// Every module's creatures (`codex::placed`), the templates read once.
fn bestiary(index: &DirIndex, base: &Resolver, game: &str) -> Vec<Module> {
    let templates = codex::templates(&ResolverSource(base));
    let mut out = Vec::new();
    for module in Resolver::modules(index) {
        let Ok(resolver) = base.with_module(index, &module) else {
            continue;
        };
        match codex::placed(&ResolverSource(&resolver), game, &module, &templates) {
            Ok(placed) if !placed.is_empty() => out.push((module, placed)),
            Ok(_) => {}
            Err(error) => eprintln!("rules: {error}"),
        }
    }
    out
}

fn creatures_html(codex: &Codex, bestiary: &[Module]) -> String {
    let class_name = |row: u8| {
        codex
            .classes
            .iter()
            .find(|class| class.row.row == u16::from(row))
            .map_or_else(|| format!("class {row}"), |class| class.row.name.clone())
    };
    let mut out = String::new();
    for (module, placed) in bestiary {
        let hostiles = placed.iter().filter(|creature| creature.hostile).count();
        let module = escape(module);
        let _ = write!(
            out,
            "<details class=\"module\" data-search=\"{}\"><summary><b>{module}</b> <span class=\"dim\">{} creatures, {hostiles} hostile</span></summary><table class=\"grid\"><tr><th>Name</th><th>Tag</th><th>Level</th><th>HP</th><th>HP ×4 players</th><th>FP</th><th>Defence</th><th>To hit</th><th>STR DEX CON INT WIS CHA</th><th>Weapons</th><th>Side</th></tr>",
            escape(
                &placed
                    .iter()
                    .map(|c| format!("{} {} {}", c.name, c.tag, c.template))
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase()
            ),
            placed.len()
        );
        for creature in placed {
            let sheet = &creature.sheet;
            let classes: Vec<String> = creature
                .classes
                .iter()
                .map(|(row, levels)| format!("{} {levels}", class_name(*row)))
                .collect();
            let to_hit = sheet.weapons[0].map_or_else(
                || sim::combat::damage::attack_bonus(sheet, &Weapon::UNARMED),
                |weapon| sim::combat::damage::attack_bonus(sheet, &weapon),
            );
            let mut weapons: Vec<String> = sheet
                .weapons
                .iter()
                .flatten()
                .map(|weapon| format!("{} {}", delivery_name(weapon.delivery), damage(weapon)))
                .collect();
            if weapons.is_empty() {
                weapons.push("unarmed".to_string());
            }
            let mut side = if creature.hostile { "hostile" } else { "" }.to_string();
            for (flag, word) in [
                (sim::combat::sheet::flags::PLOT, "plot"),
                (sim::combat::sheet::flags::MIN_ONE_HP, "never below 1 HP"),
            ] {
                if sheet.flags & flag != 0 {
                    side = [side, word.to_string()]
                        .iter()
                        .filter(|s| !s.is_empty())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                }
            }
            let _ = write!(
                out,
                "<tr><td>{}</td><td><code>{}</code></td><td>{} <span class=\"dim\">{}</span></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:+}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(if creature.name.is_empty() {
                    &creature.template
                } else {
                    &creature.name
                }),
                escape(&creature.tag),
                sheet.level,
                escape(&classes.join(", ")),
                creature
                    .hp_one
                    .map_or_else(|| "with 4 only".to_string(), |hp| hp.to_string()),
                match creature.hp_four {
                    None => "removed".to_string(),
                    Some(hp) if Some(hp) == creature.hp_one => String::new(),
                    Some(hp) => hp.to_string(),
                },
                sheet.force_points(),
                sheet.defence(),
                to_hit,
                sheet.abilities.map(|score| score.to_string()).join(" "),
                escape(&weapons.join(" / ")),
                side,
            );
        }
        out.push_str("</table></details>");
    }
    out
}

/// How a weapon fights, in a word or two.
fn delivery_name(delivery: sim::combat::sheet::Delivery) -> String {
    use sim::combat::sheet::{Delivery, SaberShape};
    match delivery {
        Delivery::Unarmed => "unarmed".to_string(),
        Delivery::Baton => "baton".to_string(),
        Delivery::Melee => "melee".to_string(),
        Delivery::Saber(SaberShape::Single) => "lightsaber".to_string(),
        Delivery::Saber(SaberShape::Staff) => "double lightsaber".to_string(),
        Delivery::Saber(SaberShape::Short) => "short lightsaber".to_string(),
        Delivery::Blaster(blaster) => format!("{blaster:?}").to_lowercase(),
    }
}

/// A table change with the install's own cell beside it.
struct Changed {
    change: &'static rules::TableChange,
    was: String,
}

/// The rulebook's changes to `game`'s tables, each with the cell the
/// install has (the table as the layers under the rulebook's resolve it).
fn table_changes(resolver: &Resolver, game: &str) -> Vec<Changed> {
    rules::TABLE_CHANGES
        .iter()
        .filter(|change| change.game == game)
        .map(|change| {
            let was = resolver
                .find(change.table, ResType::TWO_DA)
                .and_then(|found| {
                    let below = match found.layer.name.as_str() {
                        RULEBOOK => found.shadowed.first()?.1,
                        _ => found.location,
                    };
                    fmt_odyssey::twoda::TwoDa::parse(&below.read().ok()?).ok()
                })
                .and_then(|table| {
                    let row = table
                        .find("label", change.row)
                        .or_else(|| change.row.parse().ok())?;
                    table.text(row, change.column)
                })
                .unwrap_or_else(|| "****".to_string());
            Changed { change, was }
        })
        .collect()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The game's controls by label (`kjk_game::key_bindings`), for the book's
/// `[Action]` names.
fn keys() -> &'static [(String, String)] {
    static KEYS: std::sync::LazyLock<Vec<(String, String)>> =
        std::sync::LazyLock::new(kjk_game::key_bindings);
    &KEYS
}

/// `html` with each `[Action]` the controls know as its key, outside its
/// `<code>` spans.
fn keyed(html: &str) -> String {
    let mut out = String::new();
    for (at, piece) in html.split("<code>").enumerate() {
        let (code, text) = match piece.split_once("</code>") {
            Some((code, text)) if at > 0 => (Some(code), text),
            _ => (None, piece),
        };
        if at > 0 {
            out.push_str("<code>");
        }
        if let Some(code) = code {
            out.push_str(code);
            out.push_str("</code>");
        }
        out.push_str(&keyed_text(text));
    }
    out
}

/// `keyed` for text with no `<code>` span.
fn keyed_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        let named = tail.find(']').and_then(|close| {
            let label = tail[1..close]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let (_, key) = keys().iter().find(|(known, _)| *known == label)?;
            Some((close, label, escape(key)))
        });
        match named {
            Some((close, label, key)) => {
                let _ = write!(out, "<kbd title=\"{label}\">{label} · {key}</kbd>");
                rest = &tail[close + 1..];
            }
            None => {
                out.push('[');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The book's `[Action]` names no control has.
fn unknown_keys() -> Vec<String> {
    let mut out = Vec::new();
    for entry in rules::BOOK {
        let text = format!("{} {}", entry.summary, entry.detail);
        let mut rest = text.as_str();
        while let Some(open) = rest.find('[') {
            let tail = &rest[open + 1..];
            let Some(close) = tail.find(']') else { break };
            let label = tail[..close]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let worded =
                label.chars().next().is_some_and(char::is_uppercase) && !label.contains('`');
            if worded && !keys().iter().any(|(known, _)| *known == label) {
                out.push(format!("{}: [{label}]", entry.path));
            }
            rest = &tail[close..];
        }
    }
    out
}

/// The book's prose as HTML: escaped, `code` spans kept, paragraphs by
/// blank lines.
fn prose(text: &str) -> String {
    let mut out = String::new();
    for paragraph in text.split("\n\n").filter(|p| !p.trim().is_empty()) {
        let mut html = String::new();
        for (at, piece) in escape(paragraph).split('`').enumerate() {
            if at % 2 == 1 {
                let _ = write!(html, "<code>{piece}</code>");
            } else {
                html.push_str(piece);
            }
        }
        let _ = write!(
            out,
            "<p>{}</p>",
            html.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }
    keyed(&out)
}

fn anchor(path: &str) -> String {
    format!("r-{}", rules::page::anchor(path))
}

fn roll(roll: Roll) -> String {
    let mut out = String::new();
    if roll.dice > 0 {
        let _ = write!(out, "{}d{}", roll.dice, roll.die);
    }
    if roll.bonus != 0 {
        let _ = write!(out, "{:+}", roll.bonus);
    }
    out
}

fn types(bits: u16) -> String {
    let named: Vec<&str> = DAMAGE_FLAGS
        .iter()
        .enumerate()
        .filter(|(bit, _)| bits >> bit & 1 == 1)
        .map(|(_, name)| *name)
        .collect();
    named.join(", ")
}

/// A weapon's damage as its rolls say it: dice, bonus dice, flat bonus,
/// and other types' extras.
fn damage(weapon: &Weapon) -> String {
    let mut parts = vec![roll(Roll {
        dice: weapon.dice,
        die: weapon.die,
        bonus: weapon.damage_bonus,
    })];
    if weapon.bonus_dice > 0 {
        parts.push(format!("{}d{}", weapon.bonus_dice, weapon.bonus_die));
    }
    for extra in weapon.extras.iter().flatten() {
        let kind = DAMAGE_FLAGS
            .get(usize::from(extra.damage_type))
            .copied()
            .unwrap_or("its own type");
        parts.push(format!("{} {kind}", roll(extra.roll)));
    }
    parts.retain(|part| !part.is_empty());
    parts.join(" + ")
}

/// The mean of a weapon's own roll (dice and flat bonus).
fn mean(weapon: &Weapon) -> f32 {
    let dice = |count: u8, sides: u8| f32::from(count) * (f32::from(sides) + 1.0) / 2.0;
    dice(weapon.dice, weapon.die)
        + dice(weapon.bonus_dice, weapon.bonus_die)
        + f32::from(weapon.damage_bonus)
}

fn critical(weapon: &Weapon) -> String {
    let from = 21 - i32::from(weapon.crit_threat.max(1));
    let range = if from >= 20 {
        "20".to_string()
    } else {
        format!("{from}–20")
    };
    format!("{range} ×{}", weapon.crit_mult)
}

/// The rules that apply to any of `tags`, each once, as links.
fn applying(tags: &[String]) -> Vec<&'static Entry> {
    let mut out: Vec<&'static Entry> = Vec::new();
    for entry in tags.iter().flat_map(|tag| rules::applying(tag)) {
        if !out.iter().any(|known| known.path == entry.path) {
            out.push(entry);
        }
    }
    out
}

fn applying_html(tags: &[String]) -> String {
    let entries = applying(tags);
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::from("<div class=\"applies\"><b>Rules</b><ul>");
    for entry in entries {
        let value = match entry.kind {
            Kind::Rule { .. } => format!(" = <b>{}</b>", escape(&entry.value())),
            Kind::Section => String::new(),
        };
        let summary = entry
            .summary
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let _ = write!(
            out,
            "<li><a href=\"#{}\">{}</a>{value} <span class=\"path\">{}</span><br><span class=\"dim\">{}</span></li>",
            anchor(entry.path),
            escape(entry.title),
            entry.path,
            escape(&summary)
        );
    }
    out.push_str("</ul></div>");
    out
}

/// One card: a thing's title, its game text, its facts, its rules.
fn card(
    id: &str,
    title: &str,
    sub: &str,
    facts: &[(&str, String)],
    text: &str,
    tags: &[String],
) -> String {
    let mut out = format!(
        "<article class=\"card\" id=\"{id}\" data-search=\"{}\"><h4>{} <span class=\"path\">{}</span></h4>",
        escape(&format!("{title} {sub} {text}").to_lowercase()),
        escape(title),
        escape(sub)
    );
    if !facts.is_empty() {
        out.push_str("<dl>");
        for (name, value) in facts.iter().filter(|(_, value)| !value.is_empty()) {
            let _ = write!(out, "<dt>{name}</dt><dd>{}</dd>", escape(value));
        }
        out.push_str("</dl>");
    }
    if !text.is_empty() {
        let _ = write!(
            out,
            "<details><summary>Description</summary>{}</details>",
            prose(text)
        );
    }
    out.push_str(&applying_html(tags));
    out.push_str("</article>");
    out
}

fn section(out: &mut String, id: &str, title: &str, intro: &str, body: &str) {
    let _ = write!(
        out,
        "<section id=\"{id}\"><h2>{}</h2>{}{body}</section>",
        escape(title),
        prose(intro)
    );
}

fn rulebook_html() -> String {
    let mut out = String::new();
    for entry in rules::BOOK {
        let id = anchor(entry.path);
        match entry.kind {
            Kind::Section => {
                let level = (entry.depth + 2).min(6);
                let _ = write!(
                    out,
                    "<div class=\"book-section depth-{}\" id=\"{id}\" data-search=\"{}\"><h{level}>{} <span class=\"path\">{}</span></h{level}>{}{}</div>",
                    entry.depth,
                    escape(
                        &format!("{} {} {}", entry.title, entry.path, entry.summary).to_lowercase()
                    ),
                    escape(entry.title),
                    entry.path,
                    prose(entry.summary),
                    prose(entry.detail),
                );
            }
            Kind::Rule {
                unit,
                labels,
                source,
                see,
                constant,
                ty,
                ..
            } => {
                let mut table = String::new();
                if !labels.is_empty() {
                    table.push_str("<table class=\"labels\"><tr>");
                    for label in labels {
                        let _ = write!(table, "<th>{}</th>", escape(label));
                    }
                    table.push_str("</tr><tr>");
                    for value in entry.values {
                        let _ = write!(
                            table,
                            "<td>{}</td>",
                            escape(&rules::shown::said(value, unit))
                        );
                    }
                    table.push_str("</tr></table>");
                }
                let mut notes = Vec::new();
                if !source.is_empty() {
                    notes.push(format!("Source: {}", escape(source)));
                }
                if !see.is_empty() {
                    notes.push(format!("Code: <code>{}</code>", escape(see)));
                }
                notes.push(format!("Constant: <code>{constant}: {ty}</code>"));
                if !entry.applies.is_empty() {
                    notes.push(format!("Applies to: {}", escape(&entry.applies.join(", "))));
                }
                let _ = write!(
                    out,
                    "<div class=\"rule\" id=\"{id}\" data-search=\"{}\"><div class=\"rule-head\"><span class=\"rule-name\">{}</span><span class=\"value\">{}</span><span class=\"path\" title=\"Quote this to ask for a change\">{}</span></div>{table}{}{}<div class=\"notes\">{}</div></div>",
                    escape(
                        &format!(
                            "{} {} {} {}",
                            entry.title,
                            entry.path,
                            entry.summary,
                            entry.applies.join(" ")
                        )
                        .to_lowercase()
                    ),
                    escape(entry.title),
                    escape(&entry.value()),
                    entry.path,
                    prose(entry.summary),
                    prose(entry.detail),
                    notes.join(" · ")
                );
            }
        }
    }
    out
}

fn weapons_html(codex: &Codex) -> String {
    let pace = rules::combat::damage::PACE as f32;
    let mut out = String::from(
        "<table class=\"grid\"><tr><th>Weapon</th><th>Label</th><th>Damage</th><th>Mean roll</th><th>Critical</th><th>Types</th><th>Refire</th><th>Per hit</th><th>Per 3 s at even odds</th><th>Reach</th></tr>",
    );
    for entry in &codex.weapons {
        let weapon = &entry.weapon;
        let mean = mean(weapon);
        let per_hit = entry
            .refire_ms
            .map(|ms| format!("×{:.2}", pace * ms as f32 / sim::combat::ROUND_MS as f32))
            .unwrap_or_else(|| "by swing".to_string());
        let _ = write!(
            out,
            "<tr data-search=\"{}\"><td><a href=\"#w-{}\">{}</a></td><td><code>{}</code></td><td>{}</td><td>{mean:.1}</td><td>{}</td><td>{}</td><td>{}</td><td>{per_hit}</td><td>{:.1}</td><td>{}</td></tr>",
            escape(&format!("{} {}", entry.row.name, entry.row.label).to_lowercase()),
            entry.row.row,
            escape(&entry.row.name),
            escape(&entry.row.label),
            escape(&damage(weapon)),
            critical(weapon),
            types(weapon.damage_types),
            entry
                .refire_ms
                .map_or("—".to_string(), |ms| format!("{ms} ms")),
            mean * pace * 0.5,
            if weapon.reach_m > 0 {
                format!("{} m", weapon.reach_m)
            } else {
                "melee".to_string()
            },
        );
    }
    out.push_str("</table>");
    for entry in &codex.weapons {
        out.push_str(&card(
            &format!("w-{}", entry.row.row),
            &entry.row.name,
            &entry.row.label,
            &[
                ("Damage", damage(&entry.weapon)),
                ("Critical", critical(&entry.weapon)),
                ("Types", types(entry.weapon.damage_types)),
                ("Delivered as", format!("{:?}", entry.weapon.delivery)),
            ],
            &entry.row.description,
            &entry.row.tags,
        ));
    }
    out
}

fn items_html(codex: &Codex) -> String {
    let mut out = String::new();
    for item in &codex.items {
        let mut facts = vec![
            ("Template", item.template.clone()),
            ("Base item", item.base_label.clone()),
            ("Value", format!("{} credits", item.cost)),
        ];
        if let Some(weapon) = &item.weapon {
            facts.push(("Damage", damage(weapon)));
            facts.push(("Critical", critical(weapon)));
            if weapon.attack_bonus != 0 {
                facts.push(("To hit", format!("{:+}", weapon.attack_bonus)));
            }
            if weapon.enhancement > 0 {
                facts.push(("Enhancement", format!("+{}", weapon.enhancement)));
            }
        }
        if let Some(armour) = item
            .armour
            .filter(|armour| armour.defence != 0 || armour.dex_cap.is_some())
        {
            facts.push(("Defence", format!("{:+}", armour.defence)));
            if let Some(cap) = armour.dex_cap {
                facts.push(("DEX cap", format!("+{cap}")));
            }
        }
        if let Some(row) = item.casts {
            if let Some(effect) = sim::combat::grenades::effect(row) {
                facts.push(("Blast", grenade(&effect)));
            } else if let Some(use_of) = sim::combat::items::use_of(row) {
                facts.push(("Use", format!("{use_of:?}")));
            }
        }
        let name = if item.name.is_empty() {
            &item.template
        } else {
            &item.name
        };
        out.push_str(&card(
            &format!("i-{}", item.template),
            name,
            &item.base_label,
            &facts,
            &item.description,
            &item.tags,
        ));
    }
    out
}

fn grenade(effect: &sim::combat::grenades::Effect) -> String {
    let kind = DAMAGE_FLAGS
        .get(usize::from(effect.damage_type))
        .copied()
        .unwrap_or("untyped");
    let mut text = format!("{} {kind}", effect.points);
    if effect.droid_points != effect.points {
        let _ = write!(text, " ({} on droids)", effect.droid_points);
    }
    if let Some(dc) = effect.dc {
        let _ = write!(
            text,
            ", Reflex DC {dc} {}",
            if effect.save_halves {
                "halves"
            } else {
                "spares"
            }
        );
    }
    if !matches!(effect.secondary, sim::combat::grenades::Secondary::None) {
        let _ = write!(text, ", then {:?}", effect.secondary);
    }
    text
}

fn spells_html(codex: &Codex, powers: bool) -> String {
    let mut out = String::new();
    for spell in codex
        .spells
        .iter()
        .filter(|spell| spell.delivered.is_some() == powers)
    {
        let mut facts = vec![
            (
                "Force points",
                spell.cost.map_or(String::new(), |cost| cost.to_string()),
            ),
            (
                "Side",
                match spell.side.as_str() {
                    "L" => "light".to_string(),
                    "D" => "dark".to_string(),
                    _ => String::new(),
                },
            ),
            ("Impact script", spell.impact.clone()),
        ];
        let levels: Vec<String> = ["Guardian", "Consular", "Sentinel"]
            .iter()
            .zip(spell.levels)
            .filter_map(|(class, level)| Some(format!("{class} {}", level?)))
            .collect();
        facts.push(("Learnt at", levels.join(", ")));
        if let Some((power, level)) = &spell.delivered {
            facts.push(("Plays as", format!("JA's {power}, level {level}")));
        }
        if let Some(effect) = sim::combat::grenades::effect(spell.row.row) {
            facts.push(("Blast", grenade(&effect)));
        }
        if let Some(use_of) = sim::combat::items::use_of(spell.row.row) {
            facts.push(("Use", format!("{use_of:?}")));
        }
        if !powers
            && facts
                .iter()
                .all(|(name, value)| value.is_empty() || *name == "Impact script")
            && applying(&spell.row.tags).is_empty()
        {
            continue;
        }
        out.push_str(&row_card("s", &spell.row, &facts));
    }
    out
}

fn row_card(prefix: &str, row: &Row, facts: &[(&str, String)]) -> String {
    let name = if row.name.is_empty() {
        &row.label
    } else {
        &row.name
    };
    card(
        &format!("{prefix}-{}", row.row),
        name,
        &row.label,
        facts,
        &row.description,
        &row.tags,
    )
}

fn page(game: Game, codex: &Codex, changes: &[Changed], bestiary: &[Module]) -> String {
    let mut body = String::new();
    section(
        &mut body,
        "how",
        "Changes",
        &format!(
            "Every rule shows its path, such as `combat.damage.pace`. A change is asked for by its path and new value. Generated by `kjk rules {}`, rulebook edition `{:016x}`.",
            game.key(),
            rules::EDITION
        ),
        "",
    );
    let mut controls =
        String::from("<table class=\"grid\"><tr><th>Action</th><th>Default key</th></tr>");
    for (label, key) in keys() {
        let _ = write!(
            controls,
            "<tr><td>{}</td><td><kbd>{}</kbd></td></tr>",
            escape(label),
            escape(key)
        );
    }
    controls.push_str("</table>");
    section(
        &mut body,
        "keys",
        "Keys",
        "Default keys. Attacks, combos and specials: Controls, in the rulebook.",
        &controls,
    );
    section(&mut body, "rulebook", "The rulebook", "", &rulebook_html());
    let mut changed = String::new();
    if changes.is_empty() {
        changed.push_str("<p class=\"dim\">None: every table is the install's own.</p>");
    } else {
        changed.push_str("<table class=\"grid\"><tr><th>Table</th><th>Row</th><th>Column</th><th>Install</th><th>Now</th><th>Why</th></tr>");
        for changed_cell in changes {
            let change = changed_cell.change;
            let _ = write!(
                changed,
                "<tr><td><code>{}.2da</code></td><td>{}</td><td><code>{}</code></td><td>{}</td><td><b>{}</b></td><td>{}</td></tr>",
                change.table,
                escape(change.row),
                escape(change.column),
                escape(&changed_cell.was),
                escape(change.value),
                escape(change.why)
            );
        }
        changed.push_str("</table>");
    }
    section(
        &mut body,
        "changes",
        "Changes to the game's tables",
        &format!(
            "Cells of {}'s own 2DA tables the rulebook changes (`data/rules/tables/{}/`): the game reads its tables so changed.",
            game.key(),
            game.key()
        ),
        &changed,
    );
    section(
        &mut body,
        "weapons",
        "Weapons",
        "Damage per hit is the roll × the chance to hit × the pace. Per 3 seconds: at a 50% chance to hit, before criticals, Strength and feats.",
        &weapons_html(codex),
    );
    section(
        &mut body,
        "powers",
        "Force powers",
        "Damage, saves and durations are the impact script's; cost is the table's.",
        &spells_html(codex, true),
    );
    section(
        &mut body,
        "abilities",
        "Grenades, items' and creatures' abilities",
        "",
        &spells_html(codex, false),
    );
    let mut items = String::new();
    items.push_str(&items_html(codex));
    section(&mut body, "items", "Items", "", &items);
    let mut feats = String::new();
    for feat in &codex.feats {
        feats.push_str(&row_card("f", feat, &[]));
    }
    section(&mut body, "feats", "Feats", "", &feats);
    let mut skills = String::new();
    for skill in &codex.skills {
        skills.push_str(&row_card("k", skill, &[]));
    }
    section(&mut body, "skills", "Skills", "", &skills);
    let mut classes = String::new();
    for class in &codex.classes {
        classes.push_str(&row_card(
            "c",
            &class.row,
            &[
                (
                    "Hit die",
                    class.hit_die.map_or(String::new(), |die| format!("d{die}")),
                ),
                (
                    "Force die",
                    class
                        .force_die
                        .map_or(String::new(), |die| format!("d{die}")),
                ),
                (
                    "Skill points",
                    class
                        .skill_points
                        .map_or(String::new(), |points| format!("{points} + INT a level")),
                ),
                ("Attack table", class.attack_table.clone()),
                ("Saves table", class.save_table.clone()),
            ],
        ));
    }
    section(&mut body, "classes", "Classes", "", &classes);
    let mut xp = String::from("<table class=\"grid\"><tr><th>Level</th><th>Experience</th></tr>");
    for (level, points) in codex.experience.iter().enumerate() {
        let _ = write!(xp, "<tr><td>{}</td><td>{points}</td></tr>", level + 1);
    }
    xp.push_str("</table>");
    section(&mut body, "experience", "Experience", "", &xp);
    section(
        &mut body,
        "creatures",
        "Creatures",
        "By module, as placed: vitality points alone and with four players, Force points, Defense, attack bonus, attributes, weapons.",
        &creatures_html(codex, bestiary),
    );

    let mut nav = String::new();
    for (id, title) in [
        ("how", "Changes"),
        ("keys", "Keys"),
        ("rulebook", "The rulebook"),
        ("changes", "Table changes"),
        ("weapons", "Weapons"),
        ("powers", "Force powers"),
        ("abilities", "Grenades & abilities"),
        ("items", "Items"),
        ("feats", "Feats"),
        ("skills", "Skills"),
        ("classes", "Classes"),
        ("experience", "Experience"),
        ("creatures", "Creatures"),
    ] {
        let _ = write!(nav, "<a href=\"#{id}\">{title}</a>");
        if id == "rulebook" {
            for chapter in rules::BOOK.iter().filter(|entry| entry.depth == 0) {
                let _ = write!(
                    nav,
                    "<a class=\"sub\" href=\"#{}\">{}</a>",
                    anchor(chapter.path),
                    escape(chapter.title)
                );
            }
        }
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Rulebook {game}</title><style>{STYLE}</style></head><body><nav><div class=\"brand\">Rulebook · {game}</div><input id=\"q\" type=\"search\" placeholder=\"Search rules, powers, items…\" autofocus>{nav}</nav><main>{body}</main><script>{SCRIPT}</script></body></html>",
        game = game.key(),
    )
}

/// The same facts as data, for tools.
fn facts(game: Game, codex: &Codex, changes: &[Changed], bestiary: &[Module]) -> Value {
    let row = |row: &Row| {
        json!({
            "row": row.row, "label": row.label, "name": row.name,
            "tags": row.tags,
            "rules": applying(&row.tags).iter().map(|entry| entry.path).collect::<Vec<_>>(),
        })
    };
    let weapon = |weapon: &Weapon| {
        json!({
            "damage": damage(weapon), "mean_roll": mean(weapon), "critical": critical(weapon),
            "types": types(weapon.damage_types), "to_hit": weapon.attack_bonus,
            "enhancement": weapon.enhancement, "delivery": format!("{:?}", weapon.delivery),
        })
    };
    json!({
        "game": game.key(),
        "edition": format!("{:016x}", rules::EDITION),
        "rules": rules::BOOK.iter().filter(|entry| entry.is_rule()).map(|entry| json!({
            "path": entry.path, "title": entry.title, "value": entry.values,
            "shown": entry.value(), "summary": entry.summary, "applies": entry.applies,
        })).collect::<Vec<_>>(),
        "table_changes": changes.iter().map(|changed| json!({
            "table": changed.change.table, "row": changed.change.row, "column": changed.change.column,
            "install": changed.was, "now": changed.change.value, "why": changed.change.why,
        })).collect::<Vec<_>>(),
        "weapons": codex.weapons.iter().map(|entry| json!({
            "base": row(&entry.row), "weapon": weapon(&entry.weapon), "refire_ms": entry.refire_ms,
        })).collect::<Vec<_>>(),
        "items": codex.items.iter().map(|item| json!({
            "template": item.template, "name": item.name, "base": item.base_label, "cost": item.cost,
            "weapon": item.weapon.as_ref().map(weapon), "casts": item.casts, "tags": item.tags,
        })).collect::<Vec<_>>(),
        "spells": codex.spells.iter().map(|spell| json!({
            "spell": row(&spell.row), "cost": spell.cost, "side": spell.side,
            "levels": spell.levels, "impact": spell.impact, "delivered": spell.delivered,
        })).collect::<Vec<_>>(),
        "feats": codex.feats.iter().map(row).collect::<Vec<_>>(),
        "skills": codex.skills.iter().map(row).collect::<Vec<_>>(),
        "classes": codex.classes.iter().map(|class| json!({
            "class": row(&class.row), "hit_die": class.hit_die, "force_die": class.force_die,
            "skill_points": class.skill_points,
        })).collect::<Vec<_>>(),
        "experience": codex.experience,
        "creatures": bestiary.iter().map(|(module, placed)| json!({
            "module": module,
            "creatures": placed.iter().map(|creature| json!({
                "area": creature.area, "tag": creature.tag, "template": creature.template,
                "name": creature.name, "level": creature.sheet.level, "classes": creature.classes,
                "hp": creature.hp_one, "hp_four_players": creature.hp_four,
                "fp": creature.sheet.force_points(), "defence": creature.sheet.defence(),
                "abilities": creature.sheet.abilities, "hostile": creature.hostile,
                "weapons": creature.sheet.weapons.iter().flatten().map(damage).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

const STYLE: &str = r#"
:root{--bg:#f7f7f5;--panel:#fff;--ink:#1d1d1f;--dim:#6b6b70;--line:#e3e3e0;--accent:#2357c6;--value:#0f6b3a;--code:#f0f0ec}
@media (prefers-color-scheme:dark){:root{--bg:#141416;--panel:#1d1d20;--ink:#ececef;--dim:#9a9aa2;--line:#2e2e33;--accent:#7aa5ff;--value:#6fd59b;--code:#26262b}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif;display:flex}
nav{position:sticky;top:0;height:100vh;overflow:auto;width:260px;flex:none;padding:16px;border-right:1px solid var(--line);background:var(--panel)}
nav a{display:block;color:var(--ink);text-decoration:none;padding:3px 6px;border-radius:6px}nav a:hover{background:var(--code)}nav a.sub{padding-left:18px;font-size:13px;color:var(--dim)}
.brand{font-weight:700;margin-bottom:10px}#q{width:100%;padding:8px 10px;border:1px solid var(--line);border-radius:8px;background:var(--bg);color:var(--ink);margin-bottom:12px;font:inherit}
main{flex:1;min-width:0;padding:24px 32px;max-width:1100px}section{margin-bottom:48px}h2{font-size:26px;border-bottom:2px solid var(--line);padding-bottom:6px}
h2,h3,h4,h5,h6{line-height:1.25}code{background:var(--code);padding:1px 5px;border-radius:4px;font-size:13px}
.path{font:12px ui-monospace,Menlo,monospace;color:var(--dim);font-weight:400;margin-left:8px;user-select:all}
.book-section.depth-0{margin-top:36px}.book-section.depth-0 h2{font-size:22px}
.rule{background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:12px 16px;margin:10px 0}
.rule-head{display:flex;flex-wrap:wrap;align-items:baseline;gap:10px}.rule-name{font-weight:650}.value{font:600 15px ui-monospace,Menlo,monospace;color:var(--value)}
.rule p{margin:6px 0}.notes{font-size:12px;color:var(--dim);margin-top:6px}
table{border-collapse:collapse}table.labels{margin:6px 0}table.labels th,table.labels td{border:1px solid var(--line);padding:3px 8px;font-size:13px}
table.grid{width:100%;font-size:13px;background:var(--panel);border:1px solid var(--line);border-radius:10px;overflow:hidden;display:block;overflow-x:auto}
table.grid th,table.grid td{padding:5px 8px;border-bottom:1px solid var(--line);text-align:left;white-space:nowrap}table.grid th{background:var(--code)}
.card{background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:12px 16px;margin:10px 0}.card h4{margin:0 0 6px}
dl{display:grid;grid-template-columns:max-content 1fr;gap:2px 14px;margin:6px 0;font-size:14px}dt{color:var(--dim)}dd{margin:0}
details{margin:6px 0;color:var(--dim)}summary{cursor:pointer}.applies{margin-top:8px;font-size:14px}.applies ul{margin:4px 0;padding-left:18px}
kbd{font:600 12px ui-monospace,Menlo,monospace;background:var(--code);border:1px solid var(--line);border-bottom-width:2px;border-radius:5px;padding:0 5px;white-space:nowrap}
.dim{color:var(--dim)}a{color:var(--accent)}.hidden{display:none!important}
@media (max-width:760px){body{display:block}nav{position:static;height:auto;width:auto;border-right:0;border-bottom:1px solid var(--line)}main{padding:16px}}
"#;

const SCRIPT: &str = r#"
const q=document.getElementById('q');
q.addEventListener('input',()=>{const t=q.value.trim().toLowerCase();
document.querySelectorAll('[data-search]').forEach(e=>{e.classList.toggle('hidden',t!==''&&!e.dataset.search.includes(t));});});
"#;
