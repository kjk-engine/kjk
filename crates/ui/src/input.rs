//! What reaches the screens in a frame, whatever the device: keys,
//! gamepad buttons and the pointer, already turned into the same few
//! moves.

/// One move of the focus or the open screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    /// Enter, gamepad A.
    Activate,
    /// Escape, gamepad B or Start.
    Back,
    /// A double click on what has the focus (a list row's `Screen::primary`:
    /// buy, use, equip, load; a party slot's add or remove), gamepad X.
    Primary,
    /// The menu bar's next and previous tab (gamepad RB and LB); a screen
    /// with no menu bar, its own tabs.
    NextTab,
    PrevTab,
    /// The page's own tabs (gamepad RT and LT): an item filter, a log.
    NextPage,
    PrevPage,
    /// The screen's OK, Done or Accept (gamepad Menu); one with none closes.
    Accept,
    /// The screen's second button (gamepad Y): Default, Take all.
    Second,
    ScrollUp,
    ScrollDown,
    Backspace,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiInput {
    pub keys: Vec<Key>,
    /// Characters typed this frame.
    pub text: String,
    /// The keys pressed this frame by name (`KeyW`), for binding them.
    pub raw: Vec<String>,
    /// Where the pointer is, in window pixels.
    pub pointer: Option<[f32; 2]>,
    /// The pointer moved this frame.
    pub moved: bool,
    /// The main button was pressed this frame.
    pub click: bool,
    /// The main button is held down (a drag goes on while it is).
    pub held: bool,
    /// Wheel notches, up positive.
    pub wheel: f32,
    /// The window's size in pixels.
    pub window: (f32, f32),
    /// Seconds since the last frame.
    pub dt: f32,
}
