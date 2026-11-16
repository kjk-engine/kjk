//! What a character wears on its head, as drawn: the item in its head slot
//! (a mask, a visor), hung on its head model's `gogglehook`, unless its
//! player hid it with the equip page's eye. KOTOR always draws it; hiding
//! it is a remaster's choice, the player's own, kept in their settings by
//! the character (`who`), so only their game hides it and it outlasts the
//! save. Render only: the sim still has it worn (its stats count).

use sim::{EntityState, Instances, Kind};

/// Whether the character of a `who` hides its headgear.
pub type Hidden<'a> = &'a dyn Fn(&str) -> bool;

/// The setting that hides `who`'s headgear (`true` hides it).
pub fn setting(who: &str) -> String {
    format!("headgear.hidden.{who}")
}

/// The name the choice is kept by for a character: a seat's by its seat
/// (whoever plays it), any other by its tag; none for one without a tag.
pub fn who(objects: Option<&Instances>, entity: &EntityState) -> Option<String> {
    match entity.kind {
        Kind::Avatar(seat) => Some(seat_name(seat)),
        _ => objects?
            .of_entity(entity)
            .map(|facts| facts.tag.clone())
            .filter(|tag| !tag.is_empty()),
    }
}

/// A seat's character's name in `who`.
pub fn seat_name(seat: sim::Seat) -> String {
    format!("seat{}", seat.0)
}

/// A seat's look: its persona's `appearance.2da` row, what it wears on its
/// body and, as drawn, on its head (items' templates), and the alignment it
/// is dressed for (`alignment_look`).
pub type SeatLook = (
    u16,
    Option<String>,
    Option<String>,
    asset_odyssey::creature::Aligned,
);

/// Seat `seat`'s look in `snapshot` (`sim::persona::dressed`), its headgear
/// none while `hidden` says its choice is set, dressed for its character's
/// alignment in KOTOR 2 (`tsl`) or KOTOR.
pub fn seat_look(
    snapshot: &sim::Snapshot,
    (seat, tsl): (sim::Seat, bool),
    hidden: Hidden,
) -> Option<SeatLook> {
    let (appearance, armour) = sim::persona::dressed(snapshot, seat)?;
    let own = snapshot
        .entities
        .iter()
        .find(|entity| entity.kind == Kind::Avatar(seat));
    let headgear = own.and_then(|entity| drawn(snapshot, entity, hidden(&seat_name(seat))));
    let good_evil = own
        .and_then(|entity| entity.sheet)
        .map_or(sim::combat::sheet::NEUTRAL, |sheet| sheet.good_evil);
    let aligned = crate::alignment_look::aligned(tsl, good_evil);
    Some((appearance, armour, headgear, aligned))
}

/// Whether the settings say `who`'s headgear is hidden.
pub fn hidden_in(settings: &crate::settings::Settings, who: &str) -> bool {
    settings.on(&setting(who))
}

/// The characters whose headgear a conversation hides (S8). A DLG that sets
/// `UnequipHItem` hides the equipped head item (a mask, a helmet) of each
/// line's speaker and listener, from that line until the conversation ends,
/// as KOTOR 2's executable does as it sends each line. Each character is
/// kept by its name in `who`, as the player's own choice is. Render only:
/// the sim still has the item worn.
#[derive(bevy::prelude::Resource, Default)]
pub struct TalkHides {
    /// Each conversation's (owner, DLG) and a character it hides.
    noted: Vec<(script_ncs::vm::ObjectId, String, String)>,
}

impl TalkHides {
    /// Notes the speaker and listener of the line each conversation says
    /// whose DLG hides headgear (`hides_headgear`, by DLG name), and
    /// forgets those of a conversation that has ended.
    pub fn note(
        &mut self,
        conversations: &[sim::script::dialogue::Conversation],
        hides_headgear: &dyn Fn(&str) -> bool,
        named: &dyn Fn(&sim::script::dialogue::Conversation) -> [Option<String>; 2],
    ) {
        self.noted.retain(|(owner, dialog, _)| {
            conversations
                .iter()
                .any(|talk| talk.owner == *owner && talk.dialog == *dialog)
        });
        for talk in conversations
            .iter()
            .filter(|talk| hides_headgear(&talk.dialog))
        {
            for who in named(talk).into_iter().flatten() {
                let noted = (talk.owner, talk.dialog.clone(), who);
                if !self.noted.contains(&noted) {
                    self.noted.push(noted);
                }
            }
        }
    }

