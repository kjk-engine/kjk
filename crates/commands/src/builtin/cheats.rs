//! Commands that change the party or the story, through the console's own
//! orders (`sim::action::CheatOrder`).

use sim::action::{Action, CheatOrder, Mark, ScriptParams};
use sim::cheats::limits;

use super::{Ran, order};
use crate::{Alias, Arg, ArgKind, Facts, Group, Kind, Parsed, Place, Refusal, Spec};

/// An amount of credits or experience that one command adds.
const CREDITS: ArgKind = ArgKind::Int {
    min: 0,
    max: limits::CREDITS_MOST as i64,
};
const XP: ArgKind = ArgKind::Int {
    min: 0,
    max: limits::XP_MOST as i64,
};
const COUNT: ArgKind = ArgKind::Int {
    min: 1,
    max: limits::ITEMS_MOST as i64,
};
const GLOBAL: ArgKind = ArgKind::Int {
    min: i32::MIN as i64,
    max: i32::MAX as i64,
};

pub static SPECS: &[Spec] = &[
    Spec {
        name: "giveitem",
        aliases: &[],
        args: &[
            Arg::new("TEMPLATE", ArgKind::Item),
            Arg::optional("COUNT", COUNT),
        ],
        help: "Add an item (TEMPLATE) to party inventory, optionally specifying COUNT.",
        example: "giveitem g_w_lghtsbr01 2",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: giveitem,
    },
    Spec {
        name: "givecredits",
        aliases: &[],
        args: &[Arg::new("AMOUNT", CREDITS)],
        help: "Add AMOUNT credits to the party (capped at 9,999,999).",
        example: "givecredits 500",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: givecredits,
    },
    Spec {
        name: "addexp",
        aliases: &[Alias::plain("addxp")],
        args: &[Arg::new("AMOUNT", XP)],
        help: "Add AMOUNT XP to the party.",
        example: "addexp 2000",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: addexp,
    },
    Spec {
        name: "addlevel",
        aliases: &[],
        args: &[],
        help: "Give the party enough XP for your next level.",
        example: "addlevel",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: addlevel,
    },
    Spec {
        name: "heal",
        aliases: &[],
        args: &[Arg::optional("WHO", ArgKind::Who)],
        help: "Restore full Vitality and Force to party member WHO, or to everyone you control.",
        example: "heal",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: heal,
    },
    Spec {
        name: "kill",
        aliases: &[],
        args: &[Arg::optional("TAG", ArgKind::Word)],
        help: "Kill your current target, or the nearest creature matching TAG. Triggers death scripts normally. Cannot kill party members or plot-essential NPCs.",
        example: "kill",
        group: Group::World,
        place: Place::Game,
        kind: Kind::Cheat,
        run: kill,
    },
    Spec {
        name: "setglobal",
        aliases: &[],
        args: &[Arg::new("NAME", ArgKind::Word), Arg::new("VALUE", GLOBAL)],
        help: "Set global variable NAME to VALUE. For boolean globals, 0 = false and any other value = true.",
        example: "setglobal K_SWG_HELENA 1",
        group: Group::World,
        place: Place::Game,
        kind: Kind::Cheat,
        run: setglobal,
    },
    Spec {
        name: "runscript",
        aliases: &[],
        args: &[
            Arg::new("RESREF", ArgKind::Script),
            Arg::optional("WHO", ArgKind::Who),
            Arg::optional("PARAMETERS", ArgKind::Text),
        ],
        help: "Run script RESREF on the module, or on party member WHO. In KOTOR 2, pass up to 5 integers and a trailing string after WHO (read via GetScriptParameter / GetScriptStringParameter).",
        example: "runscript k_sup_gohawk",
        group: Group::World,
        place: Place::Game,
        kind: Kind::Cheat,
        run: runscript,
    },
];

fn cheat(given: CheatOrder, said: impl Into<String>) -> Ran {
    order(Action::Cheat(given), said)
}

