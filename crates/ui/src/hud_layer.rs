//! KOTOR's HUD and the power wheel over play, drawn from the view model
//! alone. The game draws them each frame, and the catalogue draws them from
//! a story (docs/UI.md, "UI catalogue"). `Hud` keeps the HUD's layout as
//! read and placed, and the part of the HUD that seldom changes, so that a
//! frame of play draws only what moved. A new `Hud` draws what one kept
//! over many frames draws.

use std::sync::Arc;

use fmt_odyssey::gui::Gui;

use crate::assets::GuiAssets;
use crate::draw::{DrawList, Quad, Sizing, Textures, fit};
use crate::screens::hud;
use crate::skin::Source;
use crate::vm::ViewModel;

/// What one frame of the HUD is drawn with besides the HUD's own layout.
pub struct Frame<'a> {
    pub assets: &'a mut GuiAssets,
    pub vm: &'a ViewModel,
    pub palette: crate::paint::Palette,
    /// How lit what pulses is this frame (`Stack::pulsing`).
    pub pulse: f32,
    /// The display's physical pixels to each of the window's logical
    /// pixels.
    pub scale_factor: f32,
    /// How many times the game's settings changed in `vm.settings`. The
    /// HUD reads settings such as "Hide Quick Menu Buttons" and the Text
    /// size, so any change of a setting draws it again.
    pub settings: u32,
    /// Each control painted leaves a mark (`crate::paint::Painter::marked`)
    /// for a control tree.
    pub marked: bool,
}

/// KOTOR's HUD layout, once read, and the HUD as last drawn.
#[derive(Default)]
pub struct Hud {
    /// KOTOR's HUD layout, once read (none: it does not read), where it was
    /// read from, and whether it is the game's own rather than a mod's.
    layout: Option<Option<(Gui, Source, bool)>>,
    /// The layout as last placed, with what it was placed from and how much
    /// it was stretched down (`Hud::gui`).
    placed: Option<(Placed, Arc<Gui>, f32)>,
    /// The HUD as last drawn, and its banner, with what they were drawn
    /// from (`Hud::layer`).
    drawn: [Option<(Drawn, Kept)>; 2],
    /// What the floating target block keeps from frame to frame.
    pub float: hud::FloatHold,
}

/// The HUD's layer of one frame, and where the floating target block's
/// quads start in it (`Hud::layer`).
pub struct Layer {
    pub list: DrawList,
    pub block_at: Option<usize>,
}

/// What a click on the HUD is on.
pub enum Click {
    /// A companion's pazaak icon (`hud::watch_at`), by its body.
    Watch(u32),
    /// A companion's portrait (`hud::portrait_at`), by its body.
    Portrait(u32),
    /// The minimap (`hud::minimap_at`): the map page.
    Map,
    /// The solo mode button (`hud::solo_at`): the end of solo mode.
    Solo,
}

/// What KOTOR's HUD layout is placed from (`Hud::gui`), besides the layout
/// itself.
#[derive(PartialEq)]
struct Placed {
    window: (f32, f32),
    /// Whether the HUD's pictures keep their shape (`Hud::pictures`).
    pictures: bool,
    /// The Interface option "Target block at the top centre" is on.
    target_top: bool,
    /// The player plays with a controller, so the action slots stand in a
    /// cross (`hud::bar_cross`).
    cross: bool,
    /// How many action slots the HUD shows. The frame round the row is as
    /// wide as they are (`hud::bar_extra`).
    slots: usize,
    /// Which of the Form slot and the feat slot the HUD shows, inside the
    /// frame after the slots (`hud::EXTRA`).
    extras: [bool; 2],
}

/// KOTOR's HUD as kept between frames (`Hud::kept`): all of it but what
/// changes on most frames, which `Hud::layer` adds to a copy each frame.
struct Kept {
    /// The HUD without its minimap and its duel bars, with what pulses at
    /// full alpha.
    out: DrawList,
    /// Which of `out`'s quads pulse (`crate::paint::Painter::pulses`).
    pulses: Vec<std::ops::Range<usize>>,
    /// Where in `out`'s quads the minimap's arrow goes (its picture goes
    /// before them all), and where the duel bars go.
    arrow_at: usize,
    duel_at: usize,
}

