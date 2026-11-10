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
pub const QUIT_TO_DESKTOP: &str = "Quit to the desktop";
pub const BREAK: &str = "Take a break";
pub const RESUME: &str = "Resume";
pub const INVERT_CAMERA: &str = "Invert camera";
/// The main menu options' way into the combat sandbox.
pub const DEBUG: &str = "Debug";
pub const CONTROLLER: &str = "Controller";
pub const CONTROLLER_OPTIONS: &str = "Controller Options";
pub const PRESS_KEY: &str = "Press a key or a mouse button. Escape cancels.";
/// `{shift}` and `{menu}` the pad's own names for its left bumper and
/// its menu button.
pub const PRESS_BUTTON: &str =
    "Press a button; hold {shift} first for a {shift} chord. {menu} cancels.";
pub const NOW_ON: &str = "Now:";
pub const UNBOUND: &str = "-";
pub const KEYS_TITLE: &str = "Keyboard and Mouse";
pub const PAD_TITLE: &str = "Controller";
pub const LOOK_SPEED: &str = "Look Speed";
pub const INVERT_LOOK: &str = "Invert look";
pub const VIBRATION: &str = "Vibration";
pub const DEAD_ZONE_SMALL: &str = "Stick Dead Zone: Small";
pub const DEAD_ZONE_MEDIUM: &str = "Stick Dead Zone: Medium";
pub const DEAD_ZONE_LARGE: &str = "Stick Dead Zone: Large";
pub const CAMERA: &str = "Camera";
pub const CAMERA_OPTIONS: &str = "Camera Options";
pub const CAMERA_DISTANCE: &str = "Distance";
pub const CAMERA_HEIGHT: &str = "Height";
pub const CAMERA_SHOULDER: &str = "Shoulder: Left - Right";
pub const CAMERA_FOV: &str = "Field of View";
pub const CAMERA_JA: &str = "Camera: Jedi Academy";
pub const CAMERA_SHOULDER_PRESET: &str = "Camera: Over the Shoulder";
pub const CAMERA_KOTOR: &str = "Camera: KOTOR";
pub const CAMERA_CLOSE: &str = "Camera: Close";
pub const CAMERA_CUSTOM: &str = "Camera: Custom";
pub const CAMERA_LAG_JA: &str = "Camera Follow: Loose";
pub const CAMERA_LAG_TIGHT: &str = "Camera Follow: Tight";
pub const CAMERA_LAG_OFF: &str = "Camera Follow: Locked";
pub const CAMERA_HELP: &str = "How far back the third-person camera stands, how high, over which shoulder, how wide it sees and how closely it follows. Hold Left Alt to look around your character; = and - (or the wheel with Alt held) bring the camera closer or farther, into first person at the nearest.";
pub const CAMERA_DISTANCE_HELP: &str = "How far behind your character the camera stands. At the nearest, one more step closer (=) looks through your character's eyes.";
pub const CAMERA_HEIGHT_HELP: &str = "How high over your character's eyes the camera aims.";
pub const CAMERA_SHOULDER_HELP: &str = "Moves the camera to the side: in the middle it stands straight behind your character, as in Jedi Academy; to the right it looks over the right shoulder. The middle mouse button swaps the side.";
pub const CAMERA_FOV_HELP: &str =
    "How wide the camera sees, in either camera. Jedi Academy's is 80.";
