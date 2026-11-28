//! Everything drawn over play besides the screens of the stack: KOTOR's HUD
//! (`crate::hud_layer`), the words floating over creatures, the area
//! transition panel, the arena's line and panel, a terminal's camera feed,
//! barks, the status summaries, the conversation, the host's request card
//! and photo mode's corner lines. The game draws them each frame from the
//! view model, and the catalogue draws them from a capture of play
//! (docs/UI.md, "UI catalogue"), both through `OverPlay::compose`.
//! `OverPlay` keeps their layouts as read.

use fmt_odyssey::gui::Gui;

use crate::draw::{DrawList, Textures, fit};
use crate::hud_layer::{Frame, Hud, sized};
use crate::screens::{
    arena_line, arena_panel, bark, dialog, floaty, photo, status_summary, transition,
};
use crate::skin::Source;

/// The layouts drawn over play, once read (none: it does not read).
#[derive(Default)]
pub struct OverPlay {
    pub hud: Hud,
    bark: Option<Option<bark::Bubble>>,
    dialog: Option<Option<dialog::Dialog>>,
    summary: Option<Option<(Gui, Source)>>,
    transition: Option<Option<transition::Panel>>,
    /// The scroll bar of KOTOR 2's Messages page, which the console's log
    /// scrolls with, once read (`OverPlay::console`).
    console_bar: Option<Option<fmt_odyssey::gui::Control>>,
}

/// The status summaries' layer and their OK as drawn (window pixels: left,
/// top, width, height), while one waits for it (`OverPlay::summaries`).
struct Summaries {
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
        let marked = frame.brush.marks();
        let mut painter = frame.painter(Source::Campaign);
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
            Some(art) => floaty::bars(&vm.overhead, art, window, marked),
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
    fn arena_panel(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let panel = frame.vm.arena_panel.clone()?;
        let (hud, ..) = self.hud.gui(frame.assets, frame.vm, window)?;
        let (wide, tall) = hud.frame();
        let at = hud.find(arena_panel::UNDER).map_or([8.0, 132.0], |map| {
            let extent = &map.extent;
            [extent.left as f32, (extent.top + extent.height) as f32]
        });
        let mut painter = frame.painter(Source::Campaign);
        Some(arena_panel::draw(
            &mut painter,
            &panel,
            (wide as f32, tall as f32),
            at,
        ))
    }