/// The parts of the HUD drawn afresh each frame (`Hud::moving`).
#[derive(Default)]
struct Moving {
    /// The minimap's picture, under the layout, and its arrow, over it.
    under: Vec<Quad>,
    arrow: Option<Quad>,
    duel: Vec<Quad>,
}

/// All the kept part of KOTOR's HUD is drawn from (`Hud::layer`): the
/// view-model's parts it reads, the window, the palette, the campaign and
/// how often the module's art was let go. The same again, it draws the same.
#[derive(PartialEq)]
struct Drawn {
    window: (f32, f32),
    scale_factor: f32,
    /// Whether the HUD's pictures keep their shape (`Hud::pictures`).
    pictures: bool,
    /// The HUD's view without what `Hud::moving` draws: the player's place
    /// and turn on the minimap, and how full the duel bars are.
    hud: Option<crate::vm::HudView>,
    party: Vec<crate::vm::MemberView>,
    /// The target as the HUD drawn under the floating block shows it: its
    /// menu point left out, as the block is drawn afresh each frame
    /// (`Hud::target_block`).
    target: Option<crate::vm::TargetView>,
    /// The Interface option "Target block at the top centre" is on.
    target_top: bool,
    /// How many times the settings had changed (`Frame::settings`).
    settings: u32,
    calls: crate::vm::CallsView,
    requests: Option<crate::vm::RequestsView>,
    crosshair: bool,
    palette: crate::paint::Palette,
    game: vfs::Game,
    released: u32,
    marked: bool,
}

/// `draw` with KOTOR 2's sizes of text and borders, in the display's
/// physical pixels (`crate::draw::Sizing::at`), for a layer drawn `scale`
/// window pixels to each of its own pixels in a window of `window` pixels,
/// on a display of `scale_factor` physical pixels to each logical one. The
/// textures hold that sizing while `draw` runs, with the font letter the
/// stack set, and `draw` is given it too. The fonts, the painter's borders
/// and a list's rows all read it. The sizing the textures had before is put
/// back afterwards.
pub fn sized<T>(
    assets: &mut GuiAssets,
    (window, scale_factor): ((f32, f32), f32),
    scale: f32,
    draw: impl FnOnce(&mut GuiAssets, Sizing) -> Option<T>,
) -> Option<T> {
    let before = assets.sizing();
    let sizing = before.at(window, scale_factor, scale);
    assets.set_sizing(sizing);
    let drawn = draw(assets, sizing);
    assets.set_sizing(before);
    drawn
}

/// How many window pixels a pixel of the HUD's frame takes (`hud::place`
/// fits the frame to the window at kjk's scale).
pub fn scale(hud: &Gui, window: (f32, f32)) -> f32 {
    let (width, height) = hud.frame();
    fit(window, (width as f32, height as f32)).0
}

/// Whether the Interface option "Target block at the top centre" is on. Off,
/// the block floats over the target as KOTOR 2's does (`Hud::target_block`).
pub fn target_top(vm: &ViewModel) -> bool {
    crate::draw::kjk_option(&vm.settings, crate::draw::TARGET_TOP)
}

/// Whether the HUD draws its crosshair dot (`hud::crosshair`). A blaster
/// draws its own crosshair instead. A character with a melee weapon readied
/// has no crosshair. While the player blocks with targeted blocks on, the
/// arcs of targeted blocks take the dot's place. The HUD's redraw key
/// (`Drawn::crosshair`) holds this value, so the HUD is drawn again when it
/// changes.
pub fn dot_shown(vm: &ViewModel) -> bool {
    vm.crosshair && vm.reticle.is_none() && vm.block_reticle.is_none() && !vm.melee_out
}

impl Hud {
    /// KOTOR's HUD layout as read, where it was read from, and whether it
    /// is the game's own, once `Hud::gui` has read it and when it reads.
    pub fn layout(&self) -> Option<&(Gui, Source, bool)> {
        self.layout.as_ref()?.as_ref()
    }

    /// Whether the HUD's pictures keep their shape (`hud::place`): the HUD
    /// is the game's own layout, not a mod's, and "Maintain image aspect
    /// ratio" is on. False while the HUD has not been read.
    fn pictures(&self, vm: &ViewModel) -> bool {
        let own = matches!(self.layout, Some(Some((_, _, true))));
        own && crate::draw::image_aspect(&vm.settings)
    }

