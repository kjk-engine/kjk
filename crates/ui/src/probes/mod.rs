//! Checks on layouts, fonts and screens built here (no game data):
//! `kjk gui probes` prints them; the approved test `gui_layouts` needs
//! every one to pass.

mod bark;
mod chargen;
mod clicks;
mod dialog;
mod draw;
mod floaty;
mod formats;
mod galaxy;
mod hitmarker;
mod hud;
mod map_page;
mod menus;
mod minimap;
mod paint;
mod party;
mod pazaak;
mod reticle;
mod sizing;
mod stack;
mod status_summary;
mod transition;
mod vm_screens;

/// A probe's result.
#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

fn check(name: &'static str, result: Result<String, String>) -> Check {
    Check {
        name,
        ok: result.is_ok(),
        detail: result.unwrap_or_else(|why| why),
    }
}

fn expect(what: &str, ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err(what.to_string()) }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

pub fn all() -> Vec<Check> {
    let mut checks = formats::layouts();
    checks.extend(formats::fonts());
    checks.extend(draw::rasters());
    checks.extend(paint::all());
    checks.extend(stack::all());
    checks.extend(stack::mod_layouts());
    checks.extend(stack::menu_widths());
    checks.extend(stack::physical_borders());
    checks.extend(stack::picture_pieces());
    checks.extend(stack::popups());
    checks.extend(sizing::all());
    checks.extend(menus::all());
    checks.extend(menus::join());
    checks.extend(menus::new_or_load());
    checks.extend(menus::campaign());
    checks.extend(menus::saves());
    checks.extend(chargen::all());
    checks.extend(clicks::all());
    checks.extend(vm_screens::all());
    checks.extend(hud::all());
    checks.extend(reticle::all());
    checks.extend(hitmarker::all());
    checks.extend(minimap::all());
    checks.extend(map_page::all());
    checks.extend(bark::all());
    checks.extend(transition::all());
    checks.extend(floaty::all());
    checks.extend(status_summary::all());
    checks.extend(dialog::all());
    checks.extend(galaxy::all());
    checks.extend(party::all());
    checks.extend(pazaak::all());
    checks.extend(menus::in_game());
    checks.extend(menus::review());
    checks.extend(menus::options());
    checks.extend(menus::key_rebinding());
    checks.extend(menus::key_names());
    checks.extend(menus::pad_menus());
    checks.extend(menus::pad_saves());
    checks.push(check("pad_icons", crate::icons::probe()));
    checks.extend(menus::display_options());
    checks.extend(menus::interface_options());
    checks.extend(menus::pages());
    checks.extend(menus::equip_dresses_members());
    checks.extend(menus::equip_swaps_sets());
    checks.extend(menus::equip_headgear_eye());
    checks.extend(menus::equip_lists_as_kotor());
    checks.extend(menus::equip_tells_off_hand());
    checks.extend(menus::equip_locks_slots());
    checks.extend(menus::equip_lists_open_at_top());
    checks.extend(hud::swap_button());
    checks.extend(hud::wide_windows());
    checks.extend(hud::effect_icons_stack());
    checks.extend(menus::inventory_uses_items());
    checks
}
