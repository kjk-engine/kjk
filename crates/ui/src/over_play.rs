//! Everything drawn over play besides the screens of the stack: KOTOR's HUD
//! (`crate::hud_layer`), the words floating over creatures, the area
//! transition panel, the arena's line and panel, a terminal's camera feed,
//! barks, the status summaries, the conversation and the host's request
//! card. The game draws them each frame from the view model, and the
//! catalogue draws them from a capture of play (docs/UI.md, "UI catalogue").
//! `OverPlay` keeps their layouts as read.

use fmt_odyssey::gui::Gui;

use crate::draw::{DrawList, Textures, fit};
use crate::hud_layer::{Frame, Hud, sized};
use crate::screens::{arena_line, arena_panel, bark, dialog, floaty, status_summary, transition};
use crate::skin::Source;

/// The layouts drawn over play, once read (none: it does not read).
#[derive(Default)]
pub struct OverPlay {
    pub hud: Hud,
    bark: Option<Option<bark::Bubble>>,
    dialog: Option<Option<dialog::Dialog>>,
    summary: Option<Option<(Gui, Source)>>,
    transition: Option<Option<transition::Panel>>,
}

/// The status summaries' layer and their OK as drawn (window pixels: left,
/// top, width, height), while one waits for it (`OverPlay::summaries`).
pub struct Summaries {
    pub list: DrawList,
    pub ok: Option<[f32; 4]>,
}

/// How far below the top-centre target block the area name stands, in
/// layout pixels of the HUD.
const UNDER_BLOCK: i32 = 4;

/// `window` (logical pixels) in the display's physical pixels, and the
/// text size there. The text size is Aspyr's for the window times the
/// player's text scale (`crate::draw::Sizing::text_size`), read from the
/// sizing the stack's step left.
fn physical_window(frame: &Frame<'_>, window: (f32, f32)) -> ((i32, i32), f32) {
    let wide = (window.0 * frame.scale_factor, window.1 * frame.scale_factor);
    (
        (wide.0.round() as i32, wide.1.round() as i32),
        frame.assets.sizing().text_size(wide),
    )
}

impl OverPlay {
    /// The conversation screen placed for what it shows now, in a window
    /// of `window` pixels, while there is a conversation and its layout
    /// reads.
    pub fn dialog_placed(
        &mut self,
        frame: &mut Frame<'_>,
        window: (f32, f32),
    ) -> Option<dialog::Placed> {
        let view = frame.vm.dialog.as_ref()?;
        let assets = &mut *frame.assets;
        let read = self
            .dialog
            .get_or_insert_with(|| dialog::Dialog::read(assets))
            .as_ref()?;
        read.place(assets, view, window)
    }

    /// The words floating over creatures for a window of `window` pixels,
    /// while there are any (`crate::screens::floaty`).
    pub fn floaty(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let vm = frame.vm;
        if vm.floaties.is_empty() && vm.overhead.is_empty() {
            return None;
        }
        let mut painter = crate::paint::Painter::new(frame.assets, frame.palette);
        // The Stamina bar wears the HUD's portrait bar art, in its layout's
        // source.
        let art = self.hud.layout().and_then(|(gui, source, _)| {
            use crate::screens::hud::BarArt;
            painter.source = *source;
            Some(floaty::OverheadArt {
                upper: BarArt::vitality(gui, &mut painter)?,
            })
        });
        let mut out = match art {
            Some(art) => floaty::bars(&vm.overhead, art, window),
            None => DrawList {
                size: window,
                ..DrawList::default()
            },
        };
        let words = floaty::draw(&mut painter, &vm.floaties, window);
        out.quads.extend(words.quads);
        Some(out)
    }

