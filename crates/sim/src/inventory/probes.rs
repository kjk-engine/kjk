//! Item properties beyond combat (journal 2026-10-05-item-properties), on
//! items and sheets made here, no install: an item's best bonus of a kind
//! with its penalties; two items adding up on a sheet, held by the caps,
//! and gone when taken off; skills, saves, dodge defence and defence
//! against one race; a bonus feat; use limits (a side's, K1's and TSL's
//! "All", Proficiency All, a fitted crystal's, a race's, an ability's);
//! immunity to criticals and Keen; on-hit effects (a stun, an immune
//! target, death); regeneration every 6 s; a worn light as a lasting
//! visual; TSL's upgrade slots (fitted, its level refused, taken out); the
//! rows `GetItemHasItemProperty` reads; the encodings through their bytes.

use super::properties::{self, ALL_SAVES, Limit, OWN_TYPE, Property, Wearer};
use super::{COLOUR_SLOT, Catalog, ItemFacts, PARTY, dress, rewear, slots};
use crate::combat::damage::{self, Blow, FriendlyFire, Relation, Source};
use crate::combat::gear::{self, Against, OnHit, Roll, immunity, on_hit, sides, traits};
use crate::combat::sheet::{Ability, Rows, Weapon, damage_types, feats, races, skills};
use crate::combat::{Fighter, Location, Outcome, Sheet, effects};
use crate::probe::check;
use crate::rng::SimRng;
use crate::wire::{Reader, Writer};