    /// KOTOR's HUD layout placed for a window of `window` pixels, where it
    /// was read from, and how much it was stretched down (`hud::place`),
    /// when it reads. The placed layout is kept and shared until the
    /// window, the pictures' shape (`Hud::pictures`) or the target block's
    /// place (`target_top`) changes. A caller that changes the layout
    /// clones it first.
    pub fn gui(
        &mut self,
        assets: &mut GuiAssets,
        vm: &ViewModel,
        window: (f32, f32),
    ) -> Option<(Arc<Gui>, Source, f32)> {
        use hud::{LAYOUT, backed, known, place};
        if self.layout.is_none() {
            let read = assets.screen_layout(LAYOUT).ok().map(|(mut gui, source)| {
                let own = !crate::assets::Assets::layout_is_mod(assets, LAYOUT, source);
                if !own && let Ok(stock) = assets.stock_screen_layout(LAYOUT) {
                    known(&mut gui, &stock);
                }
                if let Ok(campaign) = assets.layout(LAYOUT) {
                    backed(&mut gui, &campaign);
                }
                (gui, source, own)
            });
            self.layout = Some(read);
            // A layout placed before this one was read is another layout's.
            self.placed = None;
        }
        let from = Placed {
            window,
            pictures: self.pictures(vm),
            target_top: target_top(vm),
            cross: vm.pad_playing,
            slots: vm.hud.as_ref().map_or(0, |hud| hud.bar.len()),
            extras: (vm.hud.as_ref())
                .map_or([false; 2], |hud| hud.extra().map(|slot| slot.is_some())),
        };
        let (layout, source, _) = self.layout()?;
        if let Some((last, gui, stretch)) = &self.placed
            && *last == from
        {
            return Some((gui.clone(), *source, *stretch));
        }
        let (mut gui, source) = (layout.clone(), *source);
        let stretch = place(&mut gui, window, from.pictures);
        if from.target_top {
            hud::to_top(&mut gui);
        }
        hud::bar_extra(&mut gui, from.slots, from.extras);
        hud::solo_beside_portrait(&mut gui);
        if from.cross {
            hud::bar_cross(&mut gui);
        }
        let gui = Arc::new(gui);
        self.placed = Some((from, gui.clone(), stretch));
        Some((gui, source, stretch))
    }

    /// What the click at `point` (window pixels) on the HUD is on, if
    /// anything.
    pub fn click(
        &mut self,
        assets: &mut GuiAssets,
        vm: &ViewModel,
        window: (f32, f32),
        point: [f32; 2],
    ) -> Option<Click> {
        let (gui, ..) = self.gui(assets, vm, window)?;
        let (width, height) = gui.frame();
        let (scale, [left, top]) = fit(window, (width as f32, height as f32));
        let at = [(point[0] - left) / scale, (point[1] - top) / scale];
        hud::watch_at(&gui, vm, at)
            .map(Click::Watch)
            .or_else(|| hud::portrait_at(&gui, vm, at).map(Click::Portrait))
            .or_else(|| hud::minimap_at(&gui, vm, at).then_some(Click::Map))
            .or_else(|| hud::solo_at(&gui, vm, at).then_some(Click::Solo))
    }

