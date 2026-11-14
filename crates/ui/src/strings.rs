//! Words of ours on KOTOR's screens (in the game's code page), where a
//! layout's own text says something else. One table, so it can be
//! translated.

/// What a build with DLSS adds to the main menu's build line (NVIDIA's
/// DLSS licence: the SDK credited where the game names itself).
pub const DLSS_CREDIT: &str = "with NVIDIA DLSS";
/// The credits' last block in a build with DLSS.
pub const DLSS_CREDITS: &str =
    "NVIDIA DLSS\n\nNVIDIA, RTX and DLSS are trademarks of NVIDIA Corporation.";

pub const SOLO: &str = "Solo";
pub const HOST: &str = "Host";
pub const JOIN: &str = "Join";
pub const OPTIONS: &str = "Options";
pub const EXIT: &str = "Exit";
pub const JOIN_ADDRESS: &str = "Enter the host's IP address or a Steam friend's name.";
pub const JOIN_NAME: &str = "Your name";
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
pub const PRESS_KEY: &str = "Press a key or mouse button. Press Escape to cancel.";
/// `{shift}` and `{menu}` the pad's own names for its left bumper and
/// its menu button.
pub const PRESS_BUTTON: &str =
    "Press a button. Hold {shift} first for a {shift} chord. Press {menu} to cancel.";
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
pub const CAMERA_HELP: &str = "Camera distance, height, shoulder side and FOV. Hold {orbit} to look around your character. Press {zoom_in} to zoom in and {zoom_out} to zoom out.";
pub const CAMERA_DISTANCE_HELP: &str = "The camera's distance to your character.";
pub const CAMERA_HEIGHT_HELP: &str = "The camera's height.";
pub const CAMERA_SHOULDER_HELP: &str =
    "Moves the camera to the left or right of your character. Press {swap_shoulder} to swap sides.";
pub const CAMERA_FOV_HELP: &str = "The camera's viewing angle.";
pub const CAMERA_FOLLOW_HELP: &str =
    "Camera smoothing. Loose lags slightly behind your character. Locked has no lag.";
pub const BODY_FADE_HELP: &str = "Fades your character when a wall pushes the camera up close.";
pub const CAMERA_PRESET_HELP: &str =
    "Camera presets. Changing a slider switches the preset to Custom.";
pub const GUN_CAMERA_SHOULDER: &str = "Guns: Over the Shoulder";
pub const GUN_CAMERA_CLOSE: &str = "Guns: Close";
pub const GUN_CAMERA_KOTOR: &str = "Guns: KOTOR";
pub const GUN_CAMERA_SAME: &str = "Guns: Default Camera";
pub const GUN_CAMERA_JA: &str = "Guns: Jedi Academy";
pub const GUN_CAMERA_CUSTOM: &str = "Guns: Custom";
pub const GUN_CAMERA: &str = "Guns Camera";
pub const GUN_CAMERA_OPTIONS: &str = "Guns Camera Options";
pub const GUN_CAMERA_HELP: &str = "The camera while a ranged weapon is ready.";
pub const BODY_FADE_ON: &str = "Body Fade Up Close: On";
pub const BODY_FADE_OFF: &str = "Body Fade Up Close: Off";
pub const ANTI_ALIASING_OFF: &str = "Anti-Aliasing: Off";
pub const ANTI_ALIASING_2X: &str = "Anti-Aliasing: 2x";
pub const ANTI_ALIASING_4X: &str = "Anti-Aliasing: 4x";
pub const ANTI_ALIASING_TAA: &str = "Anti-Aliasing: TAA";
pub const ANTI_ALIASING_DLAA: &str = "Anti-Aliasing: DLAA";
pub const ANTI_ALIASING_DLSS_QUALITY: &str = "DLSS: Quality";
pub const ANTI_ALIASING_DLSS_BALANCED: &str = "DLSS: Balanced";
pub const ANTI_ALIASING_DLSS_PERFORMANCE: &str = "DLSS: Performance";
pub const ANTI_ALIASING_DLSS_ULTRA_PERFORMANCE: &str = "DLSS: Ultra Performance";
/// kjk's sabers' page, which KOTOR 2's options do not have. These are the
/// gameplay page's button that opens it, its title, the names of its rows
/// and the words of their values. A stepped row shows its name and its
/// value ("Saber rules: Jedi Academy", "Hilt: Luke (ROTJ)").
pub const SABERS: &str = "Sabers";
pub const SABER_OPTIONS: &str = "Saber Options";
pub const SABER_RULES: &str = "Combat rules";
pub const SABER_RULES_JA: &str = "Jedi Academy";
pub const SABER_RULES_ADVANCED: &str = "Advanced";
pub const STANCE_SETS: &str = "Movie Duels stances";
pub const DUEL_BARS: &str = "Overhead Stamina bars";
pub const DUEL_WARNING: &str = "Unblockable warning";
pub const DUEL_SLOW_MOTION: &str = "Perfect block slow motion";
pub const HILT: &str = "Hilt";
pub const STAFF: &str = "Double-bladed hilt";
pub const HILT_KOTOR: &str = "KOTOR";
pub const HILT_JA: &str = "Jedi Academy";
pub const HILT_LUKE: &str = "Luke (ROTJ)";
pub const HILT_OBI_WAN: &str = "Obi-Wan (ANH)";
pub const HILT_VADER: &str = "Vader";
pub const HILT_AHSOKA: &str = "Ahsoka";
pub const HILT_DOOKU: &str = "Dooku";
pub const HILT_DARKSABER: &str = "Darksaber";
pub const STAFF_BASTILA: &str = "Bastila";
pub const STAFF_MAUL: &str = "Maul";
pub const STAFF_INQUISITOR: &str = "Inquisitor";
pub const STAFF_CAL: &str = "Cal Kestis";
/// The character page's Stance (`pages::Character`), which KOTOR 2's page
/// does not have: its name, what it shows while no stance is picked, and
/// its tips.
pub const STANCE: &str = "Stance";
pub const STANCE_OWN: &str = "Default";
pub const STANCE_HELP: &str =
    "This character's lightsaber stance. It sets how the character stands, walks and blocks.";
