//! Skill checks of a conversation: a line or a reply offered only when the
//! character in the conversation has a skill of a score at least as high
//! as the check asks, and when the party carries the items the check
//! costs. The items are spent when the line or reply is used.
//!
//! A check is picked from lists in the editor, not written. The editor
//! writes it as two scripts of the project, as it writes logic blocks
//! (`blocks`):
//!
//! - a condition (`kjc_N.nss`), a `StartingConditional` set on the link
//!   that leads to the line or reply. Its header holds the check
//!   (`//@check SKILL SCORE COST`).
//! - when the check costs items, a script the line or reply runs
//!   (`kjp_N.nss`). It spends the items, then runs the script the line or
//!   reply ran before, if it ran one. Its header holds the cost and that
//!   script (`//@pay SKILL COST SCRIPT`, `-` for none).
//!
//! The score is the skill's total, as `GetSkillRank` gives it and the
//! character sheet shows it: the ranks, the key ability's modifier, and
//! what feats, items and effects add. The items are counted from the stack
//! the party carries (`GetItemPossessedBy` and `GetNumStackedItems`), as
//! KOTOR's own computer terminals count spikes and repair parts.
//!
//! Editing either script by hand drops its header, and the editor then
//! treats it as any other script.

/// One skill a check can ask for.
pub struct Skill {
    /// The skill's key in a check's header and in the console.
    pub key: &'static str,
    /// The skill's name as the game shows it.
    pub title: &'static str,
    /// The skill's constant in NWScript.
    constant: &'static str,
    /// What a check of this skill may cost: the item's tag, and its name
    /// for one and for several.
    pub item: Option<(&'static str, &'static str, &'static str)>,
}

/// The skills a check can ask for, in the order the editor offers them.
/// Computer Use, Repair and Security may cost items. Computer Use spends
/// computer spikes (`g_i_progspike01`), Repair spends repair parts
/// (`g_i_parts01`), and Security spends security spikes
/// (`g_i_secspike01`).
pub const SKILLS: &[Skill] = &[
    Skill {
        key: "computer",
        title: "Computer Use",
        constant: "SKILL_COMPUTER_USE",
        item: Some(("K_COMPUTER_SPIKE", "computer spike", "computer spikes")),
    },
    Skill {
        key: "repair",
        title: "Repair",
        constant: "SKILL_REPAIR",
        item: Some(("K_REPAIR_PART", "repair part", "repair parts")),
    },
    Skill {
        key: "security",
        title: "Security",
        constant: "SKILL_SECURITY",
        item: Some(("g_i_secspike01", "security spike", "security spikes")),
    },
    Skill {
        key: "persuade",
        title: "Persuade",
        constant: "SKILL_PERSUADE",
        item: None,
    },
    Skill {
        key: "awareness",
        title: "Awareness",
        constant: "SKILL_AWARENESS",
        item: None,
    },
    Skill {
        key: "demolitions",
        title: "Demolitions",
        constant: "SKILL_DEMOLITIONS",
        item: None,
    },
    Skill {
        key: "stealth",
        title: "Stealth",
        constant: "SKILL_STEALTH",
        item: None,
    },
    Skill {
        key: "treat",
        title: "Treat Injury",
        constant: "SKILL_TREAT_INJURY",
        item: None,
    },
];

/// The skill of `key`.
pub fn skill(key: &str) -> Option<&'static Skill> {
    SKILLS
        .iter()
        .find(|skill| skill.key.eq_ignore_ascii_case(key))
}

/// The highest score a check can ask for.
pub const MOST_SCORE: i32 = 99;
/// The most items a check can cost.
pub const MOST_COST: u32 = 99;

/// A skill check: the skill, the score it asks for, and the items it costs
/// (0 for none, and always 0 for a skill that costs no items).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub skill: String,
    pub score: i32,
    pub cost: u32,
}

impl Check {
    /// What the check asks, as a sentence: "Needs a Computer Use of 6, and
    /// 2 computer spikes, which are spent."
    pub fn described(&self) -> String {
        let Some(skill) = skill(&self.skill) else {
            return format!("{:?} is not a skill a check knows.", self.skill);
        };
        match (skill.item, self.cost) {
            (Some((_, one, _)), 1) => format!(
                "Needs a {} of {}, and 1 {one}, which is spent.",
                skill.title, self.score
            ),
            (Some((_, _, many)), cost) if cost > 1 => format!(
                "Needs a {} of {}, and {cost} {many}, which are spent.",
                skill.title, self.score
            ),
            _ => format!("Needs a {} of {}.", skill.title, self.score),
        }
    }

    /// The items the check costs: their tag and how many. `None` when it
    /// costs nothing.
    fn spent(&self) -> Option<(&'static str, u32)> {
        let (tag, _, _) = skill(&self.skill)?.item?;
        (self.cost > 0).then_some((tag, self.cost))
    }
}

/// The first line of a check's condition.
const MADE_CHECK: &str = "// Made by kjk's level editor from a skill check (docs/LEVEL-EDIT.md).";
/// The first line of the script that spends a check's cost.
const MADE_PAY: &str =
    "// Made by kjk's level editor to spend a skill check's cost (docs/LEVEL-EDIT.md).";
