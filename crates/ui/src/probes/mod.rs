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
mod hud;
mod map_page;
mod menus;
mod minimap;
mod paint;
mod party;
mod pazaak;
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
    checks.extend(menus::all());
    checks.extend(menus::join());
    checks.extend(menus::new_or_load());
    checks.extend(menus::campaign());
    checks.extend(menus::saves());
    checks.extend(chargen::all());
    checks.extend(clicks::all());
    checks.extend(vm_screens::all());
    checks.extend(hud::all());
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
    checks.extend(menus::display_options());
    checks.extend(menus::pages());
    checks.extend(menus::equip_dresses_members());
    checks.extend(menus::equip_swaps_sets());
    checks.extend(hud::swap_button());
    checks.extend(menus::inventory_uses_items());
    checks
}
