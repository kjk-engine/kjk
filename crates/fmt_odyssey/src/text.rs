//! The games' text and the code pages it is written in.
//!
//! The games keep every string as single bytes, and their fonts draw one
//! glyph for each byte. A Western European release (English, French, German,
//! Italian, Spanish) writes Windows-1252. A Russian release writes
//! Windows-1251 and ships fonts whose upper glyphs are Cyrillic letters.
//!
//! The engine keeps a game string as the bytes themselves, or as a `String`
//! with one character for each byte (`code_page` and `code_page_bytes`). The
//! bytes then reach the fonts unchanged in any language. Only text a person
//! reads outside the game (a transcript, a log) or types in (a name) needs to
//! know which letters the bytes stand for (`CodePage`).

use crate::tlk::Tlk;

/// Game text as a `String` with one character for each byte.
pub fn code_page(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}

/// A `String` made by `code_page` as the game's bytes again. A character
/// above 255 cannot be one of the game's bytes and becomes `?`.
pub fn code_page_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|character| u8::try_from(u32::from(character)).unwrap_or(b'?'))
        .collect()
}

/// The code page a release writes its text in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodePage {
    /// Windows-1252: English, French, German, Italian and Spanish.
    #[default]
    Western,
    /// Windows-1251: Russian.
    Cyrillic,
}

/// How many talk-table strings `CodePage::of` reads. The first few thousand
/// strings of every release are its interface, items and first dialogue.
const SAMPLED: usize = 4000;

/// Windows-1252's characters for bytes 0x80 to 0x9F. Bytes 0xA0 to 0xFF are
/// Latin-1's. A byte the code page leaves out stands for itself.
const WESTERN_80: [u16; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0x008D, 0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
];

/// Windows-1251's characters for bytes 0x80 to 0xBF. Bytes 0xC0 to 0xFF are
/// the Russian alphabet, А (U+0410) to я (U+044F).
const CYRILLIC_80: [u16; 64] = [
    0x0402, 0x0403, 0x201A, 0x0453, 0x201E, 0x2026, 0x2020, 0x2021, 0x20AC, 0x2030, 0x0409, 0x2039,
    0x040A, 0x040C, 0x040B, 0x040F, 0x0452, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x0098, 0x2122, 0x0459, 0x203A, 0x045A, 0x045C, 0x045B, 0x045F, 0x00A0, 0x040E, 0x045E, 0x0408,
    0x00A4, 0x0490, 0x00A6, 0x00A7, 0x0401, 0x00A9, 0x0404, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x0407,
    0x00B0, 0x00B1, 0x0406, 0x0456, 0x0491, 0x00B5, 0x00B6, 0x00B7, 0x0451, 0x2116, 0x0454, 0x00BB,
    0x0458, 0x0405, 0x0455, 0x0457,
];

impl CodePage {
    /// The code page `talk` is written in. A Russian talk table is mostly
    /// Cyrillic letters, which are the bytes 0xC0 to 0xFF. A Western one is
    /// mostly ASCII letters with a few accented ones. The talk table's own
    /// language field does not tell them apart: Russian releases write 0,
    /// as English does.
    pub fn of(talk: &Tlk) -> CodePage {
        let (mut ascii, mut upper) = (0usize, 0usize);
        for entry in talk
            .entries
            .iter()
            .filter(|entry| !entry.text.is_empty())
            .take(SAMPLED)
        {
            for &byte in &entry.text {
                if byte.is_ascii_alphabetic() {
                    ascii += 1;
                } else if byte >= 0xC0 {
                    upper += 1;
                }
            }
        }
        if upper > ascii {
            CodePage::Cyrillic
        } else {
            CodePage::Western
        }
    }

    /// The character `byte` stands for.
    pub fn char(self, byte: u8) -> char {
        let code = match (self, byte) {
            (_, 0..=0x7F) => u32::from(byte),
            (CodePage::Western, 0x80..=0x9F) => u32::from(WESTERN_80[usize::from(byte - 0x80)]),
            (CodePage::Western, _) => u32::from(byte),
            (CodePage::Cyrillic, 0x80..=0xBF) => u32::from(CYRILLIC_80[usize::from(byte - 0x80)]),
            (CodePage::Cyrillic, _) => 0x0410 + u32::from(byte - 0xC0),
        };
        char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER)
    }

    /// The byte that stands for `character`, or `None` when the code page
    /// has no byte for it.
    pub fn byte(self, character: char) -> Option<u8> {
        let code = u32::from(character);
        if code < 0x80 {
            return Some(code as u8);
        }
        (0x80..=0xFF).find(|&byte| self.char(byte) == character)
    }

    /// The game's bytes as the letters they stand for, for text a person
    /// reads outside the game.
    pub fn readable(self, bytes: &[u8]) -> String {
        bytes.iter().map(|&byte| self.char(byte)).collect()
    }
}