const CHECK_LINE: &str = "//@check ";
const PAY_LINE: &str = "//@pay ";

/// Whether `source` is a check's condition.
pub fn is_condition(source: &str) -> bool {
    source.starts_with(MADE_CHECK)
}

/// Whether `source` is a script that spends a check's cost.
pub fn is_payment(source: &str) -> bool {
    source.starts_with(MADE_PAY)
}

/// The header line of `source` that starts with `line`, its words after it.
fn header<'s>(source: &'s str, line: &str) -> Option<Vec<&'s str>> {
    let rest = source.lines().find_map(|text| text.strip_prefix(line))?;
    Some(rest.split_whitespace().collect())
}

/// The check a check's condition holds; `None` for any other script.
pub fn read_condition(source: &str) -> Option<Check> {
    if !is_condition(source) {
        return None;
    }
    let words = header(source, CHECK_LINE)?;
    let [skill, score, cost] = words.as_slice() else {
        return None;
    };
    Some(Check {
        skill: skill.to_ascii_lowercase(),
        score: score.parse().ok()?,
        cost: cost.parse().ok()?,
    })
}

/// What a script that spends a check's cost holds: the skill, the cost,
/// and the script it runs after (empty for none). `None` for any other
/// script.
pub fn read_payment(source: &str) -> Option<(String, u32, String)> {
    if !is_payment(source) {
        return None;
    }
    let words = header(source, PAY_LINE)?;
    let [skill, cost, then] = words.as_slice() else {
        return None;
    };
    let then = if *then == "-" { "" } else { then };
    Some((
        skill.to_ascii_lowercase(),
        cost.parse().ok()?,
        then.to_ascii_lowercase(),
    ))
}

/// The condition `check` makes: TRUE when the character in the
/// conversation has the skill's score, and the party carries the items.
pub fn condition(check: &Check) -> String {
    let mut body = String::from(
        "    object oPC = GetPCSpeaker();\n\
         \x20   if (!GetIsObjectValid(oPC))\n\
         \x20   {\n\
         \x20       oPC = GetFirstPC();\n\
         \x20   }\n",
    );
    match skill(&check.skill) {
        Some(skill) => body.push_str(&format!(
            "    if (GetSkillRank({}, oPC) < {})\n\
             \x20   {{\n\
             \x20       return FALSE;\n\
             \x20   }}\n",
            skill.constant, check.score
        )),
        None => body.push_str(&format!(
            "    // {:?} is not a skill a check knows.\n",
            check.skill
        )),
    }
    if let Some((tag, cost)) = check.spent() {
        body.push_str(&format!(
            "    object oItem = GetItemPossessedBy(GetFirstPC(), \"{tag}\");\n\
             \x20   if (!GetIsObjectValid(oItem) || GetNumStackedItems(oItem) < {cost})\n\
             \x20   {{\n\
             \x20       return FALSE;\n\
             \x20   }}\n"
        ));
    }
    format!(
        "{MADE_CHECK}\n\
         // Change the check in the conversation window; editing this file by hand ends it.\n\
         // {}\n\
         {CHECK_LINE}{} {} {}\n\
         \n\
         int StartingConditional()\n\
         {{\n\
         {body}\
         \x20   return TRUE;\n\
         }}\n",
        check.described(),
        check.skill,
        check.score,
        check.cost
    )
}

/// The script that spends `check`'s cost, then runs `then` (empty for
/// none). A check that costs nothing spends nothing.
pub fn payment(check: &Check, then: &str) -> String {
    let mut body = String::new();
    if let Some((tag, cost)) = check.spent() {
        body.push_str(&format!(
            "    object oItem = GetItemPossessedBy(GetFirstPC(), \"{tag}\");\n\
             \x20   int nHave = GetNumStackedItems(oItem);\n\
             \x20   if (nHave > {cost})\n\
             \x20   {{\n\
             \x20       SetItemStackSize(oItem, nHave - {cost});\n\
             \x20   }}\n\
             \x20   else if (GetIsObjectValid(oItem))\n\
             \x20   {{\n\
             \x20       DestroyObject(oItem);\n\
             \x20   }}\n"
        ));
    }
    let then = then.trim().to_ascii_lowercase();
    if !then.is_empty() {
        body.push_str(&format!("    ExecuteScript(\"{then}\", OBJECT_SELF);\n"));
    }
    let shown = if then.is_empty() { "-" } else { &then };
    format!(
        "{MADE_PAY}\n\
         // Change the check in the conversation window; editing this file by hand ends it.\n\
         // It spends what the check costs, then runs the script the line ran before.\n\
         {PAY_LINE}{} {} {shown}\n\
         \n\
         void main()\n\
         {{\n\
         {body}}}\n",
        check.skill, check.cost
    )
}

/// A name for a new condition of a check (`kjc_N`) or a new script that
/// spends its cost (`kjp_N`), for the smallest N from 1 that `taken`
/// refuses. `payment` picks which.
pub fn fresh_name(payment: bool, taken: impl Fn(&str) -> bool) -> String {
    let prefix = if payment { "kjp_" } else { "kjc_" };
    (1..)
        .map(|n| format!("{prefix}{n}"))
        .find(|name| !taken(name))
        .expect("some number is free")
}