    /// The floating target block for a window of `window` pixels, drawn
    /// afresh each frame over the HUD as KOTOR 2 moves it (`hud::floated`),
    /// while there is a target and the block is not at the top centre. The
    /// HUD under it leaves the block out. `pointer` is the free pointer
    /// (window pixels). The game calls this once a frame, which
    /// `hud::FloatHold` counts on.
    pub fn target_block(
        &mut self,
        frame: &mut Frame<'_>,
        window: (f32, f32),
        pointer: Option<[f32; 2]>,
    ) -> Option<DrawList> {
        if target_top(frame.vm) {
            return None;
        }
        let Some(target) = frame.vm.target.clone() else {
            self.float.point(None, pointer);
            return None;
        };
        let (gui, source, _) = self.gui(frame.assets, frame.vm, window)?;
        // Its own copy: the block is moved in it and the rest is dropped.
        let mut gui = Gui::clone(&gui);
        let Frame {
            assets,
            vm,
            palette,
            pulse,
            scale_factor,
            marked,
            ..
        } = frame;
        let float = &mut self.float;
        sized(
            assets,
            (window, *scale_factor),
            scale(&gui, window),
            |assets, sizing| {
                hud::floated(&mut gui, &target, sizing.pixel, float);
                let pointer = pointer.map(|point| {
                    let (width, height) = gui.frame();
                    let (scale, [left, top]) = fit(window, (width as f32, height as f32));
                    [(point[0] - left) / scale, (point[1] - top) / scale]
                });
                float.point(Some((&gui, &target)), pointer);
                gui.controls.retain(|control| hud::in_block(&control.tag));
                let mut out = DrawList::default();
                let mut painter = crate::paint::Painter::new(assets, *palette);
                painter.source = source;
                painter.pulsing = *pulse;
                painter.marked = *marked;
                painter.layout(&gui, &|control| hud::state(control, vm), &mut out);
                hud::target_cycle(&gui, vm, &mut painter, &mut out);
                Some(out)
            },
        )
    }

