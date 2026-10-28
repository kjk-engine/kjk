//! `kjk script impacts <game>` (I22 C10): what K1's Force powers do to a
//! target, read off the game's own impact script by running it. Every K1
//! power's `impactscript` is `k_sp1_generic`; it is run once per
//! `spells.2da` row on a recording host (`Probe`): a caster of a given
//! level and ability modifier casts at one enemy that fails every save and
//! is not resisted, and every save (with its DC), dice roll, effect made
//! and effect applied (with its duration) is written down, the code it
//! defers too. Two runs at different levels and modifiers show the
//! formulas (`C10`'s stand-ins in `combat::effects` are checked against
//! them). Nothing of the script is kept: the listing goes to the journal.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::process::ExitCode;
use std::sync::Arc;

use fmt_odyssey::ResType;
use fmt_odyssey::ncs::Ncs;
use fmt_odyssey::twoda::TwoDa;
use script_ncs::vm::{self, Deferred, Handle, ObjectId, Program, Routines, Value};
use script_ncs::{Catalog, Routine};
use serde_json::json;
use vfs::Discovery;

use crate::script_verbs::{catalog, ncs_type, open};

const CASTER: ObjectId = ObjectId(2);
const TARGET: ObjectId = ObjectId(3);
/// The most deferred blocks one cast goes on to.
const DEFERRED: usize = 32;
/// What the engine's own numbers (`GetSpellSaveDC`) answer: easy to spot.
const SENTINEL: i32 = 900;

/// The caster and what the probe answers.
#[derive(Clone, Copy)]
pub struct Caster {
    pub level: i32,
    pub modifier: i32,
}

/// A recording host: answers as `Caster` says, writes down what matters.
struct Probe<'a> {
    catalog: &'a Catalog,
    caster: Caster,
    row: i32,
    /// Effects made so far, by handle (from 1).
    effects: Vec<String>,
    deferred: Vec<Deferred>,
    lines: Vec<String>,
    /// Routines answered with nothing, by name.
    unanswered: std::collections::BTreeSet<String>,
}

fn int(args: &[Value], at: usize) -> i32 {
    match args.get(at) {
        Some(Value::Int(v)) => *v,
        _ => 0,
    }
}

impl Probe<'_> {
    fn constant(&self, name: &str) -> i32 {
        match self.catalog.constant_named(name).map(|c| &c.value) {
            Some(script_ncs::catalog::Value::Int(v)) => *v,
            _ => 0,
        }
    }

    fn who(id: ObjectId) -> &'static str {
        match id {
            CASTER => "caster",
            TARGET => "target",
            _ => "?",
        }
    }

    fn show(&self, value: &Value) -> String {
        match value {
            Value::Int(v) => v.to_string(),
            Value::Float(v) => format!("{v}"),
            Value::String(text) => format!("{text:?}"),
            Value::Object(id) => Probe::who(*id).to_string(),
            Value::Effect(Handle(n)) => self.effect(*n),
            other => format!("{other:?}"),
        }
    }

    fn effect(&self, n: u32) -> String {
        usize::try_from(n)
            .ok()
            .and_then(|n| self.effects.get(n.checked_sub(1)?))
            .cloned()
            .unwrap_or_else(|| "no effect".into())
    }

    fn call(&self, routine: &Routine, args: &[Value]) -> String {
        let shown: Vec<String> = args.iter().map(|arg| self.show(arg)).collect();
        format!("{}({})", routine.name, shown.join(", "))
    }
}

impl vm::Host for Probe<'_> {
    fn action(
        &mut self,
        routine: &Routine,
        _caller: ObjectId,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let name = routine.name.as_str();
        let empty = Value::empty(routine.ret);
        Ok(match name {
            "GetSpellId" => Value::Int(self.row),
            "GetSpellTargetObject" | "GetNearestCreature" | "GetFirstObjectInShape" => {
                Value::Object(TARGET)
            }
            "GetNextObjectInShape" | "GetPartyMemberByIndex" => Value::Object(ObjectId::INVALID),
            // The target is a level-1 creature.
            "GetHitDice" => Value::Int(match args.first() {
                Some(Value::Object(TARGET)) => 1,
                _ => self.caster.level,
            }),
            "GetAbilityModifier" => Value::Int(self.caster.modifier),
            "GetIsEnemy" => Value::Int(1),
            "GetIsObjectValid" => match args.first() {
                Some(Value::Object(id)) => Value::Int(i32::from(*id == CASTER || *id == TARGET)),
                _ => Value::Int(0),
            },
            "GetRacialType" => Value::Int(self.constant("RACIAL_TYPE_HUMAN")),
            "GetCurrentHitPoints" | "GetMaxHitPoints" => Value::Int(100),
            "GetCreatureSize" => Value::Int(self.constant("CREATURE_SIZE_MEDIUM")),
            "WillSave" | "ReflexSave" | "FortitudeSave" | "ResistForce" => {
                self.lines.push(format!("  {}", self.call(routine, &args)));
                Value::Int(0)
            }
            "DelayCommand" => {
                if let Some(Value::Action(code)) = args.get(1) {
                    self.lines
                        .push(format!("  DelayCommand({}, ...)", self.show(&args[0])));
                    self.deferred.push((**code).clone());
                }
                empty
            }
            // The engine's numbers: marked, to show where the script asks.
            "GetSpellSaveDC" | "GetCasterLevel" => {
                self.lines.push(format!("  {name}() = {SENTINEL}"));
                Value::Int(SENTINEL)
            }
            // Wrappers hand the effect on.
            "SetEffectIcon" | "ExtraordinaryEffect" | "SupernaturalEffect" | "MagicalEffect" => {
                args.first().cloned().unwrap_or(empty)
            }
            "ApplyEffectToObject" => {
                self.lines.push(format!("  {}", self.call(routine, &args)));
                empty
            }
            _ if name.starts_with("Effect") => {
                let made = if name == "EffectLinkEffects" {
                    format!("{} + {}", self.show(&args[0]), self.show(&args[1]))
                } else {
                    self.call(routine, &args)
                };
                self.effects.push(made);
                Value::Effect(Handle(self.effects.len() as u32))
            }
            _ if name.len() > 1
                && name.starts_with('d')
                && name[1..].bytes().all(|b| b.is_ascii_digit()) =>
            {
                let n = int(&args, 0).max(1);
                self.lines.push(format!("  {n}{name}"));
                // Each die's least: the formula shows as a count.
                Value::Int(n)
            }
            _ => {
                self.unanswered.insert(name.to_string());
                empty
            }
        })
    }
}