pub const STANCE_NEEDS: &str =
    "Stances come from Movie Duels. Turn on the Movie Duels stances setting to use them.";
/// The options pages' descriptions where a setting is ours, not KOTOR 2's
/// (whose own come from its talk table).
pub const INVERT_CAMERA_HELP: &str = "Inverts vertical mouse look.";
pub const AUTOSAVE_HELP: &str = "Saves the game automatically when you enter a new area.";
pub const CONTROLLER_HELP: &str = "Controller settings.";
pub const LOOK_SPEED_HELP: &str = "How fast the right stick looks around.";
pub const DEAD_ZONE_HELP: &str = "How far a stick must move before it registers.\n\nA worn stick that drifts on its own needs a larger dead zone.";
pub const ANTI_ALIASING_HELP: &str = "Smooths jagged edges.";
pub const ADVANCED_GRAPHICS_HELP: &str = "Anti-aliasing, Lighting and V-Sync.";
pub const V_SYNC: &str = "V-Sync";
pub const SHADOWS_OFF: &str = "Shadows: Off";
pub const SHADOWS_LOW: &str = "Shadows: Low";
pub const SHADOWS_HIGH: &str = "Shadows: High";
pub const SHADOWS_HELP: &str =
    "Shadows cast by characters and objects.\n\nLow draws fewer of them in a fight.";
/// The sabers' page's descriptions. `{block}` and `{attack}` are the keys
/// of the block and attack actions.
pub const SABERS_HELP: &str = "Combat rules, Stamina bars and lightsaber hilts.";
pub const SABER_RULES_HELP: &str = "Jedi Academy: Lightsabers block attacks and blaster bolts automatically.\n\nAdvanced: Hold {block} to block with a melee weapon, such as a lightsaber, a sword, a vibroblade or a staff. A blaster, a stun baton and bare fists cannot block. While blocking, press {attack} just before an attack lands for a perfect block. Every character has Stamina. Blocking, attacking and special moves use it.\n\nA game keeps the combat rules it started with. In co-op, the host's combat rules apply.";
pub const DUEL_BARS_HELP: &str = "Shows a health bar and a Stamina bar above each enemy that is fighting you or your party, or that you have targeted. Companions and characters who are not fighting never show bars. You always see your own Stamina bar next to your portrait, whether or not this option is on. Advanced rules only.";
pub const DUEL_WARNING_HELP: &str =
    "Highlights attacks that only a perfect block can stop in red. Advanced rules only.";
pub const DUEL_SLOW_MOTION_HELP: &str =
    "Slows time briefly after your perfect block. Advanced rules only, and not in co-op.";
/// What the sabers' page says of the saber rules while a game runs.
pub const SABER_RULES_FIXED: &str =
    "The combat rules are chosen before a game starts. Change them from the main menu.";
pub const STANCE_SETS_FIXED: &str =
    "The stances are chosen before a game starts. Change them from the main menu.";