    /// The floors stage's line for a window of `window` pixels.
    fn arena_line(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let words = frame.vm.arena.as_ref()?;
        let mut painter = frame.painter(Source::Campaign);
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
    /// KOTOR 2 puts it, while the main character faces a door or exit that
    /// leads out and the layout reads (`crate::screens::transition`). It is
    /// placed in the window's physical pixels, as KOTOR 2 places it, and
    /// greyed while a member of the party is down.
    fn transition(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let words = frame.vm.transition.clone()?;
        let healthy = transition::healthy(frame.vm.party.iter().map(|member| member.hp));
        let (physical, text) = physical_window(frame, window);
        let under = self
            .bark_bottom(frame, window)
            .max(self.top_block_bottom(frame, window));
        let per_pixel = 1.0 / frame.scale_factor.max(f32::EPSILON);
        let brush = frame.brush;
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
                let mut painter = brush.painter(assets, panel.source);
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
    fn summaries(
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
        let (vm, brush) = (frame.vm, frame.brush);
        let read = &mut self.summary;
        sized(
            frame.assets,
            (window, frame.scale_factor),
            per_pixel,
            |assets, _| {
                let (gui, source) = read
                    .get_or_insert_with(|| assets.screen_layout(status_summary::LAYOUT).ok())
                    .as_ref()?;
                let mut painter = brush.painter(assets, Source::Campaign);
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
    fn bark(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let said = frame.vm.bark.as_ref()?;
        let brush = frame.brush;
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
        let mut painter = brush.painter(assets, bubble.source);
        painter.layout(
            &gui,
            &|control| bark::state(control, said, bubble.words),
            &mut out,
        );
        Some(out)
    }

    /// The conversation screen for a window of `window` pixels, while the
    /// main character is in a cinematic conversation
    /// (`crate::screens::dialog`).
    fn dialog(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let placed = self.dialog_placed(frame, window)?;
        let view = frame.vm.dialog.as_ref()?;
        let source = self.dialog.as_ref()?.as_ref()?.source;
        let mut out = DrawList {
            fill: true,
            ..DrawList::default()
        };
        let palette = dialog::palette(Some(frame.assets.game), frame.brush.palette);
        let mut painter = frame.painter(source);
        painter.palette = palette;
        placed.draw(view, source, &mut painter, &mut out);
        Some(out)
    }

    /// The host's request card for a window of `window` pixels while a
    /// guest asks it in (`crate::screens::dialog::Dialog::card`), whether the
    /// host talks (under its top bar) or walks.
    /// The console, while it is open, in KOTOR 2's colours whichever game is
    /// played (`crate::screens::console`). It is drawn over everything, so
    /// the game draws it after `compose`, in play and out of it.
    pub fn console(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let view = frame.vm.console.as_ref()?;
        let assets = &mut *frame.assets;
        let bar = self.console_bar.get_or_insert_with(|| {
            let (gui, _) = assets.screen_layout("messages").ok()?;
            gui.find("LB_MESSAGES")?.scrollbar.as_deref().cloned()
        });
        let mut brush = frame.brush;
        brush.palette = crate::paint::Palette::TSL;
        let mut painter = brush.painter(assets, Source::Campaign);
        Some(crate::screens::console::draw(
            &mut painter,
            view,
            window,
            bar.as_ref(),
        ))
    }

    fn card(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let talking = self.dialog_placed(frame, window);
        let requests = frame.vm.requests.as_ref()?;
        let brush = frame.brush;
        let assets = &mut *frame.assets;
        let read = self
            .dialog
            .get_or_insert_with(|| dialog::Dialog::read(assets))
            .as_ref()?;
        let card = read.card(assets, requests, window, talking.as_ref())?;
        let mut out = DrawList::default();
        let mut painter = brush.painter(assets, Source::Campaign);
        card.draw(read.source, &mut painter, &mut out);
        Some(out)
    }

    /// Everything drawn over play and the screens of the stack, `screens`,
    /// in a window of `window` pixels, under to over: the HUD and its
    /// floating target block, the floating words, the area transition
    /// panel, the power wheel, the arena's line and panel, a terminal's
    /// camera feed, a bark, the status summaries, the conversation, the
    /// host's request card, the screens, the banner of a break or a ready
    /// check, and photo mode's corner lines.
    ///
    /// The HUD and what stands with it give way to a conversation, to a
    /// minigame's own HUD and to a fade, as KOTOR's do. So do the status
    /// summaries. A bark gives way to a conversation only. The Hide
    /// interface key hides the HUD, the words floating over creatures, the
    /// transition panel, the arena's line and panel, and barks. The power
    /// wheel still shows while its key is held, so that whoever holds it
    /// sees what they choose. Photo mode hides all of that and the
    /// rest besides the wheel, the screens and its own lines.
    pub fn compose(
        &mut self,
        frame: &mut Frame<'_>,
        window: (f32, f32),
        state: PlayState<'_>,
        screens: Vec<Shown>,
    ) -> Composed {
        use crate::screens::hud::LAYOUT as HUD;
        let covered = state.talking || state.minigame || state.faded;
        let hidden = state.hidden || state.photo.is_some();
        let in_play = state.photo.is_none();
        let fresh;
        let block = match &state.floating {
            Some(placed) => placed.block,
            None => {
                fresh = self.hud.target_block(frame, window, None);
                fresh.as_ref()
            }
        };
        let hud = self.hud.layer(frame, window, false, block);
        let mut composed = Composed {
            layers: Vec::new(),
            hud: hud.is_some(),
            block_at: hud.as_ref().and_then(|hud| hud.block_at),
            hud_shown: false,
            ok: None,
        };
        let mut layers: Vec<(&'static str, &'static str, Option<DrawList>)> = Vec::new();
        if let Some(hud) = hud.filter(|_| !covered) {
            if !hidden {
                let words = match &state.floating {
                    Some(placed) => placed.words.cloned(),
                    None => self.floaty(frame, window),
                };
                layers.push(("hud", HUD, Some(hud.list)));
                layers.push(("floaty", "", words));
                layers.push((
                    "transition",
                    transition::LAYOUT,
                    self.transition(frame, window),
                ));
                composed.hud_shown = true;
            }
            layers.push(("wheel", HUD, self.hud.wheel(frame, window)));
            if !hidden {
                layers.push(("arena_line", "", self.arena_line(frame, window)));
                layers.push(("arena_panel", "", self.arena_panel(frame, window)));
            }
        }
        if in_play && frame.vm.camera_feed {
            let feed = crate::screens::feed::draw(window, state.clock);
            layers.push(("feed", "", Some(feed)));
        }
        if !state.talking && !hidden {
            layers.push(("bark", bark::LAYOUT, self.bark(frame, window)));
        }
        if in_play && !covered {
            let summaries = self.summaries(frame, window, state.corner);
            let summaries = summaries.map(|summaries| {
                composed.ok = summaries.ok;
                summaries.list
            });
            layers.push(("status_summary", status_summary::LAYOUT, summaries));
        }
        if in_play {
            layers.push(("dialog", dialog::LAYOUT, self.dialog(frame, window)));
            layers.push(("dialog", dialog::LAYOUT, self.card(frame, window)));
        }
        composed.layers = named(layers);
        composed.layers.extend(screens);
        let mut over = Vec::new();
        if in_play {
            let banner = self.hud.layer(frame, window, true, None);
            over.push(("hud", HUD, banner.map(|layer| layer.list)));
        }
        if let Some(lines) = state.photo {
            let mut painter = frame.painter(Source::Campaign);
            over.push(("photo", "", photo::draw(&mut painter, lines, window)));
        }
        composed.layers.extend(named(over));
        composed
    }
}

/// The layers drawn, each named by its module and layout.
fn named(layers: Vec<(&'static str, &'static str, Option<DrawList>)>) -> Vec<Shown> {
    layers
        .into_iter()
        .filter_map(|(screen, layout, list)| {
            Some(Shown {
                screen: screen.to_string(),
                layout: layout.to_string(),
                list: list?,
            })
        })
        .collect()
}

/// How play stands in a frame, which decides what `OverPlay::compose`
/// hides.
#[derive(Default)]
pub struct PlayState<'a> {
    /// The main character is in a cinematic conversation.
    pub talking: bool,
    /// A minigame shows, with its own HUD.
    pub minigame: bool,
    /// A fade covers the view, as the black between a cutscene's
    /// conversations does.
    pub faded: bool,
    /// The Hide interface key hid the interface.
    pub hidden: bool,
    /// Photo mode's corner lines, while photo mode is on.
    pub photo: Option<&'a photo::Lines>,
    /// The status summaries stand in the window's corner
    /// (`OverPlay::summaries`).
    pub corner: bool,
    /// The stack's clock, which a terminal's camera feed rolls with
    /// (`crate::screens::feed`).
    pub clock: f32,
    /// The floating target block and floating words as the game last
    /// placed them. The game places them again once the camera has moved.
    /// Without them, as in a still frame of the catalogue, they are drawn
    /// afresh.
    pub floating: Option<Floating<'a>>,
}

/// The floating target block and the floating words as last placed
/// (`PlayState::floating`).
pub struct Floating<'a> {
    pub block: Option<&'a DrawList>,
    pub words: Option<&'a DrawList>,
}

/// A frame as `OverPlay::compose` stacks it.
#[derive(Default)]
pub struct Composed {
    /// Its layers, bottom first.
    pub layers: Vec<Shown>,
    /// KOTOR's HUD was drawn, also while something covers or hides it.
    pub hud: bool,
    /// Where the floating target block's quads start in the HUD's layer
    /// (`crate::hud_layer::Layer::block_at`), while the HUD was drawn.
    pub block_at: Option<usize>,
    /// The HUD is shown. Its layer is then the first, and the floating
    /// words, when there are any, the second.
    pub hud_shown: bool,
    /// The OK of a status summary that waits for it, as drawn (window
    /// pixels: left, top, width, height).
    pub ok: Option<[f32; 4]>,
}

/// One layer of a frame: the module that drew it (a screen's, or one drawn
/// over play), the layout it drew (empty for a layer drawn without one),
/// and what it drew.
pub struct Shown {
    pub screen: String,
    pub layout: String,
    pub list: DrawList,
}
