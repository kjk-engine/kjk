//! The corpus's probes (`kjk sim probes`, group "save corpus"): two
//! synthetic slot folders, one current and one older than a fake `breaking`
//! entry, and the table that lists them.

use super::*;
use sim::probe::{Checks, check};

/// The fake `breaking` entry: saves of a version below 60 are refused.
const FAKE: &str = "# not an entry\n- KJKV 60 (fake-bump): why old saves can't be read\n";

/// In a folder of its own under the system's temporary one:
/// - a slot written reads back whole, and a slot whose world is not the one
///   its meta hashed is refused;
/// - the corpus's slots are found by release and name;
/// - a current save passes, and one older than the breaking entry is
///   expected; the same save with no entry before it, or one it is not
///   before, fails;
/// - a slot is written only as a plain folder name, and the neighbours stay;
/// - a panic in one save's step is that save's FAIL row, the table whole;
/// - what the table says of both, and that a failing row fails the run.
pub fn probes() -> Checks {
    let root = std::env::temp_dir().join(format!("kjk-save-corpus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let made = synthetic(&root);
    let checks = match &made {
        Ok(slots) => vec![
            ("slot_round_trip", round_trip(&root, slots)),
            ("slots_found", found(&root)),
            ("current_passes", current_passes(slots)),
            ("older_expected", older_expected(slots)),
            ("table_prints", table_prints(slots)),
            ("slot_names_kept_to_the_slot", slot_names(&root, slots)),
            ("panic_is_a_row", panic_is_a_row()),
        ],
        Err(error) => vec![("synthetic_slots", Err(error.clone()))],
    };
    let _ = std::fs::remove_dir_all(&root);
    checks
}

/// The two slots' worlds: this build's, and the same with a version before
/// the fake entry's.
struct Slots {
    current: Vec<u8>,
    older: Vec<u8>,
}

fn synthetic(root: &Path) -> Result<Slots, String> {
    let (_, sink) = sim::fixtures::kitchen_sink();
    let current = sink.encode();
    let mut older = current.clone();
    older[4..8].copy_from_slice(&1u32.to_le_bytes());
    for (release, name, world) in [("v9.9.9", "current", &current), ("v0.0.1", "older", &older)] {
        let slot = Slot {
            world: world.clone(),
            meta: Some(Meta {
                name: name.into(),
                version: Some(release.into()),
                ..Meta::default()
            }),
            story: None,
        };
        slot.write(&root.join(release), name)?;
    }
    Ok(Slots { current, older })
}

fn round_trip(root: &Path, slots: &Slots) -> Result<String, String> {
    let folder = root.join("v9.9.9").join("current");
    let slot = Slot::open(&folder)?;
    let named = slot.meta.as_ref().map(|meta| meta.name.as_str());
    let world = folder.join(WORLD);
    let mut bytes = std::fs::read(&world).map_err(|error| error.to_string())?;
    bytes[0] ^= 1;
    std::fs::write(&world, bytes).map_err(|error| error.to_string())?;
    let damaged = Slot::open(&folder).err().unwrap_or_else(String::new);
    check(
        slot.world == slots.current && named == Some("current") && damaged.contains("damaged"),
        format!("named {named:?}, damaged: {damaged}"),
    )
}

fn found(root: &Path) -> Result<String, String> {
    let names: Vec<(String, String)> = slots(root)?
        .into_iter()
        .map(|(release, name, _)| (release, name))
        .collect();
    let expected = [
        ("v0.0.1".to_string(), "older".to_string()),
        ("v9.9.9".to_string(), "current".to_string()),
    ];
    check(names == expected, format!("{names:?}"))
}

fn decoded(world: &[u8]) -> Result<(), sim::DecodeError> {
    sim::Save::decode(world).map(|_| ())
}

fn current_passes(slots: &Slots) -> Result<String, String> {
    let read = decoded(&slots.current);
    let verdict = Verdict::of(read.as_ref().map(|_| ()), &Breaking::parse(FAKE), &[]);
    let problem = ["step went wrong".to_string()];
    let failing = Verdict::of(Ok(()), &Breaking::default(), &problem);
    check(
        verdict == Verdict::Pass && failing == Verdict::Fail("step went wrong".into()),
        format!("{verdict:?}, with a problem {failing:?}"),
    )
}

fn older_expected(slots: &Slots) -> Result<String, String> {
    let refused = decoded(&slots.older)
        .err()
        .ok_or("the older save decodes")?;
    let of = |text: &str| Verdict::of(Err(&refused), &Breaking::parse(text), &[]);
    let expected = of(FAKE);
    let none = of("");
    let not_before = of("- KJKV 1 (fake-bump): the version it is");
    // Expected only while the story restore that rescues it holds.
    let lost = ["story: lost items".to_string()];
    let unrescued = Verdict::of(Err(&refused), &Breaking::parse(FAKE), &lost);
    check(
        matches!(&expected, Verdict::Expected(why) if why.contains("KJKV 1 is before the breaking bump KJKV 60"))
            && none.failed()
            && not_before.failed()
            && unrescued.failed(),
        format!("{expected:?}; no entry {none:?}; entry not after it {not_before:?}"),
    )
}

fn table_prints(slots: &Slots) -> Result<String, String> {
    let refused = decoded(&slots.older)
        .err()
        .ok_or("the older save decodes")?;
    let row = |name: &str, release: &str, verdict| Row {
        name: name.into(),
        release: release.into(),
        formats: if name == "current" { "none" } else { "-" }.into(),
        stamps: usize::from(name == "current"),
        reconcile: "2/0/1/0/0".into(),
        story: "OK".into(),
        verdict,
    };
    let rows = [
        row("current", "v9.9.9", Verdict::Pass),
        row(
            "older",
            "v0.0.1",
            Verdict::of(Err(&refused), &Breaking::parse(FAKE), &[]),
        ),
    ];
    let text = table(&rows);
    let expected = "\
| save    | release | formats migrated | stamps | reconcile m/d/a/s/l | story | verdict  |
| ------- | ------- | ---------------- | ------ | ------------------- | ----- | -------- |
| current | v9.9.9  | none             | 1      | 2/0/1/0/0           | OK    | PASS     |
| older   | v0.0.1  | -                | 0      | 2/0/1/0/0           | OK    | EXPECTED |
EXPECTED: older: KJKV 1 is before the breaking bump KJKV 60
";
    let failing = [Row {
        verdict: Verdict::Fail("x".into()),
        ..rows[0].clone()
    }];
    check(
        text == expected
            && !rows.iter().any(|row| row.verdict.failed())
            && failing.iter().any(|row| row.verdict.failed()),
        text,
    )
}

fn slot_names(root: &Path, slots: &Slots) -> Result<String, String> {
    let slot = Slot {
        world: slots.current.clone(),
        meta: None,
        story: None,
    };
    let release = root.join("v9.9.9");
    let refused: Vec<bool> = ["", ".", "..", ".hidden", "a/b", "a\\b"]
        .iter()
        .map(|name| slot.write(&release, name).is_err())
        .collect();
    let kept = release.join("current").join(WORLD).exists()
        && root.join("v0.0.1").join("older").join(WORLD).exists();
    check(
        refused.iter().all(|refused| *refused) && kept,
        format!("refused {refused:?}, neighbours kept {kept}"),
    )
}

fn panic_is_a_row() -> Result<String, String> {
    let row = |name: &str| Row {
        name: name.into(),
        release: "v9.9.9".into(),
        formats: "none".into(),
        stamps: 0,
        reconcile: "-".into(),
        story: "OK".into(),
        verdict: Verdict::Pass,
    };
    let rows = [
        guarded("first", "v9.9.9", || row("first")),
        guarded("second", "v9.9.9", || panic!("the step panicked")),
        guarded("third", "v9.9.9", || row("third")),
    ];
    let text = table(&rows);
    check(
        rows.map(|row| row.verdict.to_string()) == ["PASS", "FAIL", "PASS"]
            && text.contains("FAIL: second: panicked: the step panicked"),
        text,
    )
}