/// What the sabers' page says of Movie Duels' choices without it.
pub const NEEDS_MOVIE_DUELS: &str = "Needs Movie Duels installed beside Jedi Academy.";
pub const MOVIE_DUELS_HILTS: &str =
    "Movie Duels' hilts need Movie Duels installed beside Jedi Academy.";
pub const STANCE_SETS_HELP: &str = "Characters use Movie Duels stances and guards. In co-op, every player needs Movie Duels when this is on.";
pub const HILT_HELP: &str = "The hilt model for single-bladed lightsabers.";
pub const STAFF_HELP: &str = "The hilt model for double-bladed lightsabers.";
pub const LIGHTING_CLASSIC: &str = "Lighting: Classic";
pub const LIGHTING_ENHANCED: &str = "Lighting: Enhanced";
pub const LIGHTING_HELP: &str = "Classic uses the original game's lighting.\n\nEnhanced adds glow to lightsabers, blaster bolts and muzzle flashes.";
/// kjk's Interface page, which KOTOR 2's options do not have. These are
/// its title, the names of its stepped rows and the words of their values,
/// the names of its check boxes, and its descriptions.
pub const INTERFACE: &str = "Interface";
pub const INTERFACE_HELP: &str = "Sets how wide the menus are drawn, whether their pictures keep their shape and how large their text is. It also turns on kjk's own changes to the original game's notifications and target block, and sets a frame rate limit.";
pub const MENU_WIDTH: &str = "Menu width";
pub const MENU_WIDTH_FILL: &str = "Fill the screen";
pub const MENU_WIDTH_16X9: &str = "Up to 16:9";
pub const MENU_WIDTH_4X3: &str = "4:3";
pub const IMAGE_ASPECT: &str = "Maintain image aspect ratio";
pub const TEXT_SIZE: &str = "Text size";
pub const TEXT_SIZE_AUTO: &str = "Auto";
pub const TEXT_SIZE_SMALL: &str = "Small";
pub const TEXT_SIZE_LARGE: &str = "Large";
pub const TEXT_SCALE: &str = "Text scale";
pub const TEXT_SCALE_HELP: &str = "How large the text of the menus, the HUD, the notifications and the conversations is drawn. 100% is the original game's size. The scale goes from 80% to 150% in steps of 10%.";
pub const MENU_WIDTH_HELP: &str = "How wide the menus are drawn. Fill the screen stretches them to the window's edges, as the original game does. Up to 16:9 stretches them no wider than a 16:9 screen and adds black bars beyond it. 4:3 keeps their original shape.";
pub const IMAGE_ASPECT_HELP: &str = "Icons, portraits, logos and character views keep their original shape when the menus are stretched to fill the screen. Turn this off to stretch them as the original game does.";
pub const TEXT_SIZE_HELP: &str = "Auto picks the larger text in windows 1280 pixels wide or more, as the original game does. Small and Large always use one size.";
pub const CORNER_NOTICES: &str = "Notifications in the corner";
pub const CORNER_NOTICES_HELP: &str = "ON: Notifications such as \"Game saved\", experience gained and a journal entry added slide in at the top right of the screen, under the menu buttons. A datapad you pick up gets a notification of its own there, with the key that reads it.\n\nOFF: Notifications show in the middle of the screen, as in the original game. The status summary of experience, credits and journal entries pauses the game until you press its OK button.";
pub const TARGET_TOP: &str = "Target block at the top centre";
pub const TARGET_TOP_HELP: &str = "ON: The target's name, health bar and actions stay at the top centre of the screen, and the chosen action is named under them.\n\nOFF: The target's name, health bar and actions float above the target, as in the original game.";
pub const FRAME_LIMIT: &str = "Frame rate limit";
pub const FRAME_LIMIT_OFF: &str = "Off";
pub const FRAME_LIMIT_HELP: &str = "Limits the frames per second the game draws. A lower limit keeps the graphics card cooler and quieter. With V-Sync off, Off still stops at 240.";
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
pub const CAMPAIGN: &str = "Select game";
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
pub const WAIT_FOR_HOST: &str = "Your party has fallen. Waiting for the host.";
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

/// An option's name and its chosen value as a list's row shows them:
/// "Menu width: Up to 16:9".
pub fn option(name: &str, value: &str) -> Vec<u8> {
    bytes(&format!("{name}: {value}"))
}

/// A percentage as an option's row shows it: "100%".
pub fn percent(value: &str) -> String {
    format!("{value}%")
}

/// A frame rate as an option's row shows it: "60 FPS".
pub fn fps(value: &str) -> String {
    format!("{value} FPS")
}

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