/// `spells.2da`'s rows whose impact script is `script`: row and label.
fn powers(resolver: &vfs::resolver::Resolver, script: &str) -> Result<Vec<(i32, String)>, String> {
    let found = resolver
        .find(
            "spells",
            ResType::TWO_DA,
        )
        .ok_or("spells.2da: not in the install")?;
    let bytes = found.location.read().map_err(|error| error.to_string())?;
    let table = TwoDa::parse(&bytes).map_err(|error| error.to_string())?;
    let column = |name: &str| table.column(name).ok_or(format!("spells.2da: no {name}"));
    let (label, impact) = (column("label")?, column("impactscript")?);
    let text = |row: usize, column: usize| {
        table
            .get(row, column)
            .map(|cell| String::from_utf8_lossy(cell).trim().to_string())
            .unwrap_or_else(String::new)
    };
    Ok((0..table.cells.len())
        .filter(|&row| text(row, impact).eq_ignore_ascii_case(script))
        .map(|row| (row as i32, text(row, label)))
        .collect())
}

/// Each power run through `k_sp1_generic` for `casters`, what it did.
pub fn impacts(discovery: &Discovery, game: &str, casters: &[Caster]) -> ExitCode {
    let run = || -> Result<serde_json::Value, String> {
        let (game, _, base) = open(discovery, game)?;
        let catalog = Arc::new(catalog(&base)?);
        let routines = Routines::new(catalog.clone());
        let script = "k_sp1_generic";
        let found = base
            .find(script, ncs_type())
            .ok_or(format!("{script}.ncs: not found"))?;
        let bytes = found.location.read().map_err(|error| error.to_string())?;
        let program = Program::new(Ncs::parse(&bytes).map_err(|error| error.to_string())?);
        let rows = powers(&base, script)?;
        let mut out = BTreeMap::new();
        let mut faults = 0;
        for (row, label) in &rows {
            let mut report = String::new();
            for caster in casters {
                let mut probe = Probe {
                    catalog: &catalog,
                    caster: *caster,
                    row: *row,
                    effects: Vec::new(),
                    deferred: Vec::new(),
                    lines: Vec::new(),
                    unanswered: std::collections::BTreeSet::new(),
                };
                let mut outcome = vm::run(&program, &routines, CASTER, &mut probe).map(|_| ());
                let mut done = 0;
                while outcome.is_ok() && done < DEFERRED {
                    let Some(code) = probe.deferred.pop() else {
                        break;
                    };
                    done += 1;
                    outcome =
                        vm::resume(&program, &routines, CASTER, &code, &mut probe).map(|_| ());
                }
                if let Err(fault) = outcome {
                    faults += 1;
                    probe.lines.push(format!("  fault: {fault}"));
                }
                let _ = writeln!(
                    report,
                    " level {}, modifier {}:",
                    caster.level, caster.modifier
                );
                for line in &probe.lines {
                    let _ = writeln!(report, "  {line}");
                }
                if !probe.unanswered.is_empty() {
                    let names: Vec<&str> = probe.unanswered.iter().map(String::as_str).collect();
                    let _ = writeln!(report, "    (answered with nothing: {})", names.join(", "));
                }
            }
            outln!("{row} {label}\n{report}");
            out.insert(format!("{row} {label}"), report);
        }
        outln!(
            "{}: {} powers through {script}, {faults} faults",
            game.key(),
            rows.len()
        );
        Ok(json!({ "game": game.key(), "powers": out, "faults": faults }))
    };
    match run() {
        Ok(facts) => {
            crate::collision_verbs::write_facts(facts);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