    /// The combat arena's panel of keys for a window of `window` pixels, in
    /// the arena: in the HUD's frame, under its minimap.
    pub fn arena_panel(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let panel = frame.vm.arena_panel.clone()?;
        let (hud, ..) = self.hud.gui(frame.assets, frame.vm, window)?;
        let (wide, tall) = hud.frame();
        let at = hud.find(arena_panel::UNDER).map_or([8.0, 132.0], |map| {
            let extent = &map.extent;
            [extent.left as f32, (extent.top + extent.height) as f32]
        });
        let mut painter = crate::paint::Painter::new(frame.assets, frame.palette);
        Some(arena_panel::draw(
            &mut painter,
            &panel,
            (wide as f32, tall as f32),
            at,
        ))
    }

    /// The floors stage's line for a window of `window` pixels.
    pub fn arena_line(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let words = frame.vm.arena.as_ref()?;
        let mut painter = crate::paint::Painter::new(frame.assets, frame.palette);
        Some(arena_line::line(&mut painter, words, window))
    }

    /// The bark bubble's bottom in physical pixels for a window of `window`
    /// pixels, while a bark shows and its layout reads (`OverPlay::bark`).
    fn bark_bottom(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<i32> {
        let said = frame.vm.bark.as_ref()?;
        let assets = &mut *frame.assets;
        let bubble = self
            .bark
            .get_or_insert_with(|| bark::Bubble::read(assets))
            .as_ref()?;
        let gui = bark::place(&bubble.gui, assets, bubble.source, said, window)?;
        let (width, height) = gui.frame();
        let (scale, [_, top]) = crate::draw::fill(window, (width as f32, height as f32));
        let bottom = gui
            .controls
            .iter()
            .map(|control| control.extent.top + control.extent.height)
            .max()?;
        Some(((top + bottom as f32 * scale) * frame.scale_factor).round() as i32)
    }

    /// The bottom of the target block and its action's words in physical
    /// pixels for a window of `window` pixels, plus a small gap, while the
    /// Interface option "Target block at the top centre" is on. The area
    /// name stands under it there, as kjk placed it before, so the two never
    /// cover each other.
    fn top_block_bottom(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<i32> {
        if !crate::hud_layer::target_top(frame.vm) {
            return None;
        }
        let (hud, _, _) = self.hud.gui(frame.assets, frame.vm, window)?;
        let (width, height) = hud.frame();
        let (scale, [_, top]) = crate::draw::fill(window, (width as f32, height as f32));
        let bottom = crate::screens::hud::block_bottom(&hud) + UNDER_BLOCK;
        Some(((top + bottom as f32 * scale) * frame.scale_factor).round() as i32)
    }

    /// The area transition panel for a window of `window` pixels, where
    /// KOTOR 2 puts it, while the player faces a door or exit that leads out
    /// and the layout reads (`crate::screens::transition`). It is placed in
    /// the window's physical pixels, as KOTOR 2 places it, and greyed while
    /// a member of the party is down.
    pub fn transition(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let words = frame.vm.transition.clone()?;
        let healthy = transition::healthy(frame.vm.party.iter().map(|member| member.hp));
        let (physical, text) = physical_window(frame, window);
        let under = self
            .bark_bottom(frame, window)
            .max(self.top_block_bottom(frame, window));
        let per_pixel = 1.0 / frame.scale_factor.max(f32::EPSILON);
        let palette = frame.palette;
        let read = &mut self.transition;
        sized(
            frame.assets,
            (window, frame.scale_factor),
            per_pixel,
            |assets, _| {
                let panel = read
                    .get_or_insert_with(|| transition::Panel::read(assets))
                    .as_ref()?;
                let mut gui = transition::place(&panel.gui, physical, text, under);
                transition::shown(&mut gui, healthy);
                let mut out = DrawList::default();
                let mut painter = crate::paint::Painter::new(assets, palette);
                painter.source = panel.source;
                painter.layout(
                    &gui,
                    &|control| transition::state(control, &words, panel.words, healthy),
                    &mut out,
                );
                Some(out)
            },
        )
    }

    /// The status summaries for a window of `window` pixels, centred in it
    /// as KOTOR 2 centres its summary, while there are any and the layout
    /// reads (`crate::screens::status_summary`). They are placed in the
    /// window's physical pixels, as KOTOR 2 places them. With `corner` (the
    /// Interface option "Notifications in the corner", and always in a
    /// headless run) they stand at the window's right side instead, under
    /// the HUD's menu buttons and as wide as them (`OverPlay::corner`).
    pub fn summaries(
        &mut self,
        frame: &mut Frame<'_>,
        window: (f32, f32),
        corner: bool,
    ) -> Option<Summaries> {
        if frame.vm.summaries.is_empty() {
            return None;
        }
        let corner = corner.then(|| self.corner(frame, window)).flatten();
        let (physical, text) = physical_window(frame, window);
        let per_pixel = 1.0 / frame.scale_factor.max(f32::EPSILON);
        let (vm, palette) = (frame.vm, frame.palette);
        let read = &mut self.summary;
        sized(
            frame.assets,
            (window, frame.scale_factor),
            per_pixel,
            |assets, _| {
                let (gui, source) = read
                    .get_or_insert_with(|| assets.screen_layout(status_summary::LAYOUT).ok())
                    .as_ref()?;
                let mut painter = crate::paint::Painter::new(assets, palette);
                let (out, ok) = status_summary::draw(
                    &mut painter,
                    gui,
                    *source,
                    &vm.summaries,
                    (physical, text),
                    corner,
                );
                Some(Summaries {
                    list: out,
                    ok: ok.map(|rect| rect.map(|value| value * per_pixel)),
                })
            },
        )
    }

    /// Where the notifications in the corner stand, in physical pixels
    /// (`OverPlay::summaries`), when the HUD's layout reads. They start 12
    /// window pixels under the backing of the HUD's menu buttons in the top
    /// right corner, as wide as the backing and lined up with its right
    /// edge. With the Feedback option "Hide Quick Menu Buttons" on, or a
    /// layout without the backing, they start 12 pixels under the window's
    /// top and are as wide as their words.
    fn corner(
        &mut self,
        frame: &mut Frame<'_>,
        window: (f32, f32),
    ) -> Option<status_summary::Corner> {
        let factor = frame.scale_factor;
        let hidden = crate::feedback::on(&frame.vm.settings, crate::feedback::HIDE_MENU_BUTTONS);
        let (hud, ..) = self.hud.gui(frame.assets, frame.vm, window)?;
        let (width, height) = hud.frame();
        let (scale, [left, top]) = fit(window, (width as f32, height as f32));
        let backing = hud
            .find("LBL_MENUBG")
            .filter(|_| !hidden)
            .map(|backing| backing.extent);
        let below = backing.map_or(0.0, |backing| (backing.top + backing.height) as f32);
        let under = backing.map(|backing| {
            let right = left + (backing.left + backing.width) as f32 * scale;
            (right * factor, backing.width as f32 * scale * factor)
        });
        Some(status_summary::Corner {
            top: (top + below * scale + 12.0) * factor,
            under,
        })
    }

    /// The bark bubble for a window of `window` pixels, while there is a
    /// bark and its layout reads (`crate::screens::bark`).
    pub fn bark(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let said = frame.vm.bark.as_ref()?;
        let assets = &mut *frame.assets;
        let bubble = self
            .bark
            .get_or_insert_with(|| bark::Bubble::read(assets))
            .as_ref()?;
        let gui = bark::place(&bubble.gui, assets, bubble.source, said, window)?;
        let mut out = DrawList {
            fill: true,
            ..DrawList::default()
        };
        let mut painter = crate::paint::Painter::new(assets, frame.palette);
        painter.source = bubble.source;
        painter.layout(
            &gui,
            &|control| bark::state(control, said, bubble.words),
            &mut out,
        );
        Some(out)
    }

    /// The conversation screen for a window of `window` pixels, while this
    /// player is in a cinematic conversation (`crate::screens::dialog`).
    pub fn dialog(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let placed = self.dialog_placed(frame, window)?;
        let view = frame.vm.dialog.as_ref()?;
        let source = self.dialog.as_ref()?.as_ref()?.source;
        let mut out = DrawList {
            fill: true,
            ..DrawList::default()
        };
        let palette = dialog::palette(Some(frame.assets.game), frame.palette);
        let mut painter = crate::paint::Painter::new(frame.assets, palette);
        placed.draw(view, source, &mut painter, &mut out);
        Some(out)
    }

    /// The host's request card for a window of `window` pixels while a
    /// guest asks it in (`crate::screens::dialog::Dialog::card`), whether the
    /// host talks (under its top bar) or walks.
    pub fn card(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let talking = self.dialog_placed(frame, window);
        let requests = frame.vm.requests.as_ref()?;
        let assets = &mut *frame.assets;
        let read = self
            .dialog
            .get_or_insert_with(|| dialog::Dialog::read(assets))
            .as_ref()?;
        let card = read.card(assets, requests, window, talking.as_ref())?;
        let mut out = DrawList::default();
        let mut painter = crate::paint::Painter::new(assets, frame.palette);
        card.draw(read.source, &mut painter, &mut out);
        Some(out)
    }

    /// Everything drawn over play in one still frame for a window of
    /// `window` pixels, under to over, as a frame of play stacks it: the
    /// HUD and its floating target block, the floating words, the area
    /// transition panel, the power wheel, the arena's line and panel, a
    /// terminal's camera feed at `clock` seconds, a bark, the status
    /// summaries (in the corner with `corner`), the conversation, the host's
    /// request card and the banner. A conversation hides the HUD and what
    /// stands with it, barks and the summaries, as in play. Each layer comes
    /// with the module that draws it and the layout it draws (empty for a
    /// layer drawn without one).
    pub fn still(
        &mut self,
        frame: &mut Frame<'_>,
        window: (f32, f32),
        (clock, corner): (f32, bool),
    ) -> Vec<Still> {
        use crate::screens::hud::LAYOUT as HUD;
        let talking = frame.vm.dialog.is_some();
        let mut layers: Vec<(&'static str, &'static str, Option<DrawList>)> = Vec::new();
        if !talking {
            let block = self.hud.target_block(frame, window, None);
            if let Some(hud) = self.hud.layer(frame, window, false, block.as_ref()) {
                layers.push(("hud", HUD, Some(hud.list)));
                layers.push(("floaty", "", self.floaty(frame, window)));
                layers.push((
                    "transition",
                    transition::LAYOUT,
                    self.transition(frame, window),
                ));
                layers.push(("wheel", HUD, self.hud.wheel(frame, window)));
                layers.push(("arena_line", "", self.arena_line(frame, window)));
                layers.push(("arena_panel", "", self.arena_panel(frame, window)));
            }
        }
        if frame.vm.camera_feed {
            layers.push(("feed", "", Some(crate::screens::feed::draw(window, clock))));
        }
        if !talking {
            layers.push(("bark", bark::LAYOUT, self.bark(frame, window)));
            let summaries = self.summaries(frame, window, corner);
            layers.push((
                "status_summary",
                status_summary::LAYOUT,
                summaries.map(|summaries| summaries.list),
            ));
        }
        layers.push(("dialog", dialog::LAYOUT, self.dialog(frame, window)));
        layers.push(("dialog", dialog::LAYOUT, self.card(frame, window)));
        let banner = self.hud.layer(frame, window, true, None);
        layers.push(("hud", HUD, banner.map(|layer| layer.list)));
        layers
            .into_iter()
            .filter_map(|(module, layout, list)| {
                Some(Still {
                    module,
                    layout,
                    list: list?,
                })
            })
            .collect()
    }
}

/// One layer of `OverPlay::still`.
pub struct Still {
    pub module: &'static str,
    pub layout: &'static str,
    pub list: DrawList,
}
