//! Words of ours on KOTOR's screens (in the game's code page), where a
//! layout's own text says something else. One table, so it can be
//! translated.

pub const SOLO: &str = "Solo";
pub const HOST: &str = "Host";
pub const JOIN: &str = "Join";
pub const OPTIONS: &str = "Options";
pub const EXIT: &str = "Exit";
pub const JOIN_ADDRESS: &str =
    "Join a game: the host's address (host or host:port), or a Steam friend's name";
pub const JOIN_NAME: &str = "Your name in the game";
pub const DEFAULT_NAME: &str = "Player";
pub const OK: &str = "OK";
pub const QUIT_TO_MENU: &str = "Quit to the main menu";
pub const BREAK: &str = "Take a break";
pub const RESUME: &str = "Resume";
pub const INVERT_CAMERA: &str = "Invert camera";
pub const ANTI_ALIASING_OFF: &str = "Anti-Aliasing: Off";
pub const ANTI_ALIASING_2X: &str = "Anti-Aliasing: 2x";
pub const ANTI_ALIASING_4X: &str = "Anti-Aliasing: 4x";
pub const TAKE_ALL: &str = "Take all (A)";
pub const CONTAINER: &str = "Container";
pub const CLOSE: &str = "Close";
pub const BACK: &str = "Back";
pub const START: &str = "Start";
pub const SEATS: &str = "Seats:";
pub const NEW_GAME: &str = "New game";
pub const CAMPAIGN: &str = "Which story does the new game begin?";
pub const KOTOR: &str = "KOTOR";
pub const SITH_LORDS: &str = "The Sith Lords";
pub const SOLO_TITLE: &str = "Play alone";
pub const HOST_TITLE: &str = "Host a game";
pub const PORT_HINT: &str = "Port: type to change";
pub const CANCEL: &str = "Cancel";
pub const FALLEN: &str = "You have fallen.";
pub const REVIVING: &str = "Reviving";
pub const PARTY_FALLEN: &str = "Your party has fallen.";
pub const WAIT_FOR_HOST: &str = "Your party has fallen. The host loads the last save.";
pub const BUY: &str = "Buy";
pub const SELL: &str = "Sell";
pub const SHOW_BUY_LIST: &str = "Show Buy List";
pub const SAVE_TITLE: &str = "Save the game";
pub const LOAD_TITLE: &str = "Load a game";
pub const NEW_SAVE: &str = "New save";
pub const SAVE: &str = "Save";
pub const LOAD: &str = "Load";
pub const DELETE: &str = "Delete";
pub const NO_SAVES: &str = "No saved games";
/// Why the host's answer to a join request waits: a ready check is up, the
/// host is down, the party fights, or the host's own conversation is locked.
pub const ANSWER_AFTER_READY: &str = "Answer after the ready check";
pub const ANSWER_WHEN_UP: &str = "Answer once you stand";
pub const ANSWER_AFTER_COMBAT: &str = "Answer after the fight";
pub const ANSWER_AFTER_TALK: &str = "Answer once your conversation ends";
/// A reply that says nothing (xoreos's reading).
pub const CONTINUE: &str = "[CONTINUE]";

/// `text` as the game's bytes.
pub fn bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|character| u8::try_from(u32::from(character)).unwrap_or(b'?'))
        .collect()
}

/// A conversation's reply as offered: `nth` (from 0) numbered from 1,
/// "1. …"; one that says nothing "[CONTINUE]".
pub fn reply(nth: usize, words: &[u8]) -> Vec<u8> {
    let words = if words.is_empty() {
        CONTINUE.as_bytes()
    } else {
        words
    };
    [format!("{}. ", nth + 1).as_bytes(), words].concat()
}

/// The row that leaves a conversation (R6), its key named: "Leave (L)".
pub fn leave(key: &[u8]) -> Vec<u8> {
    [b"Leave (", key, b")"].concat()
}

/// The title over another player's pazaak table (R9a): "Watching Carth".
pub fn watching(name: &[u8]) -> Vec<u8> {
    [b"Watching ", name].concat()
}

/// What a pazaak player is doing before the table, as its watcher is told
/// (R9a): "Carth is placing a wager.", shorter "Placing a wager.".
pub fn wagering(name: &[u8]) -> Vec<u8> {
    [name, b" is placing a wager."].concat()
}
pub const WAGERING: &str = "Placing a wager.";

/// "Carth is choosing a side deck.", shorter "Choosing a side deck.".
pub fn choosing(name: &[u8]) -> Vec<u8> {
    [name, b" is choosing a side deck."].concat()
}
pub const CHOOSING: &str = "Choosing a side deck.";

/// Why a watched match is paused (R39): "Carth is away." (its player's
/// seat dropped), shorter "Away.".
pub fn away(name: &[u8]) -> Vec<u8> {
    [name, b" is away."].concat()
}
pub const AWAY: &str = "Away.";

/// "Carth is hurt." (its player hurt, still in a fight), shorter "Hurt.".
pub fn hurt(name: &[u8]) -> Vec<u8> {
    [name, b" is hurt."].concat()
}
pub const HURT: &str = "Hurt.";

/// A pazaak result as a watcher is told it (R9a): "Carth won the set.",
/// "Carth lost the match.", "The set is tied." (`won` none).
pub fn pazaak_result(name: &[u8], won: Option<bool>, whole: bool) -> Vec<u8> {
    let what: &[u8] = if whole { b"match" } else { b"set" };
    match won {
        Some(won) => {
            let did: &[u8] = if won { b" won the " } else { b" lost the " };
            [name, did, what, b"."].concat()
        }
        None => [b"The ", what, b" is tied."].concat(),
    }
}

/// A guest asking the host in (R5): "Carth asks you to join: Davik".
pub fn asks(name: &[u8], npc: &[u8]) -> Vec<u8> {
    [name, b" asks you to join: ", npc].concat()
}

/// The keys that answer a request (R5): "Y: accept   N: end".
pub fn answers(accept: &[u8], end: &[u8]) -> Vec<u8> {
    [accept, b": accept   ", end, b": end"].concat()
}
