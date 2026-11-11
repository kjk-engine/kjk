//! KOTOR 2's sizing of text, and mods told from the game's own files: the
//! stock files Aspyr ships in the override folder, the "a" and "b" font
//! sheets, and Aspyr's larger text in very wide windows.

use std::path::PathBuf;

use fmt_odyssey::ResType;
use vfs::resolver::{Layer, Location, Rank, Resolved};

use super::paint::Fake;
use super::{Check, check, expect, near};
use crate::assets::font_file;
use crate::draw::{SmallFonts, text_size};
use crate::input::UiInput;
use crate::screen::Stack;
use crate::vm::ViewModel;

/// Whether a file `name` of `size` bytes, alone in a layer of `rank` called
/// `folder`, is a mod's.
fn is_mod(rank: Rank, folder: &str, name: &str, size: u64) -> bool {
    let (resref, extension) = name.rsplit_once('.').expect("a name with an extension");
    let restype = ResType::from_extension(extension).expect("a resource type");
    let location = Location::on_disk(PathBuf::from(folder).join(name), 0, size);
    let layer = Layer {
        name: folder.into(),
        rank,
        resources: [((resref.to_ascii_lowercase(), restype), location.clone())].into(),
    };
    Resolved {
        layer: &layer,
        location: &location,
        shadowed: Vec::new(),
    }
    .is_mod()
}

/// The text scale and font letter an empty stack sets in a window of
/// `window` pixels with `factor` physical pixels to each, with "Use Small
/// Fonts" set to `small` (unset if none).
fn stepped(window: (f32, f32), factor: f32, small: Option<&str>) -> (Option<f32>, Option<char>) {
    let mut fake = Fake::new();
    let mut vm = ViewModel::default();
    if let Some(small) = small {
        vm.settings.insert(SmallFonts::SETTING.into(), small.into());
    }
    let input = UiInput {
        window,
        scale_factor: factor,
        ..UiInput::default()
    };
    Stack::new(|_, _| None).step(&mut fake, &vm, &input);
    (fake.text_scales.first().copied(), fake.font_suffix)
}

