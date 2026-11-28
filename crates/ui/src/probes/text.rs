//! The games' text in other languages: every byte reaches the fonts
//! unchanged, a talk table tells its code page, and a name box turns typed
//! letters into the game's bytes.

use std::sync::Arc;

use super::{Check, check};
use crate::assets::GuiAssets;
use crate::draw::{Sizing, SmallFonts, Textures};
use crate::edit::EditBox;
use crate::skin::Source;
use fmt_odyssey::CodePage;
use fmt_odyssey::tlk::{TEXT_PRESENT, Tlk, TlkEntry};
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Game, Install};

pub fn all() -> Vec<Check> {
    vec![
        check("text_bytes_round_trip", round_trip()),
        check("text_code_pages", code_pages()),
        check("text_name_box", name_box()),
        check("text_caret", caret()),
        check("font_upper_glyph_fallback", upper_glyph_fallback()),
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

/// The caret moves through the text, letters go in and come out at it, and
/// a fresh box's first key still replaces its default.
fn caret() -> Result<String, String> {
    let page = CodePage::Western;
    let mut edit = EditBox::new(16);
    edit.type_text("gvecredts", page);
    edit.home();
    edit.step(1);
    edit.type_text("i", page);
    edit.end();
    edit.step(-2);
    edit.type_text("i", page);
    let typed = (edit.as_str(), edit.caret());
    edit.end();
    edit.type_text("XYZ1234567", page);
    let full = (edit.as_str(), edit.caret());
    edit.step(-20);
    edit.delete();
    edit.step(3);
    edit.backspace();
    let cut = (edit.as_str(), edit.caret());
    let mut fresh = EditBox::with("Player", 16);
    fresh.step(-2);
    fresh.type_text("R", page);
    let replaced = (fresh.as_str(), fresh.caret());
    let want = (
        ("givecredits".to_string(), 9),
        ("givecreditsXYZ12".to_string(), 16),
        ("ivcreditsXYZ12".to_string(), 2),
        ("R".to_string(), 1),
    );
    let got = (typed, full, cut, replaced);
    match got == want {
        true => Ok("typed at the caret, cut either side, the default replaced".into()),
        false => Err(format!(
            "typed, filled, cut and fresh {got:?}, not {want:?}"
        )),
    }
}

/// A 64 × 64 bottom-up 32-bit TGA holding a 16 × 16 grid of 4 × 4 glyph cells.
/// If `distinct_upper` is false, every cell from `0x80` up holds the same
/// placeholder box (as TSL's 256 × 128 `dialogfont10x10` does).
fn font_sheet(distinct_upper: bool) -> (Vec<u8>, Vec<u8>) {
    let side = 64usize;
    let mut tga = vec![0u8; 18 + side * side * 4];
    tga[2] = 2;
    tga[12..14].copy_from_slice(&(side as u16).to_le_bytes());
    tga[14..16].copy_from_slice(&(side as u16).to_le_bytes());
    tga[16] = 32;
    tga[17] = 8;
    for glyph in 0..256usize {
        let (gx, gy) = ((glyph % 16) * 4, (glyph / 16) * 4);
        let pattern = if glyph >= 0x80 && !distinct_upper {
            0xFFFFu16
        } else {
            ((glyph as u16).wrapping_mul(37)) ^ 0xA55A
        };
        for dy in 0..4usize {
            for dx in 0..4usize {
                let bit = dy * 4 + dx;
                let alpha = if (pattern >> bit) & 1 != 0 { 255 } else { 0 };
                let row = side - 1 - (gy + dy);
                let at = 18 + (row * side + gx + dx) * 4;
                tga[at..at + 4].copy_from_slice(&[255, 255, 255, alpha]);
            }
        }
    }
    let mut txi = String::from("numchars 256\nfontheight 0.16\nupperleftcoords 256\n");
    for glyph in 0..256usize {
        let (u0, v0) = ((glyph % 16) as f32 / 16.0, 1.0 - (glyph / 16) as f32 / 16.0);
        txi.push_str(&format!("{u0:.6} {v0:.6} 0\n"));
    }
    txi.push_str("lowerrightcoords 256\n");
    for glyph in 0..256usize {
        let (u1, v1) = (
            ((glyph % 16) + 1) as f32 / 16.0,
            1.0 - ((glyph / 16) + 1) as f32 / 16.0,
        );
        txi.push_str(&format!("{u1:.6} {v1:.6} 0\n"));
    }
    (tga, txi.into_bytes())
}

/// When `SmallFonts::Small` (or a window `<= 1279` px wide) looks up a font
/// with no `"a"` sheet and the unsuffixed sheet lacks distinct upper glyphs
/// (`0x80..=0xFF`), `GuiAssets` falls back to its `"b"` sheet rather than
/// drawing placeholder boxes for Cyrillic and accented characters.
fn upper_glyph_fallback() -> Result<String, String> {
    let root = std::env::temp_dir().join(format!("kjk-font-upper-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let over = root.join("override");
    std::fs::create_dir_all(&over).map_err(|e| e.to_string())?;
    let mut key = vec![0u8; 64];
    key[..8].copy_from_slice(b"KEY V1  ");
    key[16..20].copy_from_slice(&64u32.to_le_bytes());
    key[20..24].copy_from_slice(&64u32.to_le_bytes());
    std::fs::write(root.join("chitin.key"), &key).map_err(|e| e.to_string())?;

    let (small_tga, small_txi) = font_sheet(false);
    let (large_tga, large_txi) = font_sheet(true);
    std::fs::write(over.join("dialogfont10x10.tga"), &small_tga).map_err(|e| e.to_string())?;
    std::fs::write(over.join("dialogfont10x10.txi"), &small_txi).map_err(|e| e.to_string())?;
    std::fs::write(over.join("dialogfont10x10b.tga"), &large_tga).map_err(|e| e.to_string())?;
    std::fs::write(over.join("dialogfont10x10b.txi"), &large_txi).map_err(|e| e.to_string())?;
    std::fs::write(over.join("dialogfont16x16.tga"), &large_tga).map_err(|e| e.to_string())?;
    std::fs::write(over.join("dialogfont16x16.txi"), &large_txi).map_err(|e| e.to_string())?;
    std::fs::write(over.join("fnt_console.tga"), &small_tga).map_err(|e| e.to_string())?;
    std::fs::write(over.join("fnt_console.txi"), &small_txi).map_err(|e| e.to_string())?;

    let install = Install {
        game: Game::Kotor2,
        root: root.clone(),
        data_root: root.clone(),
        source: vfs::Source::GogDefault,
        store: vfs::Store::Unknown,
        markers: Vec::new(),
        workshop: false,
        ja_mod: None,
    };
    let index = Arc::new(DirIndex::of_install(&install).map_err(|e| e.to_string())?);
    let base =
        Resolver::base(Game::Kotor2, &index, TextureQuality::High).map_err(|e| e.to_string())?;
    let mut assets =
        GuiAssets::with(Game::Kotor2, index, base.clone()).with_skin(Game::Kotor2, base);
    assets.set_sizing(Sizing::physical(
        (1440.0, 900.0),
        1.0,
        1.25,
        SmallFonts::Small,
        1.0,
    ));
    let (_, small_font) = assets
        .font(Source::Skin, "dialogfont10x10")
        .ok_or("dialogfont10x10 missing")?;
    let (_, big_font) = assets
        .font(Source::Skin, "dialogfont16x16")
        .ok_or("dialogfont16x16 missing")?;
    let (_, console_font) = assets
        .font(Source::Skin, "fnt_console")
        .ok_or("fnt_console missing")?;

    // Narrow window with Auto also requests 'a' and must fall back to 'b'.
    assets.set_sizing(Sizing::physical(
        (1024.0, 768.0),
        1.0,
        1.0,
        SmallFonts::Auto,
        1.0,
    ));
    let (_, narrow_font) = assets
        .font(Source::Skin, "dialogfont10x10")
        .ok_or("dialogfont10x10 narrow missing")?;
    let _ = std::fs::remove_dir_all(&root);

    if small_font.texture != "dialogfont10x10b" {
        return Err(format!(
            "dialogfont10x10 loaded {:?} instead of dialogfont10x10b",
            small_font.texture
        ));
    }
    if narrow_font.texture != "dialogfont10x10b" {
        return Err(format!(
            "narrow dialogfont10x10 loaded {:?} instead of dialogfont10x10b",
            narrow_font.texture
        ));
    }
    if big_font.texture != "dialogfont16x16" {
        return Err(format!(
            "dialogfont16x16 loaded {:?} instead of dialogfont16x16",
            big_font.texture
        ));
    }
    if console_font.texture != "fnt_console" {
        return Err(format!(
            "fnt_console loaded {:?} instead of fnt_console",
            console_font.texture
        ));
    }
    Ok("incomplete unsuffixed sheet falls back to b; complete and console stay".into())
}