    /// Whether a conversation hides the headgear of the character `who`
    /// names.
    pub fn hides(&self, who: &str) -> bool {
        self.noted.iter().any(|(_, _, noted)| noted == who)
    }
}

/// Each frame, before characters are dressed: `TalkHides` kept to the
/// presented snapshot's conversations.
pub fn note_talks(
    mut hides: bevy::prelude::ResMut<TalkHides>,
    presented: bevy::prelude::Res<crate::sim_host::PresentedSnapshot>,
    swap: bevy::prelude::Res<crate::sim_host::SceneSwap>,
) {
    let snapshot = presented.0.current();
    let level = &swap.level;
    let Some(scripts) = level.scripts() else {
        return;
    };
    let objects = Instances::in_snapshot(level, snapshot);
    let hides_headgear = |dialog: &str| {
        scripts
            .dialogue(dialog)
            .is_some_and(|dlg| dlg.unequip_headgear)
    };
    let named = |talk: &sim::script::dialogue::Conversation| {
        let pair = crate::cast::line_pair(snapshot, talk);
        [pair.speaker, pair.listener].map(|id| {
            snapshot
                .get(id)
                .and_then(|entity| who(Some(&objects), entity))
        })
    };
    hides.note(&snapshot.talk.conversations, &hides_headgear, &named);
}

/// The item (a template, lowercase) `entity` is drawn wearing on its head
/// in `snapshot`: its head slot's, none while `hidden` (the character's
/// choice is set).
pub fn drawn(snapshot: &sim::Snapshot, entity: &EntityState, hidden: bool) -> Option<String> {
    if hidden {
        return None;
    }
    snapshot
        .worn(entity.id.0, sim::inventory::slots::HEAD)
        .map(str::to_ascii_lowercase)
}

/// What a conversation hides (`TalkHides`), on conversations made here: a
/// DLG that sets `UnequipHItem` hides the headgear of its first line's
/// speaker and listener, keeps it hidden through a line said by others,
/// hides the next line's speaker too, and shows every one again once the
/// conversation ends. A conversation whose DLG does not set it hides none.
/// No installs (approved 2026-11-16).
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    use sim::script::dialogue::Conversation;
    let talk = |dialog: &str, owner: u32| Conversation {
        dialog: dialog.into(),
        owner: script_ncs::vm::ObjectId(owner),
        ..Conversation::default()
    };
    let flagged = |dialog: &str| dialog == "masked";
    fn line(names: [&'static str; 2]) -> impl Fn(&Conversation) -> [Option<String>; 2] {
        move |_| names.map(|name| Some(name.to_string()))
    }
    let shown = |hides: &TalkHides| {
        ["bastila", "zhar", "carth", "mission"]
            .into_iter()
            .filter(|who| hides.hides(who))
            .collect::<Vec<_>>()
    };
    let mut hides = TalkHides::default();
    let masked = [talk("masked", 7)];
    hides.note(&masked, &flagged, &line(["bastila", "zhar"]));
    let first = shown(&hides);
    hides.note(&masked, &flagged, &line(["carth", "zhar"]));
    let second = shown(&hides);
    hides.note(&[talk("plain", 8)], &flagged, &line(["mission", "carth"]));
    let ended = shown(&hides);
    let mut plain = TalkHides::default();
    plain.note(&[talk("plain", 8)], &flagged, &line(["mission", "carth"]));
    let unflagged = shown(&plain);
    vec![(
        "conversation hides headgear",
        check(
            first == ["bastila", "zhar"]
                && second == ["bastila", "zhar", "carth"]
                && ended.is_empty()
                && unflagged.is_empty(),
            format!(
                "hidden after the first line {first:?}, after the second {second:?}, after the end {ended:?}; by a DLG without the flag {unflagged:?}"
            ),
        ),
    )]
}