fn giveitem(parsed: &Parsed, _: &Facts) -> Ran {
    let template = parsed.word(0).unwrap_or("").to_string();
    let count = match parsed.given(1) {
        true => u16::try_from(parsed.int(1)).unwrap_or(1),
        false => 1,
    };
    let said = format!("Added {count}x {template}.");
    cheat(CheatOrder::Item { template, count }, said)
}

/// The party's experience, which `addlevel` reads.
fn known(value: Option<i32>) -> Result<i64, Refusal> {
    value
        .map(i64::from)
        .ok_or_else(|| Refusal::new("Party XP isn't loaded yet."))
}

/// `amount` held to the order's limit.
fn held(amount: i64, most: i32) -> i32 {
    i32::try_from(amount.clamp(0, i64::from(most))).unwrap_or(most)
}

fn givecredits(parsed: &Parsed, _: &Facts) -> Ran {
    let amount = held(parsed.int(0), limits::CREDITS_MOST);
    cheat(
        CheatOrder::Credits(amount),
        format!("Added {amount} credits."),
    )
}

fn addexp(parsed: &Parsed, _: &Facts) -> Ran {
    let amount = held(parsed.int(0), limits::XP_MOST);
    cheat(CheatOrder::Xp(amount), format!("Added {amount} XP."))
}

fn addlevel(_: &Parsed, facts: &Facts) -> Ran {
    let now = known(facts.xp)?;
    let Some(next) = facts
        .next_level_xp
        .map(i64::from)
        .filter(|next| *next > now)
    else {
        return Err(Refusal::new("You are already at max level."));
    };
    let delta = held(next - now, limits::XP_MOST);
    cheat(
        CheatOrder::Xp(delta),
        format!("Added {delta} XP for next level."),
    )
}

fn heal(parsed: &Parsed, facts: &Facts) -> Ran {
    let (who, said) = match parsed.given(0) {
        true => (Some(parsed.who(0, facts)?), "Healed party member."),
        false => (None, "Healed party."),
    };
    cheat(CheatOrder::Heal { who }, said)
}

fn kill(parsed: &Parsed, facts: &Facts) -> Ran {
    let (who, said) = match (parsed.word(0), facts.target) {
        (Some(tag), _) => (Mark::Tag(tag.to_string()), format!("Killing {tag}.")),
        (None, Some(target)) => (Mark::Id(target), "Killing target.".to_string()),
        (None, None) => {
            return Err(Refusal::new("Target a creature or specify a TAG."));
        }
    };
    cheat(CheatOrder::Kill { who }, said)
}

fn setglobal(parsed: &Parsed, _: &Facts) -> Ran {
    let name = parsed.word(0).unwrap_or("").to_string();
    let value = i32::try_from(parsed.int(1)).unwrap_or(0);
    let said = format!("Set {name} = {value}.");
    cheat(CheatOrder::Global { name, value }, said)
}

fn runscript(parsed: &Parsed, facts: &Facts) -> Ran {
    let resref = parsed.word(0).unwrap_or("").to_ascii_lowercase();
    let who = match parsed.given(1) {
        true => Some(parsed.who(1, facts)?),
        false => None,
    };
    let params = script_params(parsed.word(2).unwrap_or(""))?;
    let said = format!("Running script {resref}.");
    cheat(
        CheatOrder::Script {
            resref,
            who,
            params,
        },
        said,
    )
}

/// `runscript`'s parameters: the whole numbers first, in order, for
/// `GetScriptParameter(1)` to `GetScriptParameter(5)`. The first word that
/// is not a whole number starts the text for `GetScriptStringParameter`,
/// which runs to the end of the line.
fn script_params(words: &str) -> Result<ScriptParams, Refusal> {
    let mut params = ScriptParams::default();
    let mut numbers = 0;
    let mut rest = words.split_ascii_whitespace().peekable();
    while let Some(number) = rest.peek().and_then(|word| word.parse::<i32>().ok()) {
        let Some(slot) = params.numbers.get_mut(numbers) else {
            return Err(Refusal(format!(
                "Too many integer parameters (max {}).",
                params.numbers.len()
            )));
        };
        *slot = number;
        numbers += 1;
        rest.next();
    }
    params.text = rest.collect::<Vec<_>>().join(" ");
    Ok(params)
}
