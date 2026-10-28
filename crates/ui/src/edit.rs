//! A line of text being typed, in the game's code page.

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

    pub fn with(text: &str, max: usize) -> EditBox {
        let mut edit = EditBox::new(max);
        edit.type_text(text);
        edit.fresh = true;
        edit
    }

    /// Typed characters the font has (printable Latin-1), up to `max`.
    pub fn type_text(&mut self, text: &str) {
        if std::mem::take(&mut self.fresh) {
            self.text.clear();
        }
        for character in text.chars() {
            let code = u32::from(character);
            let printable = (32..127).contains(&code) || (160..256).contains(&code);
            if printable && self.text.len() < self.max {
                self.text.push(code as u8);
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
        self.text.iter().map(|&byte| char::from(byte)).collect()
    }
}