type Checks = Vec<(&'static str, Result<String, String>)>;

/// An item of `properties` worn in `slot`, made of them.
fn item(template: &str, slot: u8, properties: Vec<Property>) -> ItemFacts {
    ItemFacts {
        slots: 1 << slot,
        properties,
        ..crate::world::containers::plain(template)
    }
    .made()
}

/// A blaster pistol of `properties`.
fn pistol(properties: Vec<Property>) -> ItemFacts {
    let bare = Some(Weapon::plain(
        12,
        1,
        6,
        (1, 2),
        damage_types::ENERGY,
        [feats::PROFICIENCY_BLASTER; 3],
    ));
    ItemFacts {
        base_item: 12,
        slots: 1 << slots::RIGHT_WEAPON,
        weapon: bare,
        bare,
        properties,
        ..crate::world::containers::plain("pistol")
    }
    .made()
}

fn ability(ability: Ability, by: i8) -> Property {
    Property::Ability {
        ability: ability as u8,
        by,
    }
}

/// The sheet everything reads with `worn` on.
fn wearing(worn: &[(u8, &ItemFacts)]) -> (Sheet, Sheet) {
    let sheet = dress(Sheet::dev_soldier(), worn.iter().copied());
    let now = effects::sheet_now(sheet, &Fighter::of(&sheet), 0);
    (sheet, now)
}

/// The best bonus of a kind, penalties adding; deflection adding up.
fn made() -> Checks {
    let made = pistol(vec![
        Property::Attack(2),
        Property::Enhancement(3),
        Property::Attack(-1),
        Property::Damage {
            row: OWN_TYPE,
            roll: Roll {
                bonus: 1,
                ..Roll::NONE
            },
        },
        Property::Damage {
            row: 8,
            roll: Roll {
                dice: 1,
                die: 6,
                bonus: 0,
            },
        },
        Property::Keen,
    ]);
    let weapon = made.weapon.unwrap_or(Weapon::UNARMED);
    let fire = weapon.extras[0].map(|extra| (extra.damage_type, extra.roll));
    vec![
        (
            "item_best_bonus_and_penalties",
            check(
                weapon.attack_bonus == 2 && weapon.damage_bonus == 3,
                format!(
                    "{} to hit, {} damage",
                    weapon.attack_bonus, weapon.damage_bonus
                ),
            ),
        ),
        (
            "item_other_type_extra",
            check(
                fire == Some((
                    8,
                    Roll {
                        dice: 1,
                        die: 6,
                        bonus: 0,
                    },
                )),
                format!("{:?}", weapon.extras),
            ),
        ),
        (
            "item_keen",
            check(weapon.crit_threat == 2, weapon.crit_threat.to_string()),
        ),
    ]
}

/// Scores, skills, saves, defence and a feat from two items; the caps;
/// taken off, all gone.
fn worn() -> Checks {
    let belt = item(
        "belt",
        slots::BELT,
        vec![
            ability(Ability::Strength, 2),
            ability(Ability::Strength, 1),
            Property::Skill {
                skill: skills::SECURITY as u8,
                by: 3,
            },
            Property::Save {
                save: ALL_SAVES,
                by: 1,
            },
            Property::Save { save: 0, by: 2 },
            Property::Defence {
                by: 2,
                against: None,
            },
            Property::Feat(feats::PROFICIENCY_ALL),
        ],
    );
    let band = item(
        "band",
        slots::LEFT_ARM,
        vec![
            ability(Ability::Strength, 2),
            Property::Defence {
                by: 1,
                against: None,
            },
            Property::Defence {
                by: 4,
                against: Some(Against::Race(races::DROID)),
            },
        ],
    );
    let mighty = item("mighty", slots::HEAD, vec![ability(Ability::Strength, 30)]);
    let base = Sheet::dev_soldier();
    let (sheet, now) = wearing(&[(slots::BELT, &belt), (slots::LEFT_ARM, &band)]);
    let (_, capped) = wearing(&[(slots::HEAD, &mighty)]);
    let off = rewear(
        sheet,
        &[(slots::BELT, &belt), (slots::LEFT_ARM, &band)],
        &[],
    );
    let off_now = effects::sheet_now(off, &Fighter::of(&off), 0);
    let mut droid = Sheet::dev_soldier();
    droid.race = races::DROID;
    let against = |foe: &Sheet| now.gear.defence_against(foe, damage_types::ENERGY);
    vec![
        (
            "worn_scores_add",
            check(
                now.abilities[0] == base.abilities[0] + 4 && sheet.abilities == base.abilities,
                format!("{:?} of {:?}", now.abilities, base.abilities),
            ),
        ),
        (
            "worn_skills_saves_defence",
            check(
                now.skills[skills::SECURITY] == base.skills[skills::SECURITY] + 3
                    && now.saves == [base.saves[0] + 3, base.saves[1] + 1, base.saves[2] + 1]
                    && now.defence == base.defence + 3,
                format!("{:?} {:?} {}", now.skills, now.saves, now.defence),
            ),
        ),
        (
            "worn_defence_against_race",
            check(
                against(&droid) == 4 && against(&base) == 0,
                format!("{} {}", against(&droid), against(&base)),
            ),
        ),
        (
            "worn_bonus_feat",
            check(
                now.feats.has(feats::PROFICIENCY_ALL) && !sheet.feats.has(feats::PROFICIENCY_ALL),
                "Proficiency All".into(),
            ),
        ),
        (
            "worn_capped",
            check(
                capped.abilities[0] == base.abilities[0] + 20,
                format!("{}", capped.abilities[0]),
            ),
        ),
        (
            "worn_taken_off",
            check(
                off_now.abilities == base.abilities
                    && off_now.defence == base.defence
                    && off.gear == gear::Gear::default(),
                format!("{:?} {}", off_now.abilities, off_now.defence),
            ),
        ),
    ]
}

/// Who may wear what.
fn limits() -> Checks {
    let wearer = |good_evil: u8, tsl: bool| Wearer {
        good_evil,
        classes: [3, u8::MAX],
        race: races::HUMAN,
        subrace: 0,
        feats: Rows::default(),
        gender: 1,
        abilities: [12; 6],
        player: true,
        tag: String::new(),
        tsl,
    };
    let light_only = [Limit::Side(sides::LIGHT)];
    let all = [Limit::Side(sides::ALL)];
    let dark = wearer(20, false);
    let master = Wearer {
        feats: Rows::default().with(feats::PROFICIENCY_ALL),
        ..dark.clone()
    };
    // A saber whose crystal of row 24 adds a light-side limit, fitted.
    let saber = ItemFacts {
        upgrades: vec![properties::Upgrade {
            row: 24,
            kind: 0,
            template: "solari".into(),
            properties: vec![Property::Limit(Limit::Side(sides::LIGHT))],
            rows: 1 << 43,
        }],
        ..pistol(Vec::new())
    };
    let catalog = Catalog::new([saber.clone()]);
    let bare = saber.fitted(0, &[], &catalog);
    let fitted = saber.fitted(1 << 24, &[], &catalog);
    // Equipped as a player would: refused to the dark wearer, crystal fitted.
    let mut state = crate::script::State::default();
    let held = state
        .make_item(&catalog, "pistol", PARTY, 1, None)
        .map_or(0, |id| id.0);
    if let Some(mut item) = state.items.get_mut(&held) {
        item.upgrades = 1 << 24;
    }
    let equip = crate::action::Action::Equip {
        item: held,
        slot: slots::RIGHT_WEAPON,
    };
    let mut out = crate::script::Out::default();
    let refused = state.wear(&catalog, &mut out, (1, &dark), equip.clone());
    let worn = state.wear(&catalog, &mut out, (1, &wearer(80, false)), equip);
    // `SetItemNonEquippable`: refused to anyone, then put on once cleared.
    let barred = state
        .make_item(&catalog, "pistol", PARTY, 1, None)
        .map_or(0, |id| id.0);
    let set_barred = |state: &mut crate::script::State, on: bool| {
        if let Some(mut item) = state.items.get_mut(&barred) {
            item.non_equippable = on;
        }
    };
    let put_on = crate::action::Action::Equip {
        item: barred,
        slot: slots::RIGHT_WEAPON,
    };
    set_barred(&mut state, true);
    let light = wearer(80, false);
    let unequippable = state.wear(&catalog, &mut out, (2, &light), put_on.clone());
    set_barred(&mut state, false);
    let equippable = state.wear(&catalog, &mut out, (2, &light), put_on);
    // An item for humans only (`droidorhuman` 1): refused to a droid (T3-M4
    // and a forearm band), worn by a human.
    let humans = Catalog::new([ItemFacts {
        droids: Some(false),
        ..pistol(Vec::new())
    }]);
    let mut kept = crate::script::State::default();
    let band = kept
        .make_item(&humans, "pistol", PARTY, 2, None)
        .map_or(0, |id| id.0);
    let band_on = crate::action::Action::Equip {
        item: band,
        slot: slots::RIGHT_WEAPON,
    };
    let droid = Wearer {
        race: races::DROID,
        ..light.clone()
    };
    let on_droid = kept.wear(&humans, &mut out, (3, &droid), band_on.clone());
    let on_human = kept.wear(&humans, &mut out, (4, &light), band_on);
    let race = [Limit::Race(races::DROID)];
    let strong = [Limit::Ability {
        ability: 0,
        at_least: 14,
    }];
    let may = |limits: &[Limit], who: &Wearer| properties::may_wear(limits, who);
    vec![
        (
            "non_equippable_refused",
            check(
                unequippable == Err(crate::notice::Notice::CannotEquip) && equippable.is_ok(),
                format!("barred {unequippable:?}, cleared {equippable:?}"),
            ),
        ),
        (
            "droid_or_human_refused",
            check(
                on_droid == Err(crate::notice::Notice::CannotEquip) && on_human.is_ok(),
                format!("droid {on_droid:?}, human {on_human:?}"),
            ),
        ),
        (
            "limit_side",
            check(
                may(&light_only, &dark) == Err(properties::refusals::SIDE)
                    && may(&light_only, &wearer(80, false)).is_ok(),
                format!("{:?}", may(&light_only, &dark)),
            ),
        ),
        (
            "limit_all_k1_and_tsl",
            check(
                may(&all, &dark).is_err() && may(&all, &wearer(20, true)).is_ok(),
                "K1's All never, TSL's always".into(),
            ),
        ),
        (
            "limit_proficiency_all",
            check(may(&light_only, &master).is_ok(), "waived".into()),
        ),
        (
            "limit_fitted_crystal",
            check(
                may(&bare.limits, &dark).is_ok()
                    && may(&fitted.limits, &dark).is_err()
                    && fitted.has_row(43)
                    && !bare.has_row(43),
                format!("{:?} {:?}", bare.limits, fitted.limits),
            ),
        ),
        (
            "limit_fitted_crystal_equip",
            check(
                refused == Err(crate::notice::Notice::Line(properties::refusals::SIDE))
                    && worn.is_ok(),
                format!("{refused:?} then {worn:?}"),
            ),
        ),
        (
            "limit_race_and_ability",
            check(
                may(&race, &dark) == Err(properties::refusals::RACE)
                    && may(&strong, &dark) == Err(properties::refusals::ABILITY),
                "refused".into(),
            ),
        ),
    ]
}

/// A hit's crit against a target immune to criticals; on-hit effects.
fn hits() -> Checks {
    let always = Weapon {
        crit_threat: 20,
        ..pistol(Vec::new()).weapon.unwrap_or(Weapon::UNARMED)
    };
    let blow = Blow {
        attacker: None,
        relation: Relation::Hostile,
        source: Source::Weapon {
            weapon: always,
            cadence_ms: 1000,
            share: 1000,
            area: false,
        },
        location: Location::None,
        direction: None,
        sneak: 0,
    };
    let target = Sheet::dev_soldier();
    let mut immune = target;
    immune.gear.immune = 1 << immunity::CRITICAL_HIT;
    let mut rng = SimRng::new(3, 1);
    let mut outcome = |sheet: &Sheet| {
        damage::assess(
            &blow,
            None,
            sheet,
            FriendlyFire::Off,
            crate::combat::passive::Game::K1,
            &mut rng,
        )
        .outcome
    };
    let (plain, guarded) = (outcome(&target), outcome(&immune));
    let stunning = Weapon {
        on_hit: [
            Some(OnHit {
                kind: on_hit::STUN,
                chance: 100,
                seconds: 6,
                dc: 100,
                param: 0,
            }),
            None,
        ],
        ..always
    };
    let deadly = Weapon {
        on_hit: [
            Some(OnHit {
                kind: on_hit::DEATH,
                chance: 0,
                seconds: 0,
                dc: 0,
                param: 0,
            }),
            None,
        ],
        ..always
    };
    let fighter = Fighter::of(&target);
    let mut mindless = fighter;
    mindless.effects.worn_immune = 1 << immunity::MIND_SPELLS;
    let struck = |weapon: &Weapon, fighter: &Fighter, rng: &mut SimRng| {
        gear::on_hits(weapon, &target, fighter, 0, rng)
    };
    let stunned = struck(&stunning, &fighter, &mut rng);
    let kept_off = struck(&stunning, &mindless, &mut rng);
    let killed = struck(&deadly, &fighter, &mut rng);
    vec![
        (
            "immune_to_criticals",
            check(
                plain == Outcome::Critical && guarded == Outcome::Hit,
                format!("{plain:?} then {guarded:?}"),
            ),
        ),
        (
            "on_hit_stun",
            check(
                stunned
                    == vec![gear::Struck::Hold {
                        ms: 6000,
                        fear: false,
                    }]
                    && kept_off.is_empty(),
                format!("{stunned:?} {kept_off:?}"),
            ),
        ),
        (
            "on_hit_death",
            check(killed == vec![gear::Struck::Death], format!("{killed:?}")),
        ),
    ]
}

/// Regeneration every 6 s, not at the maximum; a light lasting.
fn over_time() -> Checks {
    let pendant = item(
        "pendant",
        slots::HEAD,
        vec![Property::Regenerate(2), Property::Trait(traits::LIGHT)],
    );
    let (_, now) = wearing(&[(slots::HEAD, &pendant)]);
    let mut fighter = Fighter::of(&now);
    let most = fighter.hp;
    fighter.hp -= 1000;
    let pool = now.pool();
    let mut hp = Vec::new();
    for at in [6000, 9000, 12_000] {
        effects::worn(&mut fighter, &now, pool, (at, at / 50));
        hp.push(fighter.hp);
    }
    let lit = fighter
        .effects
        .visuals(12_000)
        .any(|(row, ..)| row == gear::LIGHT_VISUAL);
    let event = fighter
        .newest()
        .any(|event| event.visual.is_some_and(|v| v.row == gear::LIGHT_VISUAL));
    let off = Sheet::dev_soldier();
    effects::worn(&mut fighter, &off, pool, (13_000, 260));
    let dark = !fighter
        .effects
        .visuals(13_000)
        .any(|(row, ..)| row == gear::LIGHT_VISUAL);
    vec![
        (
            "regenerates_every_6s",
            check(
                hp == [most - 800, most - 800, most - 600],
                format!("{hp:?} of {most}"),
            ),
        ),
        (
            "light_lasts_while_worn",
            check(lit && event && dark, format!("{lit} {event} {dark}")),
        ),
    ]
}

/// TSL's slots: a lens fitted adds its to hit, one of a higher upgrade
/// level is refused, taken out it is back with the party.
fn slots_fitted() -> Checks {
    let host = ItemFacts {
        upgrade_level: 1,
        slot_kinds: vec![0, COLOUR_SLOT, 0, 1, 2, 3],
        ..pistol(Vec::new())
    };
    let lens = |template: &str, level: u8, bonus: i8| ItemFacts {
        upgrade: Some((100, 1)),
        upgrade_level: level,
        properties: vec![Property::Attack(bonus)],
        rows: 1 << 38,
        ..crate::world::containers::plain(template)
    };
    let catalog = Catalog::new([host, lens("lens", 1, 2), lens("lens_rare", 2, 4)]);
    let mut state = crate::script::State::default();
    let mut out = crate::script::Out::default();
    let made = |state: &mut crate::script::State, template: &str| {
        state
            .make_item(&catalog, template, PARTY, 1, None)
            .map_or(0, |id| id.0)
    };
    let (saber, part, rare) = (
        made(&mut state, "pistol"),
        made(&mut state, "lens"),
        made(&mut state, "lens_rare"),
    );
    let fitted = state.fit(&catalog, &mut out, saber, part, None);
    let held = state.items.get(&saber).cloned();
    let attack = held.as_ref().and_then(|held| {
        let facts = catalog.get("pistol")?;
        facts
            .fitted(held.upgrades, &held.fitted, &catalog)
            .weapon
            .map(|weapon| {
                (
                    weapon.attack_bonus,
                    facts.fitted(0, &held.fitted, &catalog).has_row(38),
                )
            })
    });
    let refused = state.fit(&catalog, &mut out, saber, rare, None);
    let out_again = state.unfit(&catalog, &mut out, saber, 3);
    let back = state
        .carried(PARTY)
        .iter()
        .filter_map(|id| state.items.get(&id.0).map(|item| item.template.clone()))
        .collect::<Vec<_>>();
    vec![
        (
            "tsl_slot_fitted",
            check(
                fitted.is_ok()
                    && held.is_some_and(|held| held.fitted.get(3).is_some_and(|t| t == "lens"))
                    && attack == Some((2, true)),
                format!("{fitted:?} {attack:?}"),
            ),
        ),
        (
            "tsl_slot_level_refused",
            check(refused.is_err(), format!("{refused:?}")),
        ),
        (
            "tsl_slot_taken_out",
            check(
                out_again.is_ok() && back.contains(&"lens".to_string()),
                format!("{out_again:?} {back:?}"),
            ),
        ),
    ]
}

/// Properties, gear and sheets through their bytes.
fn encodings() -> Checks {
    let all = vec![
        Property::Enhancement(2),
        Property::Versus(gear::Versus {
            against: Against::Side(sides::DARK),
            attack: 1,
            damage: Roll {
                dice: 2,
                die: 6,
                bonus: 0,
            },
            damage_type: 9,
        }),
        Property::Soak {
            points: 5,
            bypass: 2,
        },
        Property::Defence {
            by: 2,
            against: Some(Against::Type(1)),
        },
        Property::OnHit(OnHit {
            kind: on_hit::POISON,
            chance: 0,
            seconds: 0,
            dc: 14,
            param: 3,
        }),
        Property::Disguise(46),
        Property::Limit(Limit::Tag("Atton".into())),
        Property::Limit(Limit::Ability {
            ability: 2,
            at_least: 13,
        }),
    ];
    let mut out = Writer::default();
    for property in &all {
        property.encode(&mut out);
    }
    let mut input = Reader::new("properties", &out.0);
    let back: Result<Vec<Property>, _> = all.iter().map(|_| Property::decode(&mut input)).collect();
    let (_, now) = wearing(&[(slots::BELT, &item("belt", slots::BELT, all.clone()))]);
    let mut sheet_bytes = Writer::default();
    now.encode(&mut sheet_bytes);
    let sheet_back = Sheet::decode(&mut Reader::new("sheet", &sheet_bytes.0));
    vec![
        (
            "properties_round_trip",
            check(back.as_ref() == Ok(&all), format!("{back:?}")),
        ),
        (
            "sheet_gear_round_trip",
            check(sheet_back.as_ref() == Ok(&now), format!("{:?}", now.gear)),
        ),
    ]
}

/// Every check (see the module documentation).
pub fn probes() -> Checks {
    let mut all = made();
    all.extend(worn());
    all.extend(limits());
    all.extend(hits());
    all.extend(over_time());
    all.extend(slots_fitted());
    all.extend(encodings());
    all
}
