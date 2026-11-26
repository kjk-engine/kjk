//! Commands that change the party or the story, through the console's own
//! orders (`sim::action::CheatOrder`).

use sim::action::{Action, CheatOrder, Mark};
use sim::cheats::limits;

use super::{Ran, order};
use crate::{Alias, Arg, ArgKind, Facts, Group, Kind, Parsed, Place, Refusal, Spec};

/// An amount of credits or experience that one command adds or sets.
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
        help: "Adds the item of this template to the party's inventory. With COUNT, adds that many of it.",
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
        help: "Adds this many credits to the party's credits.",
        example: "givecredits 500",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: givecredits,
    },
    Spec {
        name: "credits",
        aliases: &[],
        args: &[Arg::new("AMOUNT", CREDITS)],
        help: "Sets the party's credits to this amount.",
        example: "credits 10000",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: credits,
    },
    Spec {
        name: "addexp",
        aliases: &[Alias::plain("addxp")],
        args: &[Arg::new("AMOUNT", XP)],
        help: "Adds this many experience points to the party's experience.",
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
        help: "Gives the party the experience that the player's character needs for its next level.",
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
        help: "Gives the party member WHO full hit points and Force points. Without WHO, heals every party member the player may change.",
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
        help: "Kills the creature the player has targeted, or the nearest creature with this tag. Its death script runs as for any other death. Party members and creatures that the story needs cannot be killed.",
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
        help: "Sets the script global NAME to VALUE. A global that holds true or false becomes true for any value but 0.",
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
            Arg::new("RESREF", ArgKind::Word),
            Arg::optional("WHO", ArgKind::Who),
        ],
        help: "Runs the game's script RESREF. With WHO, the party member WHO runs it. Without WHO, the module runs it, as it runs its own scripts.",
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
    let said = format!("Giving {count} of {template}.");
    cheat(CheatOrder::Item { template, count }, said)
}

/// The party's credits or experience, which a command that adds to them
/// reads.
fn known(value: Option<i32>, what: &str) -> Result<i64, Refusal> {
    value
        .map(i64::from)
        .ok_or_else(|| Refusal(format!("The party's {what} are not known yet.")))
}

/// `amount` held to the order's limit.
fn held(amount: i64, most: i32) -> i32 {
    i32::try_from(amount.clamp(0, i64::from(most))).unwrap_or(most)
}

fn givecredits(parsed: &Parsed, facts: &Facts) -> Ran {
    let total = held(
        known(facts.credits, "credits")? + parsed.int(0),
        limits::CREDITS_MOST,
    );
    cheat(
        CheatOrder::Credits(total),
        format!("Setting the party's credits to {total}."),
    )
}

fn credits(parsed: &Parsed, _: &Facts) -> Ran {
    let total = held(parsed.int(0), limits::CREDITS_MOST);
    cheat(
        CheatOrder::Credits(total),
        format!("Setting the party's credits to {total}."),
    )
}

fn addexp(parsed: &Parsed, facts: &Facts) -> Ran {
    let total = held(
        known(facts.xp, "experience points")? + parsed.int(0),
        limits::XP_MOST,
    );
    cheat(
        CheatOrder::Xp(total),
        format!("Setting the party's experience to {total}."),
    )
}

fn addlevel(_: &Parsed, facts: &Facts) -> Ran {
    let now = known(facts.xp, "experience points")?;
    let Some(next) = facts
        .next_level_xp
        .map(i64::from)
        .filter(|next| *next > now)
    else {
        return Err(Refusal::new(
            "The player's character has no next level to reach.",
        ));
    };
    let total = held(next, limits::XP_MOST);
    cheat(
        CheatOrder::Xp(total),
        format!("Setting the party's experience to {total}."),
    )
}

fn heal(parsed: &Parsed, facts: &Facts) -> Ran {
    let (who, said) = match parsed.given(0) {
        true => (Some(parsed.who(0, facts)?), "Healing the party member."),
        false => (None, "Healing the party."),
    };
    cheat(CheatOrder::Heal { who }, said)
}

fn kill(parsed: &Parsed, facts: &Facts) -> Ran {
    let (who, said) = match (parsed.word(0), facts.target) {
        (Some(tag), _) => (Mark::Tag(tag.to_string()), format!("Killing {tag}.")),
        (None, Some(target)) => (Mark::Id(target), "Killing the target.".to_string()),
        (None, None) => {
            return Err(Refusal::new(
                "Target a creature first, or name one by its tag.",
            ));
        }
    };
    cheat(CheatOrder::Kill { who }, said)
}

fn setglobal(parsed: &Parsed, _: &Facts) -> Ran {
    let name = parsed.word(0).unwrap_or("").to_string();
    let value = i32::try_from(parsed.int(1)).unwrap_or(0);
    let said = format!("Setting {name} to {value}.");
    cheat(CheatOrder::Global { name, value }, said)
}

fn runscript(parsed: &Parsed, facts: &Facts) -> Ran {
    let resref = parsed.word(0).unwrap_or("").to_ascii_lowercase();
    let who = match parsed.given(1) {
        true => Some(parsed.who(1, facts)?),
        false => None,
    };
    let said = format!("Running {resref}.");
    cheat(CheatOrder::Script { resref, who }, said)
}
