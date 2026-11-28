//! A line of text being typed, in the game's code page, with a caret.

use fmt_odyssey::CodePage;

/// Whether the fonts draw `byte` as a letter: ASCII from the space to `~`,
/// and every byte from 0xA0 up. The bytes 0x80 to 0x9F hold punctuation in
/// both code pages and no Russian letter, so a name box leaves them out.
fn printable(byte: u8) -> bool {
    (32..127).contains(&byte) || byte >= 0xA0
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditBox {
    pub text: Vec<u8>,
    pub max: usize,
    /// Still the default it was made with: the first key replaces it.
    fresh: bool,
    /// Where the next letter goes, as a count of the bytes before it. A box
    /// whose caret was never moved keeps it at the end of the text.
    caret: usize,
}

impl EditBox {
    pub fn new(max: usize) -> EditBox {
        EditBox {
            text: Vec::new(),
            max,
            fresh: false,
            caret: 0,
        }
    }

    /// A box that starts with `text`, a `String` with one character for
    /// each of the game's bytes (`fmt_odyssey::code_page`).
    pub fn with(text: &str, max: usize) -> EditBox {
        let mut edit = EditBox::new(max);
        edit.set(text);
        edit.fresh = true;
        edit
    }

    /// The text replaced by `text` (as `with` reads it), the caret at its
    /// end.
    pub fn set(&mut self, text: &str) {
        self.text = fmt_odyssey::code_page_bytes(text)
            .into_iter()
            .filter(|&byte| printable(byte))
            .take(self.max)
            .collect();
        self.fresh = false;
        self.caret = self.text.len();
    }

    /// The box emptied, the caret at the start.
    pub fn clear(&mut self) {
        self.text.clear();
        self.fresh = false;
        self.caret = 0;
    }

    /// The text of a fresh box cleared before the first key changes it.
    fn touch(&mut self) {
        if std::mem::take(&mut self.fresh) {
            self.clear();
        }
        self.caret = self.caret.min(self.text.len());
    }

    /// Typed letters as the bytes `page` writes them, put in at the caret,
    /// up to `max`. A letter the code page has no byte for, and a control
    /// character, is left out.
    pub fn type_text(&mut self, text: &str, page: CodePage) {
        self.touch();
        for byte in text.chars().filter_map(|character| page.byte(character)) {
            if printable(byte) && self.text.len() < self.max {
                self.text.insert(self.caret, byte);
                self.caret += 1;
            }
        }
    }

    /// The letter before the caret removed.
    pub fn backspace(&mut self) {
        self.touch();
        if self.caret > 0 {
            self.caret -= 1;
            self.text.remove(self.caret);
        }
    }

    /// The letter after the caret removed.
    pub fn delete(&mut self) {
        self.touch();
        if self.caret < self.text.len() {
            self.text.remove(self.caret);
        }
    }

    /// The caret moved by `by` letters, within the text.
    pub fn step(&mut self, by: isize) {
        self.touch();
        self.caret = self.caret.saturating_add_signed(by).min(self.text.len());
    }

    /// The caret moved to the start of the text.
    pub fn home(&mut self) {
        self.touch();
        self.caret = 0;
    }

    /// The caret moved to the end of the text.
    pub fn end(&mut self) {
        self.touch();
        self.caret = self.text.len();
    }

    /// Where the caret is: how many bytes of the text are before it.
    pub fn caret(&self) -> usize {
        self.caret.min(self.text.len())
    }

    /// Still the default it was made with, nothing typed.
    pub fn untouched(&self) -> bool {
        self.fresh
    }

    pub fn as_str(&self) -> String {
        fmt_odyssey::code_page(&self.text)
    }
}
