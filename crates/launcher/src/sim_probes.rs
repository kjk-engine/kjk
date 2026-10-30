//! `kjk sim probes`: the sim's probe groups (the I31 co-op slices'), with
//! no window and no install (`speeds` reads K1's where there is one). Each
//! group's checks are printed and `facts.json` is written.

use std::process::ExitCode;

/// A group's name and what runs it.
type Group = (&'static str, fn() -> sim::probe::Checks);

/// The groups, in slice order.
const GROUPS: &[Group] = &[
    ("containers", sim::containers::probes),
    ("drivers", sim::drivers::probes),
    ("switch", sim::switch::probes),
    ("coop talk", sim::coop_talk::probes),
    ("coop items", sim::coop_items::probes),
    ("scaling", sim::scaling::probes),
    ("encounter count", session::encounters::probes),
    ("speeds", speeds),
    ("scene", sim::scene_probes::all),
    ("dialog camera", crate::play_view::shot_probes),
    ("clip events", kjk_game::clip_events::probes),
    ("grunts", kjk_game::grunts::probes),
    ("battle music", kjk_game::battle_music::probes),
    ("flinch", kjk_game::acting::flinch_probes),
    ("damage numbers", kjk_game::floaty_view::damage_probes),
    ("screen shake", kjk_game::script_fx::shake_probes),
    ("line video effect", kjk_game::video_effect::line_probes),
    ("stunt events", crate::play_view::stunt_heard_probes),
    (
        "module resources",
        kjk_game::lifecycle::module_resolver_probes,
    ),
    ("conversation voice", kjk_game::dialogue_view::voice_probes),
    ("transcript tokens", kjk_game::transcript::probes),
    ("dialogue check", crate::scene_check::probes),
    ("falls", sim::falls::probes),
    ("arena", sim::arena_probes::probes),
    ("party", sim::party::probes),
    ("save copies", saves::copies::probes),
    ("save refusals", saves::refusal_probes),
    ("save safety", saves::safety_probes),
    ("flight", sim::flight::probes),
    ("crash", kjk_game::crash::probes),
    ("autosave", kjk_game::sim_host::autosave_probes),
    ("report", kjk_game::report::probes),
    ("report verbs", crate::report_verbs::probes),
    ("minigame", sim::minigame::probes::probes),
    ("minigame tsl", sim::minigame::tsl_probes::probes),
    ("minigame tracks", session::minigame_probes),
    ("galaxy and end", sim::galaxy::probes),
    ("pazaak", sim::pazaak::probes::probes),
    ("pazaak view", crate::pazaak_view::probes),
    ("feats", sim::feats::probes),
    ("destroys", sim::destroys::probes),
    ("floor items", sim::floor_probes::probes),
    ("tsl", sim::tsl_probes::probes),
    ("regeneration", sim::regen_probes::probes),
    ("weapon sets", sim::weapon_set_probes::probes),
    ("aim", sim::aim_probes::probes),
    ("dual pistols", sim::pair_probes::probes),
    ("tsl effects", sim::effects_probes::probes),
    ("tsl force", sim::force_probes::probes),
    ("ute encounters", sim::spawner_probes::probes),
    ("ute encounters in the games", crate::ute_encounter_probes::probes),
    ("influence", sim::script::influence::probes),
    ("presentation", sim::presentation_probes::probes),
    ("sheath", sim::strikes::sheath_probes),
    ("blade taken", sim::strikes::blade_taken_probes),
    ("minds", sim::minds_probes),
    ("pace", sim::pace_probes::probes),
    ("talk ends", sim::talk_end_probes::probes),
    ("speakers", sim::speaker_probes::probes),
    ("harness chase", crate::playthrough::probes),
    ("harness walls", crate::playthrough::wall_probes),
    ("harness fallen", crate::playthrough::fallen_probes),
    ("harness quiet", crate::playthrough::quiet_probes),
    ("harness straggle", crate::playthrough::straggle_probes),
    ("creation", crate::creation_probes::probes),
    ("workbench", sim::workshop_probes::probes),
    ("tsl workbench", crate::workbench_probes::probes),
    ("weapons view", crate::weapons::view_probes),
    ("up again", kjk_game::rise_probes::probes),
    ("clips", clips),
];

/// The scripts' and conversations' clips (`session::clip_probes`), on K1's
/// and TSL's data where this machine has the installs.
fn clips() -> sim::probe::Checks {
    let k1 = kjk_game::k1_resolver();
    let tsl = kjk_game::installs::open(&kjk_game::discover(), vfs::Game::Kotor2)
        .and_then(|install| install.resolver(None))
        .ok();
    let [k1, tsl] = [&k1, &tsl].map(|found| found.as_ref().map(crate::modules::ResolverSource));
    fn source<'a>(
        found: &'a Option<crate::modules::ResolverSource<'_>>,
    ) -> Option<&'a dyn asset_odyssey::module::Source> {
        found.as_ref().map(|source| source as _)
    }
    session::clip_probes(source(&k1), source(&tsl))
}

/// Creatures' speeds and head arc (`session::speed_probes`), with K1's own
/// table where this machine has the install.
fn speeds() -> sim::probe::Checks {
    let resolver = kjk_game::k1_resolver();
    let source = resolver.as_ref().map(crate::modules::ResolverSource);
    session::speed_probes(
        source
            .as_ref()
            .map(|source| source as &dyn asset_odyssey::module::Source),
    )
}

/// Runs every group, or with `only` the groups and cases it names
/// (comma-separated); fails if any check fails.
pub fn probes(only: Option<&str>) -> ExitCode {
    let wanted: Option<Vec<&str>> = only.map(|only| only.split(',').map(str::trim).collect());
    let picked = |name: &str| wanted.as_ref().is_none_or(|wanted| wanted.contains(&name));
    let mut failed = 0;
    let mut facts = Vec::new();
    for (group, run) in GROUPS {
        let whole = picked(group);
        let cases: Vec<_> = run()
            .into_iter()
            .filter(|(name, _)| whole || picked(name))
            .collect();
        if cases.is_empty() {
            continue;
        }
        outln!("{group}:");
        for (name, result) in cases {
            let (mark, detail) = match &result {
                Ok(detail) => ("ok  ", detail),
                Err(detail) => ("FAIL", detail),
            };
            outln!("  {mark} {name}: {detail}");
            failed += usize::from(result.is_err());
            facts.push(serde_json::json!({
                "group": group, "name": name, "pass": result.is_ok(), "detail": detail,
            }));
        }
    }
    outln!("sim probes: {} checks, {failed} failed", facts.len());
    crate::runtime::write_facts(serde_json::json!({
        "verb": "sim probes", "checks": facts, "failed": failed,
    }));
    if failed == 0 && !facts.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