    /// The power wheel for a window of `window` pixels, in the frame of the
    /// HUD's layout as placed for the window, while the wheel is open
    /// (`crate::screens::wheel`).
    pub fn wheel(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<DrawList> {
        let wheel = frame.vm.wheel.clone()?;
        let (gui, source, _) = self.gui(frame.assets, frame.vm, window)?;
        let (palette, pulse, marked) = (frame.palette, frame.pulse, frame.marked);
        sized(
            frame.assets,
            (window, frame.scale_factor),
            scale(&gui, window),
            |assets, sizing| {
                let mut painter = crate::paint::Painter::new(assets, palette);
                painter.source = source;
                painter.pulsing = pulse;
                painter.marked = marked;
                Some(crate::screens::wheel::draw(
                    &gui,
                    &wheel,
                    sizing.pixel,
                    &mut painter,
                ))
            },
        )
    }

    /// KOTOR's HUD for a window of `window` pixels, when the game is in
    /// play and its layout reads; with `banner`, only its banner (a break, a
    /// ready check: `hud::banner`), when there is one. `block` is the
    /// floating target block as last placed (`Hud::target_block`), which
    /// the HUD holds among its quads at `Layer::block_at`.
    pub fn layer(
        &mut self,
        frame: &mut Frame<'_>,
        window: (f32, f32),
        banner: bool,
        block: Option<&DrawList>,
    ) -> Option<Layer> {
        if banner {
            frame.vm.calls.banner.as_ref()?;
        } else {
            frame.vm.hud.as_ref()?;
        }
        let vm = frame.vm;
        let from = Drawn {
            window,
            scale_factor: frame.scale_factor,
            pictures: self.pictures(vm),
            hud: vm.hud.clone().map(|hud| crate::vm::HudView {
                minimap: hud.minimap.map(|minimap| crate::vm::MinimapView {
                    at: [0.0; 2],
                    spin: 0.0,
                    ..minimap
                }),
                duel: hud.duel.map(|_| crate::vm::DuelBars::default()),
                ..hud
            }),
            party: vm.party.clone(),
            target: vm.target.clone().map(|target| crate::vm::TargetView {
                point: None,
                ..target
            }),
            target_top: target_top(vm),
            settings: frame.settings,
            calls: vm.calls.clone(),
            requests: vm.requests.clone(),
            crosshair: dot_shown(vm),
            palette: frame.palette,
            game: frame.assets.game,
            released: frame.assets.released(),
            marked: frame.marked,
        };
        let at = usize::from(banner);
        if !matches!(&self.drawn[at], Some((last, _)) if *last == from) {
            let kept = self.kept(frame, window, banner)?;
            self.drawn[at] = Some((from, kept));
        }
        let moving = if banner {
            Moving::default()
        } else {
            self.moving(frame, window).unwrap_or(Moving::default())
        };
        let (_, kept) = self.drawn[at].as_ref()?;
        // The kept quads with this frame's among them, in the order one
        // drawing of the whole HUD gives.
        let (arrow_at, duel_at) = (kept.arrow_at, kept.duel_at);
        let arrows = usize::from(moving.arrow.is_some());
        let added = moving.under.len() + arrows + moving.duel.len();
        let mut quads = Vec::with_capacity(kept.out.quads.len() + added);
        quads.extend_from_slice(&moving.under);
        quads.extend_from_slice(&kept.out.quads[..arrow_at]);
        quads.extend(moving.arrow);
        quads.extend_from_slice(&kept.out.quads[arrow_at..duel_at]);
        quads.extend_from_slice(&moving.duel);
        quads.extend_from_slice(&kept.out.quads[duel_at..]);
        // What pulses was drawn at full alpha: this frame's pulse dims it.
        for range in &kept.pulses {
            let mut moved = moving.under.len();
            if range.start >= arrow_at {
                moved += arrows;
            }
            if range.start >= duel_at {
                moved += moving.duel.len();
            }
            for quad in &mut quads[range.start + moved..range.end + moved] {
                quad.color[3] *= frame.pulse;
            }
        }
        let mut out = DrawList {
            size: kept.out.size,
            fill: kept.out.fill,
            quads,
            wide: kept.out.wide.clone(),
            sized: kept.out.sized.clone(),
            marks: kept.out.marks.clone(),
        };
        let mut block_at = None;
        if !banner {
            // The floating target block as last placed. The game places it
            // again once the camera has moved, and finds it here.
            block_at = Some(out.quads.len());
            if let Some(block) = block {
                out.quads.extend(block.quads.iter().cloned());
                out.marks.extend(block.marks.iter().cloned());
            }
        }
        // A blaster's reticle over the HUD as kept: its gap changes as it
        // fires, the HUD under it seldom. Placed in the HUD's frame as placed
        // (`hud::place` fits it to the window).
        let placed = self.layout();
        let frame_at = placed.map(|(gui, ..)| hud::fitted(gui, window));
        // The interface colour of the HUD's layout: KOTOR 1's blue or KOTOR 2's
        // green.
        let recoloured = placed.is_some_and(|(_, source, _)| {
            crate::draw::Textures::recoloured(&*frame.assets, *source)
        });
        let tint = crate::skin::interface_colour(recoloured);
        // One of the display's physical pixels in the frame's pixels.
        let pixel = placed.map_or(1.0, |(gui, ..)| {
            1.0 / (scale(gui, window) * frame.scale_factor).max(f32::EPSILON)
        });
        let crosshair = vm.crosshair && !banner;
        if let (Some(frame_at), true) = (frame_at, crosshair) {
            use hud::{MIN_GAP, block_arcs, gap_px, hitmarker, reticle};
            let height = frame_at.1 as f32;
            let gap = vm.reticle.map_or(MIN_GAP * height / 1080.0, |shown| {
                gap_px(shown.cone, shown.fov, height)
            });
            // While the player blocks with targeted blocks on, the arcs of
            // targeted blocks take the crosshair's place.
            if let (Some(shown), None) = (vm.reticle, vm.block_reticle) {
                out.quads
                    .extend(reticle(frame_at, shown.family, gap, pixel, tint));
            }
            // A landed shot's mark about it.
            if let Some((mark, age_ms)) = vm.hitmark {
                out.quads.extend(hitmarker(frame_at, mark, gap, age_ms));
            }
            if let Some(view) = vm.block_reticle {
                out.quads.extend(block_arcs(frame_at, view));
            }
        }
        // The perfect block mark, as a ring wherever no arcs show it: also
        // while the crosshair is down, as it is under a side camera.
        let arcs = crosshair && vm.block_reticle.is_some();
        if let (Some(frame_at), true, false, false) = (frame_at, vm.perfect_mark, arcs, banner) {
            out.quads.extend(hud::perfect_ring(frame_at));
        }
        Some(Layer {
            list: out,
            block_at,
        })
    }

    /// The HUD, its floating target block, the power wheel and the banner
    /// of one still frame for a window of `window` pixels, under to over,
    /// as a frame of play draws them where nothing floats over creatures.
    pub fn still(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Vec<DrawList> {
        let block = self.target_block(frame, window, None);
        let hud = self.layer(frame, window, false, block.as_ref());
        let wheel = self.wheel(frame, window);
        let banner = self.layer(frame, window, true, None);
        let hud = hud.map(|layer| layer.list);
        let banner = banner.map(|layer| layer.list);
        [hud, wheel, banner].into_iter().flatten().collect()
    }

    /// The minimap and the duel bars for a window of `window` pixels, as
    /// this frame's view has them. They change on most frames of play, so
    /// `Hud::layer` draws them each frame and keeps the rest of the HUD.
    fn moving(&mut self, frame: &mut Frame<'_>, window: (f32, f32)) -> Option<Moving> {
        let shown = frame.vm.hud.as_ref()?;
        if shown.minimap.is_none() && shown.duel.is_none() {
            return None;
        }
        let (gui, source, stretch) = self.gui(frame.assets, frame.vm, window)?;
        let (vm, palette) = (frame.vm, frame.palette);
        sized(
            frame.assets,
            (window, frame.scale_factor),
            scale(&gui, window),
            |assets, _| {
                let shown = vm.hud.as_ref()?;
                // The minimap's picture under the layout (its border), the arrow over.
                let minimap = shown
                    .minimap
                    .as_ref()
                    .and_then(|view| hud::minimap(&gui, view, stretch, assets, source));
                let mut duel = DrawList::default();
                if shown.duel.is_some() {
                    let mut painter = crate::paint::Painter::new(assets, palette);
                    painter.source = source;
                    if let Some(art) = hud::BarArt::vitality(&gui, &mut painter) {
                        hud::duel_bars(&gui, vm, art, stretch, &mut duel);
                    }
                }
                Some(Moving {
                    under: minimap.iter().flat_map(hud::Minimap::under).collect(),
                    arrow: minimap.and_then(|minimap| minimap.arrow),
                    duel: duel.quads,
                })
            },
        )
    }

    /// The kept part of KOTOR's HUD drawn afresh (`Hud::layer`): all but
    /// its minimap and its duel bars (`Hud::moving`), with what pulses in it
    /// at full alpha. Its text and borders are KOTOR 2's size in the
    /// display's physical pixels (`sized`). Its pieces stand where KOTOR 2
    /// places them (`hud::place`). The target block is left out while it
    /// floats (`Hud::target_block`), and stands at the top centre with the
    /// Interface option "Target block at the top centre" on (`hud::to_top`).
    fn kept(&mut self, frame: &mut Frame<'_>, window: (f32, f32), banner: bool) -> Option<Kept> {
        let (gui, source, _) = self.gui(frame.assets, frame.vm, window)?;
        let (vm, palette, marked) = (frame.vm, frame.palette, frame.marked);
        // The floating target block is drawn on its own (`Hud::target_block`).
        let floating = !target_top(vm);
        sized(
            frame.assets,
            (window, frame.scale_factor),
            scale(&gui, window),
            |assets, sizing| {
                let mut out = DrawList::default();
                let mut painter = crate::paint::Painter::new(assets, palette);
                painter.source = source;
                painter.marked = marked;
                painter.layout(
                    &gui,
                    &|control| {
                        if banner {
                            hud::banner(control, vm)
                        } else if floating && hud::in_block(&control.tag) {
                            crate::paint::ControlState {
                                hidden: true,
                                ..crate::paint::ControlState::default()
                            }
                        } else {
                            hud::state(control, vm)
                        }
                    },
                    &mut out,
                );
                let arrow_at = out.quads.len();
                if !banner {
                    hud::effects(&gui, vm, &mut painter, &mut out);
                    hud::busy(&gui, vm, &mut painter, &mut out);
                    hud::players(&gui, vm, &mut painter, &mut out);
                    hud::action_keys(&gui, vm, &mut painter, &mut out);
                    if !floating {
                        hud::target_cycle(&gui, vm, &mut painter, &mut out);
                    }
                }
                let duel_at = out.quads.len();
                if dot_shown(vm) {
                    out.quads.extend(hud::crosshair(
                        &gui,
                        sizing.pixel,
                        painter.interface_colour(),
                    ));
                }
                let pulses = std::mem::take(&mut painter.pulses);
                Some(Kept {
                    out,
                    pulses,
                    arrow_at,
                    duel_at,
                })
            },
        )
    }
}
