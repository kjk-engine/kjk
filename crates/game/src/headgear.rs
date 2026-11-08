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
/// body and, as drawn, on its head (items' templates).
pub type SeatLook = (u16, Option<String>, Option<String>);

/// Seat `seat`'s look in `snapshot` (`sim::persona::dressed`), its headgear
/// none while `hidden` says its choice is set.
pub fn seat_look(snapshot: &sim::Snapshot, seat: sim::Seat, hidden: Hidden) -> Option<SeatLook> {
    let (appearance, armour) = sim::persona::dressed(snapshot, seat)?;
    let headgear = snapshot
        .entities
        .iter()
        .find(|entity| entity.kind == Kind::Avatar(seat))
        .and_then(|entity| drawn(snapshot, entity, hidden(&seat_name(seat))));
    Some((appearance, armour, headgear))
}

/// Whether the settings say `who`'s headgear is hidden.
pub fn hidden_in(settings: &crate::settings::Settings, who: &str) -> bool {
    settings.on(&setting(who))
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
