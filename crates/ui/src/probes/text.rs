//! The games' text in other languages: every byte reaches the fonts
//! unchanged, a talk table tells its code page, and a name box turns typed
//! letters into the game's bytes.

use super::{Check, check};
use crate::edit::EditBox;
use fmt_odyssey::CodePage;
use fmt_odyssey::tlk::{TEXT_PRESENT, Tlk, TlkEntry};

pub fn all() -> Vec<Check> {
    vec![
        check("text_bytes_round_trip", round_trip()),
        check("text_code_pages", code_pages()),
        check("text_name_box", name_box()),
    ]
}

fn talk(line: &[u8]) -> Tlk {
    Tlk {
        language: 0,
        entries: vec![TlkEntry {
            flags: TEXT_PRESENT,
            sound: String::new(),
            volume_variance: 0,
            pitch_variance: 0,
            text: line.to_vec(),
            sound_length: 0,
        }],
        lent: Default::default(),
    }
}

/// Every byte survives a trip through a `String` and back.
fn round_trip() -> Result<String, String> {
    let all: Vec<u8> = (0..=255).collect();
    let back = fmt_odyssey::code_page_bytes(&fmt_odyssey::code_page(&all));
    if back != all {
        return Err(format!("bytes changed: {back:?}"));
    }
    Ok("all 256 bytes kept".into())
}

/// A talk table tells its code page, and each code page reads its letters
/// and types them back.
fn code_pages() -> Result<String, String> {
    let told: [(&[u8], CodePage); 3] = [
        (b"Welcome to the Endar Spire.", CodePage::Western),
        (b"Willkommen an Bord. Gr\xfc\xdfe!", CodePage::Western),
        // "Добро пожаловать" in Windows-1251.
        (
            b"\xc4\xee\xe1\xf0\xee \xef\xee\xe6\xe0\xeb\xee\xe2\xe0\xf2\xfc",
            CodePage::Cyrillic,
        ),
    ];
    for (line, page) in told {
        let found = CodePage::of(&talk(line));
        if found != page {
            return Err(format!("{line:?}: {found:?}, not {page:?}"));
        }
    }
    let letters: [(CodePage, &[u8], &str); 2] = [
        (
            CodePage::Western,
            b"Gr\xfc\xdfe \x96 caf\xe9\x85",
            "Grüße – café…",
        ),
        (
            CodePage::Cyrillic,
            b"\xc4\xe0\xf0\xf2 \xd0\xe5\xe2\xe0\xed \xa8\xb8",
            "Дарт Реван Ёё",
        ),
    ];
    for (page, bytes, read) in letters {
        let shown = page.readable(bytes);
        let typed: Vec<u8> = read.chars().filter_map(|c| page.byte(c)).collect();
        if shown != read || typed != bytes {
            return Err(format!("{page:?}: read {shown:?}, typed {typed:?}"));
        }
    }
    Ok("Western and Cyrillic told apart, read and typed".into())
}

/// A name box keeps the game's bytes and types each code page's letters.
fn name_box() -> Result<String, String> {
    let typed: [(CodePage, &str, &[u8]); 3] = [
        (CodePage::Western, "Zoë Müller", b"Zo\xeb M\xfcller"),
        (
            CodePage::Cyrillic,
            "Бастила",
            b"\xc1\xe0\xf1\xf2\xe8\xeb\xe0",
        ),
        // A Cyrillic letter has no Western byte and is left out.
        (CodePage::Western, "Revan Д", b"Revan "),
    ];
    for (page, letters, bytes) in typed {
        let mut edit = EditBox::new(16);
        edit.type_text(letters, page);
        if edit.text != bytes {
            return Err(format!("{letters:?} typed as {:?}", edit.text));
        }
    }
    let name = b"J\xe9r\xf4me";
    let edit = EditBox::with(&fmt_odyssey::code_page(name), 16);
    if edit.text != name {
        return Err(format!("a game name starts the box as {:?}", edit.text));
    }
    Ok("typed letters become the code page's bytes".into())
}
