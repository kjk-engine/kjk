//! A line of text being typed, in the game's code page.

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
}

impl EditBox {
    pub fn new(max: usize) -> EditBox {
        EditBox {
            text: Vec::new(),
            max,
            fresh: false,
        }
    }

    /// A box that starts with `text`, a `String` with one character for
    /// each of the game's bytes (`fmt_odyssey::code_page`).
    pub fn with(text: &str, max: usize) -> EditBox {
        let mut edit = EditBox::new(max);
        edit.text = fmt_odyssey::code_page_bytes(text)
            .into_iter()
            .filter(|&byte| printable(byte))
            .take(max)
            .collect();
        edit.fresh = true;
        edit
    }

    /// Typed letters as the bytes `page` writes them, up to `max`. A letter
    /// the code page has no byte for, and a control character, is left out.
    pub fn type_text(&mut self, text: &str, page: CodePage) {
        if std::mem::take(&mut self.fresh) {
            self.text.clear();
        }
        for byte in text.chars().filter_map(|character| page.byte(character)) {
            if printable(byte) && self.text.len() < self.max {
                self.text.push(byte);
            }
        }
    }

    pub fn backspace(&mut self) {
        if std::mem::take(&mut self.fresh) {
            self.text.clear();
        }
        self.text.pop();
    }

    /// Still the default it was made with, nothing typed.
    pub fn untouched(&self) -> bool {
        self.fresh
    }

    pub fn as_str(&self) -> String {
        fmt_odyssey::code_page(&self.text)
    }
}