pub const CAMERA_FOLLOW_HELP: &str = "How closely the camera follows your turns and moves: Loose trails a little behind as Jedi Academy's does, Locked stays fixed to your character.";
pub const BODY_FADE_HELP: &str = "Fades your character when a wall pushes the camera close, so the camera never shows the inside of a head.";
pub const CAMERA_PRESET_HELP: &str = "Sets the four sliders at once: Jedi Academy's camera, over the shoulder, KOTOR's, or close. Moving a slider makes it Custom.";
pub const GUN_CAMERA_SHOULDER: &str = "Guns: Over the Shoulder";
pub const GUN_CAMERA_CLOSE: &str = "Guns: Close";
pub const GUN_CAMERA_KOTOR: &str = "Guns: KOTOR";
pub const GUN_CAMERA_SAME: &str = "Guns: Default Camera";
pub const GUN_CAMERA_JA: &str = "Guns: Jedi Academy";
pub const GUN_CAMERA_CUSTOM: &str = "Guns: Custom";
pub const GUN_CAMERA: &str = "Guns Camera";
pub const GUN_CAMERA_OPTIONS: &str = "Guns Camera Options";
pub const GUN_CAMERA_HELP: &str = "The camera while a gun is readied: it eases there as you draw one for a fight and back as you put it away. Its own sliders, or a preset: over the shoulder, close, KOTOR's, Jedi Academy's; Default Camera keeps the camera page's camera, the one for melee and empty hands.";
pub const BODY_FADE_ON: &str = "Body Fade Up Close: On";
pub const BODY_FADE_OFF: &str = "Body Fade Up Close: Off";
pub const ANTI_ALIASING_OFF: &str = "Anti-Aliasing: Off";
pub const ANTI_ALIASING_2X: &str = "Anti-Aliasing: 2x";
pub const ANTI_ALIASING_4X: &str = "Anti-Aliasing: 4x";
/// The options pages' descriptions where a setting is ours, not KOTOR 2's
/// (whose own come from its talk table).
pub const INVERT_CAMERA_HELP: &str = "ON: Moving the mouse up turns the view downwards.\n\nOFF: Moving the mouse up turns the view upwards.";
pub const AUTOSAVE_HELP: &str = "ON: The game saves each time you enter a new area.\nYou should still save often.\n\nOFF: Only your own saves are kept.";
pub const CONTROLLER_HELP: &str =
    "Look speed, stick dead zone, inverted look and vibration for a controller.";
pub const LOOK_SPEED_HELP: &str = "Change how fast the right stick turns the view.";
pub const DEAD_ZONE_HELP: &str = "How far a stick is pushed before it counts.\n\nA worn stick that drifts on its own needs a larger dead zone.";
pub const ANTI_ALIASING_HELP: &str = "4x: for the best visual experience\n\n2x: a blend of game performance and visual quality\n\nOff: suggested for older video cards";
pub const ADVANCED_GRAPHICS_HELP: &str = "Anti-aliasing, V-Sync and soft shadows.";
/// kjk's Interface page, which KOTOR 2's options do not have: its title,
/// its stepped buttons' choices (each with the option's name), its check
/// box, and its descriptions.
pub const INTERFACE: &str = "Interface";
pub const INTERFACE_HELP: &str = "Sets how wide the menus are drawn, whether their pictures keep their shape, and how large their text is.";
pub const MENU_WIDTH_FILL: &str = "Menu width: Fill the screen";
pub const MENU_WIDTH_16X9: &str = "Menu width: Up to 16:9";
pub const MENU_WIDTH_4X3: &str = "Menu width: 4:3";
pub const IMAGE_ASPECT: &str = "Maintain image aspect ratio";
pub const TEXT_SIZE_AUTO: &str = "Text size: Auto";
pub const TEXT_SIZE_SMALL: &str = "Text size: Small";
pub const TEXT_SIZE_LARGE: &str = "Text size: Large";
pub const MENU_WIDTH_HELP: &str = "How wide the menus are drawn. Fill the screen stretches them to the window's edges, as the original game does. Up to 16:9 stretches them no wider than a 16:9 screen and adds black bars beyond it. 4:3 keeps their original shape.";
pub const IMAGE_ASPECT_HELP: &str = "Icons, portraits, logos and character views keep their original shape when the menus are stretched to fill the screen. Turn this off to stretch them as the original game does.";
pub const TEXT_SIZE_HELP: &str = "Auto picks the larger text in windows 1280 pixels wide or more, as the original game does. Small and Large always use one size.";
pub const TAKE_ALL: &str = "Take all (A)";
/// The same while a pad plays: its hint bar names its button.
pub const TAKE_ALL_PAD: &str = "Take all";
pub const CONTAINER: &str = "Container";
/// The words beside the journal key in a picked-up datapad's box.
pub const READ_DATAPAD: &str = "Read datapad";
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
pub const SELECT: &str = "Select";
pub const TABS: &str = "Tabs";
pub const SCREENS: &str = "Screens";
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
pub const RESTORE: &str = "Restore";
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

/// An object's name as the game's screens show it (the target label, a
/// speaker, a container, the message log): its bytes with what it holds in
/// braces out, the designers' notes ("Lift Controls{to 002EBO}"). Scripts'
/// `GetName` keeps them; only what is shown leaves them out.
pub fn name(text: &str) -> Vec<u8> {
    fmt_odyssey::tlk::unbraced(&bytes(text))
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