pub(super) fn all() -> Vec<Check> {
    // Aspyr's stock files by name and size, any case; a different size, a
    // Workshop item's override, and a file outside the override folders.
    let stock = !is_mod(Rank::Directory, "override", "custom.txt", 4_884)
        && !is_mod(Rank::Directory, "override", "MainMenu8x6_p.gui", 14_367);
    let resized = is_mod(Rank::Directory, "override", "mainmenu8x6_p.gui", 14_368);
    let workshop = is_mod(Rank::Directory, "workshop/1/override", "equip_p.gui", 1_000);
    let packed = !is_mod(Rank::Erf2, "texturepacks/swpc_tex_gui.erf", "lbl_x.tga", 10);
    // A layout of a stock name and size whose bytes are not Aspyr's: a mod
    // that only changed numbers in it.
    let edited = {
        let folder = std::env::temp_dir().join(format!("kjk-stock-{}", std::process::id()));
        let written = std::fs::create_dir_all(&folder)
            .and_then(|()| std::fs::write(folder.join("equip_p.gui"), vec![0u8; 37_553]));
        let edited = written.is_ok()
            && is_mod(
                Rank::Directory,
                &folder.to_string_lossy(),
                "equip_p.gui",
                37_553,
            );
        let _ = std::fs::remove_dir_all(&folder);
        edited
    };

    // KOTOR 2's own data has "b" sheets and the credits' "a" sheet alone.
    let sheets = ["dialogfont16x16b", "fnt_creditsa", "fnt_consoleb"];
    let file =
        |name: &str, suffix: Option<char>| font_file(name, suffix, |file| sheets.contains(&file));
    let files = [
        file("dialogfont16x16", Some('b')),
        file("dialogfont16x16", Some('a')),
        file("fnt_credits", Some('a')),
        file("fnt_console", Some('b')),
        file("dialogfont16x16", None),
    ];
    let letters = [
        SmallFonts::Auto.suffix(1279.0),
        SmallFonts::Auto.suffix(1280.0),
        SmallFonts::named("small").suffix(3840.0),
        SmallFonts::named("large").suffix(800.0),
        SmallFonts::named("other").suffix(800.0),
    ];
    let set_by_stack = [
        stepped((1024.0, 768.0), 1.0, None).1,
        stepped((1920.0, 1080.0), 1.0, None).1,
        stepped((1024.0, 768.0), 1.0, Some("large")).1,
        stepped((1920.0, 1080.0), 1.0, Some("small")).1,
    ];

    // Aspyr's text sizes, and the stack's text scale over kjk's scale of a
    // layout pixel (the window's height over 720).
    let sizes = [1920.0, 2045.0, 2046.0, 2560.0, 3840.0].map(|width| text_size((width, 1080.0)));
    let scales = [(1920.0, 1080.0), (2560.0, 1440.0), (3840.0, 2160.0)]
        .map(|window| stepped(window, 1.0, None).0.unwrap_or(0.0));

    // A Retina display, 2 physical pixels to each of the window's: a
    // 1024 × 768 window is 2048 pixels wide to KOTOR 2, which reads the "b"
    // sheets and draws text 1.5 times its size; a 13-inch MacBook's 1470 ×
    // 956 window is 2940 wide, 1.5 times too. A font pixel is then 1.5
    // physical pixels, 0.75 of the window's.
    let retina = [(1024.0, 768.0), (1470.0, 956.0)].map(|window| stepped(window, 2.0, None));
    let retina_scale = |window: (f32, f32)| 0.75 / (window.1 / 720.0).max(1.0);

    vec![
        check("stock_override_files_are_not_mods", {
            expect("Aspyr's custom.txt and main menu are the game's own", stock)
                .and_then(|()| expect("a main menu of another size is a mod's", resized))
                .and_then(|()| expect("a Workshop item's layout is a mod's", workshop))
                .and_then(|()| expect("a texture pack's art is the game's own", packed))
                .and_then(|()| {
                    expect(
                        "a layout of Aspyr's name and size with other bytes is a mod's",
                        edited,
                    )
                })
                .map(|()| {
                    "by name and size (a layout by its bytes too), in the override folders only"
                        .into()
                })
        }),
        check("font_sheet_letters", {
            expect(
                &format!("the sheets read: {files:?}"),
                files
                    == [
                        "dialogfont16x16b",
                        "dialogfont16x16",
                        "fnt_creditsa",
                        "fnt_console",
                        "dialogfont16x16",
                    ],
            )
            .and_then(|()| {
                expect(
                    &format!("the letters by option and width: {letters:?}"),
                    letters == ['a', 'b', 'a', 'b', 'a'],
                )
            })
            .and_then(|()| {
                expect(
                    &format!("the letters the stack sets: {set_by_stack:?}"),
                    set_by_stack == [Some('a'), Some('b'), Some('b'), Some('a')],
                )
            })
            .map(|()| "b from 1280 wide, a below, the name as written where none".into())
        }),
        check("aspyr_text_size", {
            expect(
                &format!("1, 1, 1.5, 1.5, 2 times: {sizes:?}"),
                sizes == [1.0, 1.0, 1.5, 1.5, 2.0],
            )
            .and_then(|()| {
                expect(
                    &format!("the stack's text scales 1/1.5, 1.5/2, 2/3: {scales:?}"),
                    near(scales[0], 1.0 / 1.5)
                        && near(scales[1], 1.5 / 2.0)
                        && near(scales[2], 2.0 / 3.0),
                )
            })
            .map(|()| "1 to 2045 wide, 1.5 at 2560, 2 at 3840".into())
        }),
        check("text_in_physical_pixels", {
            let [(small_scale, small_letter), (mac_scale, mac_letter)] = retina;
            expect(
                &format!("the b sheets by the physical width: {small_letter:?}, {mac_letter:?}"),
                small_letter == Some('b') && mac_letter == Some('b'),
            )
            .and_then(|()| {
                expect(
                    &format!("a font pixel 0.75 window pixels: {small_scale:?}, {mac_scale:?}"),
                    small_scale.is_some_and(|by| near(by, retina_scale((1024.0, 768.0))))
                        && mac_scale.is_some_and(|by| near(by, retina_scale((1470.0, 956.0)))),
                )
            })
            .map(|()| "Aspyr's rules read the display's physical pixels".into())
        }),
    ]
}
