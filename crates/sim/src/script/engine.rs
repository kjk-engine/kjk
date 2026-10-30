//! The engine routines scripts call (`Engine`, a `Host` for the VM): the
//! bound ones over the step's `View` and the scripts' `State`; every other
//! one a counted gap.

use std::cmp::Ordering;
use std::sync::Arc;

use script_ncs::catalog::Routine;
use script_ncs::vm::{self, Deferred, Handle, Host, ObjectId, Value};

use super::actions::{Action, Target, Use};
use super::effects::{self, Applied, Effect, Hold};
use super::items;
use super::party;
use super::state::{SPELL_EVENT, global, set_global};
use super::{
    AREA, Change, Character, Criterion, Event, HUNDREDTHS, Hook, Library, MODULE, MOST_NESTED, Out,
    Pending, Report, State, Thing, View, finish, flat, key, name, nearest_first, nth_nearest,
    perception, tagged, twice_average,
};
use crate::combat::effects::{ALL_SAVES, Lasting, Modifies, POISON_ID, state};
use crate::combat::sheet::{Rows, flags};
use crate::inventory;
use crate::module::{Global, ModuleData, ObjectKind};
use crate::notice::{Notice, Screen};
use crate::rng::SimRng;
use crate::snapshot::{Departure, Going, StatusKind};

/// The routines bound, each by its `nwscript.nss` name; `Bound::named`
/// maps a name to its binding.
macro_rules! bound {
    ($($routine:ident),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(super) enum Bound {
            $($routine,)*
            /// `d2` to `d100`: so many sides.
            Dice(u32),
            Abs,
            Fabs,
            /// `PrintString`, `AurPostString`: traced.
            Print,
        }

        impl Bound {
            pub(super) fn named(name: &str) -> Option<Bound> {
                Some(match name {
                    $(stringify!($routine) => Bound::$routine,)*
                    "d2" => Bound::Dice(2),
                    "d3" => Bound::Dice(3),
                    "d4" => Bound::Dice(4),
                    "d6" => Bound::Dice(6),
                    "d8" => Bound::Dice(8),
                    "d10" => Bound::Dice(10),
                    "d12" => Bound::Dice(12),
                    "d20" => Bound::Dice(20),
                    "d100" => Bound::Dice(100),
                    "abs" => Bound::Abs,
                    "fabs" => Bound::Fabs,
                    "PrintString" | "AurPostString" => Bound::Print,
                    _ => return None,
                })
            }
        }
    };
}

bound! {
    GetGlobalNumber, SetGlobalNumber, GetGlobalBoolean, SetGlobalBoolean, GetGlobalLocation,
    SetGlobalLocation,
    GetGlobalString, SetGlobalString, GetLocalBoolean, SetLocalBoolean,
    GetLocalNumber, SetLocalNumber,
    GetObjectByTag, GetWaypointByTag, GetNearestObjectByTag, GetTag,
    GetIsObjectValid, GetFirstPC, GetPartyMemberByIndex, GetIsPC, GetModule,
    GetArea, GetObjectType, GetIsDead, GetIsOpen, GetLocked, SetLocked,
    GetPosition, GetFacing, GetLocation, Location, GetPositionFromLocation,
    GetFacingFromLocation, GetDistanceBetween, GetDistanceToObject,
    GetDistanceBetween2D, GetDistanceToObject2D, GetDistanceBetweenLocations,
    GetCurrentHitPoints, GetMaxHitPoints,
    GetEnteringObject, GetExitingObject, GetModuleItemAcquired, GetModuleItemLost, GetClickingObject,
    GetLastOpenedBy,
    GetLastUsedBy, GetLastClosedBy,
    GetLastDisturbed,
    GetLastKiller, GetLastDamager, GetRunScriptVar, GetUserDefinedEventNumber,
    GetLastPerceived, GetLastPerceptionSeen, GetLastPerceptionVanished,
    GetLastPerceptionHeard, GetLastPerceptionInaudible, GetObjectSeen, GetObjectHeard,
    GetAttackTarget, GetAttemptedAttackTarget, GetNearestCreature, GetIsEnemy, GetIsFriend,
    GetIsNeutral, GetStandardFaction, GetRacialType, GetHitDice, GetHasSpell,
    IsObjectPartyMember, FindSubString, ActionAttack, GetIsInCombat, GetLastAttacker,
    TalentSpell, TalentFeat, TalentSkill, GetIsTalentValid, GetTypeFromTalent, GetIdFromTalent,
    GetCategoryFromTalent, GetCreatureHasTalent, GetCreatureTalentBest, GetCreatureTalentRandom,
    ActionUseTalentOnObject, ActionUseTalentAtLocation, GetLastHostileTarget, GetIsDebilitated,
    GetLevelByClass, GetClassByPosition, GetLastSpellCaster, GetLastSpell, GetLastSpellHarmful,
    EffectDamage, EffectHeal, EffectDeath, EffectHealForcePoints, EffectDamageForcePoints,
    EffectLinkEffects, SetEffectIcon, ApplyEffectToObject, ApplyEffectAtLocation, CutsceneAttack,
    SetListening, GetIsListening, SetListenPattern, GetListenPatternNumber, GetLastSpeaker,
    SpeakString, ActionSpeakString, GetSubRace, GetGender, GetIsEncounterCreature,
    GetAttemptedSpellTarget, GetAttemptedMovementTarget, GetFirstObjectInArea,
    GetNextObjectInArea, GetFirstInPersistentObject, GetNextInPersistentObject,
    GetFirstObjectInShape, GetNextObjectInShape, GetNearestObject, ActionRandomWalk, ShipBuild,
    EffectAbilityIncrease, EffectAbilityDecrease, EffectACIncrease, EffectACDecrease,
    EffectSavingThrowIncrease, EffectSavingThrowDecrease, EffectAttackIncrease,
    EffectAttackDecrease, EffectDamageIncrease, EffectDamageDecrease, EffectDamageResistance,
    EffectDamageImmunityIncrease, EffectDamageImmunityDecrease, EffectImmunity,
    EffectMovementSpeedIncrease, EffectMovementSpeedDecrease, EffectModifyAttacks,
    EffectTemporaryForcePoints, EffectTemporaryHitpoints, EffectForceShield, EffectResurrection,
    EffectEntangle, EffectPoison,
    EffectSkillIncrease, EffectSkillDecrease, EffectAssuredHit, EffectForceResistanceIncrease,
    EffectStunned, EffectParalyze, EffectHorrified, EffectDroidStun, EffectChoke, EffectSleep,
    EffectWhirlWind, EffectDisguise, EffectVisualEffect, EffectBeam, GetFirstEffect, GetNextEffect,
    GetIsEffectValid, GetEffectType, GetEffectCreator, GetEffectSpellId, RemoveEffect, RemoveEffectByID,
    ClearAllEffects, GetHasSpellEffect, GetSpellId, GetSpellTargetObject,
    GetSpellTargetLocation, GetSpellSaveDC, GetCasterLevel, ResistForce, WillSave, FortitudeSave,
    ReflexSave, GetAbilityScore, GetAbilityModifier, GetIsLinkImmune, EventSpellCastAt, EffectForceFizzle,
    EffectForceResisted, EffectForcePushed, EffectForcePushTargeted, EffectLightsaberThrow,
    SoundObjectPlay, SoundObjectStop, SoundObjectFadeAndStop, SoundObjectSetVolume,
    SoundObjectSetFixedVariance,
    SoundObjectGetVolume, MusicBackgroundPlay, MusicBackgroundStop, MusicBackgroundChangeDay,
    MusicBackgroundChangeNight, MusicBackgroundGetDayTrack, MusicBackgroundGetNightTrack,
    MusicBattlePlay, MusicBattleStop, MusicBattleChange, MusicBackgroundGetBattleTrack,
    AmbientSoundPlay, AmbientSoundStop, AmbientSoundChangeDay, AmbientSoundChangeNight,
    AmbientSoundSetDayVolume, AmbientSoundSetNightVolume,
    GetLoadFromSaveGame,
    CreateItemOnObject, CreateItemOnFloor, GetItemInSlot, GetFirstItemInInventory,
    GetNextItemInInventory, GetItemPossessedBy, GetItemPossessor, GetBaseItemType,
    GetItemStackSize, GetNumStackedItems, SetItemStackSize, GetItemHasItemProperty,
    GetItemCharges, SetItemCharges, DestroyObject, ActionEquipItem, ActionUnequipItem,
    ActionGiveItem, ActionTakeItem, GetGold, GiveGoldToCreature, TakeGoldFromCreature,
    GetPlotFlag, SetPlotFlag, GetIdentified, SetIdentified, SetItemNonEquippable,
    AddAvailableNPCByObject, AddAvailableNPCByTemplate, RemoveAvailableNPC, IsAvailableCreature,
    AddPartyMember, RemovePartyMember, IsNPCPartyMember, GetPartyMemberCount, SetPartyLeader,
    GetNPCAIStyle, SetNPCAIStyle, GetPartyAIStyle, SetPartyAIStyle, GetSoloMode, SetSoloMode,
    GetPlayerRestrictMode, SetPlayerRestrictMode, GetNPCSelectability, SetNPCSelectability,
    SpawnAvailableNPC, SaveNPCState, SwitchPlayerCharacter, CreateObject,
    DelayCommand, AssignCommand, ExecuteScript, SignalEvent, EventUserDefined,
    ActionDoCommand, ActionWait, ActionMoveToObject, ActionMoveToLocation,
    ActionForceMoveToObject, ActionForceMoveToLocation, ActionJumpToObject,
    ActionJumpToLocation, JumpToObject, JumpToLocation, ActionOpenDoor,
    ActionCloseDoor, YavinHackCloseDoor, ActionUseObject, ActionInteractObject, ActionPlayAnimation, ActionStartConversation, BeginConversation,
    SpeakOneLinerConversation, BarkString, GetIsInConversation, GetIsConversationActive,
    GetPCSpeaker, ActionPauseConversation, ActionResumeConversation, SetCustomToken,
    SetDialogPlaceableCamera, SetLockOrientationInDialog, SetLockHeadFollowInDialog,
    GetScriptParameter, GetScriptStringParameter,
    ClearAllActions, GetCurrentAction, SetCommandable, GetCommandable,
    SetFacing, SetFacingPoint,
    Random, IntToString, FloatToString, IntToFloat, FloatToInt, StringToInt,
    GetStringLength, GetSubString, GetStringLeft, GetStringRight,
    GetModuleName, GetTimeHour, GetTimeMinute, GetTimeSecond, GetTimeMillisecond, EndGame,
    StartCreditSequence, IsCreditSequenceInProgress, PlayRoomAnimation,
    GetObjectPersonalSpace, IncrementGlobalNumber, DecrementGlobalNumber,
    RemoveHeartbeat, GetStringByStrRef, GetLastHostileActor, GetHasFeat, GetFeatAcquired,
    GrantFeat, GetHasSkill,
    GetSkillRank,
    GetIsPartyLeader, GetPartyLeader, GetIsPlayerMadeCharacter, GetNextPC, GetUserActionsPending,
    ObjectToString, PlayAnimation, GetLevelByPosition, Vector, VectorNormalize, VectorMagnitude,
    SetHealTarget, GetHealTarget, GetCurrentForcePoints, GetMaxForcePoints, GiveItem,
    GetSpellForcePointCost, ChangeToStandardFaction, ChangeFactionByFaction, CancelCombat, SetLightsaberPowered, SurrenderToEnemies, SurrenderRetainBuffs, SurrenderByFaction, SetMinOneHP, GetMinOneHP,
    GetGoodEvilValue, GetAlignmentGoodEvil, AdjustAlignment, SetGoodEvilValue,
    GetName, GetReputation, AdjustReputation, SetIsDestroyable, GetFactionEqual,
    GetFactionAverageReputation, GetModuleFileName, GetIsLiveContentAvailable, GetIsXBox,
    PlayRumblePattern, StopRumblePattern, SetForceAlwaysUpdate, SetCreatureAILevel,
    ResetCreatureAILevel, EnableRendering, HasLineOfSight, GetRandomDestination,
    SetOrientOnClick, DisableHealthRegen, IsMeditating, GetInfluence, SetInfluence, ModifyInfluence,
    AddAvailablePUPByTemplate, AddAvailablePUPByObject, AssignPUP, SpawnAvailablePUP,
    AddPartyPuppet, GetPUPOwner, GetIsPuppet, SavePUPByObject, ActionFollowOwner,
    RemoveNPCFromPartyToBase, RebuildPartyTable, SaveNPCByObject, SetForfeitConditions,
    GetLastForfeitViolation,
    GetAreaUnescapable,
    SetAreaUnescapable, SetMaxHitPoints, GetIsPoisoned,
    SendMessageToPC, OpenStore, ShowUpgradeScreen, ShowChemicalUpgradeScreen, ShowLevelUpGUI, ShowPartySelectionGUI,
    PopUpDeathGUIPanel, DisplayMessageBox, PlayMovie, SetGlobalFadeOut, SetGlobalFadeIn,
    HoldWorldFadeInForDialog, SetFadeUntilScript, EnableVideoEffect, DisableVideoEffect,
    StartNewModule, ShowGalaxyMap,
    SetPlanetSelectable, GetPlanetSelectable, SetPlanetAvailable, GetPlanetAvailable, ResetDialogState,
    GetSelectedPlanet, PlayPazaak, GetLastPazaakResult,
    AddJournalQuestEntry, RemoveJournalQuestEntry, GetJournalEntry, GetJournalQuestExperience,
    GiveXPToCreature, SetXP, GetXP, GivePlotXP, AddMultiClass,
    GetLockKeyRequired, GetLockLockable, GetLockUnlockDC, GetLockLockDC, GetLastUnlocked,
    GetLastLocked, ActionUnlockObject, ActionLockObject, GetIsDoorActionPossible, DoDoorAction,
    GetLastAttackAction, GetLastAttackResult, GetWasForcePowerSuccessful, GetAppearanceType,
    GetCreatureSize, GetCreatureMovmentType, GetLastForcePowerUsed, GetLastCombatFeatUsed, GetTotalDamageDealt,
    GetSpellTarget, ActionMoveAwayFromObject, ActionFollowLeader, ActionEquipMostDamagingMelee,
    ActionEquipMostDamagingRanged, ActionSwitchWeapons, PlaySound, SetMapPinEnabled,
    ExploreAreaForPlayer, RevealMap,
    GetReflexAdjustedDamage,
    GetIsTrapped, GetTrapBaseType, GetTrapDetectable, GetTrapDisarmable, GetTrapFlagged,
    GetTrapOneShot, GetTrapDetectDC, GetTrapDisarmDC, GetTrapKeyTag, GetTrapCreator,
    GetTrapDetectedBy, SetTrapDetectedBy, SetTrapDisabled, GetLastTrapDetected, GetLastDisarmed,
    GetNearestTrapToObject, VersusTrapEffect, AwardStealthXP, GetStealthXPEnabled,
    SetStealthXPEnabled, GetCurrentStealthXP, SetCurrentStealthXP, GetMaxStealthXP,
    SetMaxStealthXP, GetStealthXPDecrement, SetStealthXPDecrement, IsStealthed,
    EffectCutSceneParalyze, EffectCutSceneStunned, EffectCutSceneHorrified, EffectCrush,
    EffectDroidConfused, EffectMindTrick, EffectDroidScramble, EffectBlind, EffectTrueSeeing,
    EffectFury, EffectForceSight, EffectForceBody, EffectBodyFuel, EffectPsychicStatic,
    EffectFactionModifier, EffectVPRegenModifier, EffectFPRegenModifier, EffectRegenerate,
    RemoveEffectByExactMatch, GetIsImmune, FaceObjectAwayFromObject, ActionCastFakeSpellAtObject,
    ActionCastFakeSpellAtLocation, ActionCastSpellAtObject, ActionCastSpellAtLocation,
    ActionUseSkill, ActionSurrenderToEnemies, GetLastWeaponUsed, GetFoundEnemyCreature,
    GetBlockingCreature, GetBlockingDoor, GetInventoryDisturbItem,
    // TSL's combat and Force (`script::force`).
    GetSpellBaseForcePointCost, GetSpellFormMask, GetSpellAcquired, GrantSpell, IsFormActive,
    SetCurrentForm, IsInTotalDefense, AddBonusForcePoints, SetBonusForcePoints,
    GetBonusForcePoints, AdjustCreatureAttributes, AdjustCreatureSkills,
    ModifyFortitudeSavingThrowBase, ModifyReflexSavingThrowBase, ModifyWillSavingThrowBase,
    GetSkillRankBase, GetRacialSubType, IsRunning, GetCombatActionsPending, SetFakeCombatState,
    CreatureFlourishWeapon, ForceHeartbeat, GetOwnerDemolitionsSkill, SpawnMine, DetonateMine,
    RoundsToSeconds, HoursToSeconds, TurnsToSeconds, AngleToVector, GetGameDifficulty,
    IsMoviePlaying, QueueMovie, PlayMovieQueue, SetInputClass, UnlockAllSongs, PauseGame,
    EnableRain, ShowDemoScreen, YavinHackDoorClose, SoundObjectGetPitchVariance,
    SoundObjectSetPosition, NoClicksFor, ActionBarkString, SetKeepStealthInDialog,
    DisplayFeedBackText, ShowTutorialWindow, DisableMap, SetDisableTransit, SetReturnStrref,
    ChangeObjectAppearance, DuplicateHeadAppearance, PlayOverlayAnimation, DisplayDatapad,
    GetEncounterActive, SetEncounterActive, GetEncounterSpawnsMax, SetEncounterSpawnsMax,
    GetEncounterSpawnsCurrent, SetEncounterSpawnsCurrent, SetEncounterDifficulty,
    GetEncounterDifficulty, CancelPostDialogCharacterSwitch,
}

/// Whether the routine `name` (`nwscript.nss`) is bound; every other one
/// is a counted gap (`kjk script check` lists those the scripts call).
pub fn bound(name: &str) -> bool {
    Bound::named(name).is_some()
}

/// TSL's game key (`ModuleData::game`), for its executable's own rules.
const TSL: &str = "tsl";

/// The least range `ActionFollowOwner` follows at (metres).
const FOLLOW_LEAST: f32 = rules::scripts::moves::FOLLOW_LEAST;

/// The Remote's tag (TSL's puppet 0, `RebuildPartyTable`).
const REMOTE: &str = "remote";

/// How far a surrender reaches (metres; `SurrenderToEnemies`).
const SURRENDER_REACH: f32 = rules::scripts::fights::SURRENDER_REACH;

/// How far above the places `HasLineOfSight` is given its line runs
/// (metres): clear of the floor they stand on.
const SIGHT_CLEAR_M: f32 = rules::scripts::sight::CLEAR_M;

/// Who did what last to an object (`State::last`), for `GetLast…`; what
/// it last perceived, and how (a `perception` kind).
pub mod last {
    pub const OPENED: u8 = 0;
    pub const USED: u8 = 1;
    pub const DAMAGER: u8 = 2;
    pub const KILLER: u8 = 3;
    pub const PERCEIVED: u8 = 4;
    pub const PERCEPTION: u8 = 5;
    pub const ATTACKER: u8 = 6;
    pub const SPELL_CASTER: u8 = 7;
    pub const SPELL: u8 = 8;
    pub const SPELL_HARMFUL: u8 = 9;
    pub const SPEAKER: u8 = 10;
    pub const CLOSED: u8 = 11;
    pub const DISTURBED: u8 = 12;
    /// The last to attack it or cast a harmful power at it
    /// (`GetLastHostileActor`).
    pub const HOSTILE: u8 = 13;
    /// Whom a healer means to heal (`SetHealTarget`, `GetHealTarget`).
    pub const HEAL_TARGET: u8 = 14;
    pub const UNLOCKED: u8 = 15;
    pub const LOCKED: u8 = 16;
    /// What its last attack was (`GetLastAttackAction`): `ATTACK_OBJECT`
    /// (a blow, with a combat feat or not) or `ATTACK_POWER`; the Force
    /// power and the combat feat it last used (rows).
    pub const ATTACK_ACTION: u8 = 17;
    pub const FORCE_POWER: u8 = 18;
    pub const COMBAT_FEAT: u8 = 19;
    /// The hit points its last damage took (`GetTotalDamageDealt`).
    pub const DAMAGE: u8 = 20;
    /// Whom its last Force power reached (`GetSpellTarget`).
    pub const SPELL_TARGET: u8 = 21;

    pub const ATTACK_OBJECT: u32 = 0;
    pub const ATTACK_POWER: u32 = 1;
    /// How its last blow came out (`GetLastAttackResult`): a
    /// `combat::Outcome` (hit, critical, assured, parried, deflected,
    /// refused).
    pub const ATTACK_RESULT: u8 = 23;
    /// Whether its last Force power's impact landed (1) or not (0):
    /// `GetWasForcePowerSuccessful`.
    pub const POWER_LANDED: u8 = 24;
    /// The last to disarm a trap (`GetLastDisarmed`).
    pub const DISARMED: u8 = 25;
    /// The last trap a creature detected (`GetLastTrapDetected`).
    pub const TRAP_DETECTED: u8 = 26;
    /// The item its last attack that reached its target struck with
    /// (`GetLastWeaponUsed`).
    pub const WEAPON: u8 = 27;
    /// The door that last blocked its way (`GetBlockingDoor`), the creature
    /// it last went round and a hostile it met doing so
    /// (`GetBlockingCreature`, `GetFoundEnemyCreature`).
    pub const BLOCKING_DOOR: u8 = 28;
    pub const BLOCKING_CREATURE: u8 = 29;
    pub const FOUND_ENEMY: u8 = 30;
    /// The item its inventory was last disturbed with
    /// (`GetInventoryDisturbItem`).
    pub const DISTURB_ITEM: u8 = 31;
}

/// The game's clock as `GetTimeHour` … `GetTimeMillisecond` read it, sim
/// time `now_ms` in: an hour lasts `minutes_per_hour` real minutes
/// (`rules::scripts::clock::MINUTES_PER_HOUR` where the IFO does not say),
/// a minute of it is counted up to that, a
/// second is a real one. The day starts at hour 0 [verify: the IFO's
/// `Mod_StartHour`]; only the swoop's timer reads it, by differences.
fn clock(now_ms: u64, minutes_per_hour: u8, which: Bound) -> i32 {
    let per_hour = u64::from(if minutes_per_hour == 0 {
        rules::scripts::clock::MINUTES_PER_HOUR
    } else {
        minutes_per_hour
    });
    let minutes = now_ms / 60_000;
    let value = match which {
        Bound::GetTimeHour => minutes / per_hour % 24,
        Bound::GetTimeMinute => minutes % per_hour,
        Bound::GetTimeSecond => now_ms / 1000 % 60,
        _ => now_ms % 1000,
    };
    value as i32
}

/// Good and evil as scripts see them (`ALIGNMENT_*`, the same in both
/// games' `nwscript.nss`).
pub(crate) mod alignment {
    pub const NEUTRAL: i32 = 1;
    pub const LIGHT_SIDE: i32 = 2;
    pub const DARK_SIDE: i32 = 3;

    /// The `ALIGNMENT_*` of a value (`GetAlignmentGoodEvil`).
    pub fn side(value: u8) -> i32 {
        use crate::combat::sheet::Side;
        match crate::combat::sheet::side(value) {
            Side::Light => LIGHT_SIDE,
            Side::Dark => DARK_SIDE,
            Side::Neutral => NEUTRAL,
        }
    }

    /// `value` moved `shift` toward a side (`AdjustAlignment`), within 0 to
    /// 100; toward neutral it stops at 50.
    pub fn shifted(value: u8, toward: i32, shift: i32) -> u8 {
        let (value, shift) = (i32::from(value), shift.max(0));
        let neutral = i32::from(crate::combat::sheet::NEUTRAL);
        let moved = match toward {
            LIGHT_SIDE => value + shift,
            DARK_SIDE => value - shift,
            NEUTRAL if value > neutral => (value - shift).max(neutral),
            NEUTRAL => (value + shift).min(neutral),
            _ => value,
        };
        moved.clamp(0, 100) as u8
    }
}

/// Whether `script` run for `who` is its own AI: one of its AI event
/// hooks (`AI_HOOKS`). A user-defined event that run signals to itself is
/// its AI's too (`Pending::ai`).
pub(super) fn own_ai(view: &View, who: ObjectId, script: &str) -> bool {
    AI_HOOKS
        .iter()
        .any(|hook| view.hook(who, *hook).as_deref() == Some(script))
}

/// A creature's AI event hooks: what they queue on a companion a guest
/// drives is skipped (`Engine::own_ai_on_driven`).
const AI_HOOKS: [Hook; 9] = [
    Hook::Heartbeat,
    Hook::EndDialogue,
    Hook::Notice,
    Hook::EndRound,
    Hook::Damaged,
    Hook::Attacked,
    Hook::SpellCastAt,
    Hook::Disturbed,
    Hook::Blocked,
];

/// The `TALKVOLUME_*` constants, in the order of their reaches
/// (`rules::scripts::talk::VOLUME_REACH`, metres). A creature hears no
/// farther than its own hearing besides (`speak`).
const VOLUMES: [&str; 5] = [
    "TALKVOLUME_TALK",
    "TALKVOLUME_WHISPER",
    "TALKVOLUME_SHOUT",
    "TALKVOLUME_SILENT_TALK",
    "TALKVOLUME_SILENT_SHOUT",
];

fn volume_reach(library: &Library, volume: i32) -> f32 {
    let reaches = rules::scripts::talk::VOLUME_REACH;
    VOLUMES
        .iter()
        .zip(reaches)
        .find(|(name, _)| library.constant(name) == Some(volume))
        .map_or(reaches[0], |(_, reach)| reach)
}

/// A talent's `TALENT_TYPE_*` (`GetTypeFromTalent`, the excluded type of
/// `GetCreatureTalentBest`): a Force power's, a feat's.
fn talent_type(library: &Library, talent: Use) -> i32 {
    let name = match talent {
        Use::Power(_) => "TALENT_TYPE_FORCE",
        Use::Feat(_) => "TALENT_TYPE_FEAT",
    };
    library.constant(name).unwrap_or(-1)
}

/// The saves `SAVING_THROW_*` names, by `Modifies::Save`'s number: one
/// save (Fortitude, Reflex, Will) or all of them (`ALL_SAVES`).
const SAVES: [(&str, u8); 3] = [
    ("SAVING_THROW_FORT", 0),
    ("SAVING_THROW_REFLEX", 1),
    ("SAVING_THROW_WILL", 2),
];

/// Whether `text` fits a listen pattern: the same, `**` standing for any
/// run of characters (NWScript's wildcard).
fn heard(pattern: &str, text: &str) -> bool {
    match pattern.split_once("**") {
        None => pattern == text,
        Some((head, rest)) => {
            let Some(text) = text.strip_prefix(head) else {
                return false;
            };
            (0..=text.len())
                .filter(|at| text.is_char_boundary(*at))
                .any(|at| heard(rest, &text[at..]))
        }
    }
}

/// Whether a character is immune to `IMMUNITY_TYPE_*` `immunity` at
/// `now`: an immunity effect on it or one its worn items give (or one to
/// every mind effect), or the Sentinel's Force Immunity feat for fear,
/// stun or paralysis.
fn immune(c: &Character, immunity: u8, now: u64) -> bool {
    use crate::combat::force::crowd::immunity as kinds;
    use crate::combat::sheet::feats;
    let worn = |kind: u8| c.sheet.gear.immune_to(kind);
    let by_effect = worn(immunity)
        || worn(kinds::MIND_SPELLS)
        || c.lasting.iter().flatten().any(|l| {
            l.until > now
                && l.modifies == Modifies::Immunity
                && (l.of == immunity || l.of == kinds::MIND_SPELLS)
        });
    let feat = match immunity {
        kinds::FEAR => Some(feats::FORCE_IMMUNITY_FEAR),
        kinds::STUN => Some(feats::FORCE_IMMUNITY_STUN),
        kinds::PARALYSIS => Some(feats::FORCE_IMMUNITY_PARALYSIS),
        _ => None,
    };
    by_effect || feat.is_some_and(|feat| c.sheet.feats.has(feat))
}

/// Whether a character is immune to `IMMUNITY_TYPE_*` `immunity` by its
/// effects and worn items alone, an immunity to all counting (TSL's
/// `GetEffectImmunity`): no feats, no mind umbrella.
fn immune_effect(c: &Character, immunity: u8, now: u64) -> bool {
    c.sheet.gear.immune_to(immunity)
        || c.lasting.iter().flatten().any(|l| {
            l.until > now && l.modifies == Modifies::Immunity && (l.of == immunity || l.of == 0)
        })
}

/// What power `row` costs `thing`, a character, now (hundredths of FP;
/// `ModuleData::power_cost`: TSL's adjustments by alignment, Charisma, the
/// room's Force rating where it stands and the Form in use, K1's row as
/// listed); `None` for a row the table lacks.
pub(super) fn cast_cost(view: &View, thing: &Thing, row: u16) -> Option<i32> {
    let c = thing.character.as_ref()?;
    view.module
        .power_cost(row, &c.sheet, c.party, thing.position)
        .map(|cost| cost * HUNDREDTHS)
}

/// Whether `thing`, a character, knows power `row` and can pay for it now:
/// its cost (`cast_cost`) within its FP, or under TSL's Force Body its
/// share there and as much from more than its last hit point
/// (`effects::can_pay`), as `GetHasSpell` and the check a combat round
/// makes before it casts have it (one it cannot pay for is not cast).
pub(super) fn can_cast(view: &View, thing: &Thing, row: u16) -> bool {
    let Some(c) = thing.character.as_ref().filter(|c| c.powers.has(row)) else {
        return false;
    };
    let hp = thing.hp.map_or(0, |(hp, _)| hp * HUNDREDTHS);
    cast_cost(view, thing, row).is_some_and(|cost| {
        crate::combat::effects::can_pay(&c.lasting, (c.fp, hp), cost, view.now_ms)
    })
}

/// `who` stops what it does and fights no more: its queue cleared (unless
/// not commandable), out of combat.
fn stop(state: &mut State, out: &mut Out, view: &View, who: ObjectId) {
    if !state.uncommandable.contains(&who.0) {
        clear_queue(state, view, who);
    }
    out.changes.push(Change::CancelCombat { who });
}

/// `who`'s queue cleared. A conversation pause still in it, one that never
/// reached the front, is dropped with it: the executables queue the pause
/// as they queue the resume (reone's `PauseConversationAction`), so a clear
/// before it ran leaves the conversation going on. Citadel Station's Czerka
/// men pause their line, clear their own queues and walk in (`a_czerkaint`):
/// their resume, cleared too, would otherwise hold it for good.
fn clear_queue(state: &mut State, view: &View, who: ObjectId) {
    let Some(queue) = state.actions.remove(&who.0) else {
        return;
    };
    let host = view.host(&state.party);
    for queued in queue {
        if let Action::Pause { talk } = queued.action
            && let Some(at) = state.talk.held_by(who, talk, host)
        {
            state.talk.conversations[at].paused = false;
        }
    }
}

/// `caller` gives up (`SurrenderToEnemies`; Calo Nord when Davik falls,
/// before his conversation), as K1 documents it: it and every creature
/// within `SURRENDER_REACH` stop what they do and fight no more; its enemies there
/// regard it as neutral (here it joins the neutral faction: KOTOR's
/// personal standing is not kept); unless it `keeps` (`SurrenderRetainBuffs`)
/// those stopped lose the effects they put on themselves. A player's
/// character never does (TSL's executable).
pub(super) fn surrender(
    view: &View,
    state: &mut State,
    out: &mut Out,
    caller: ObjectId,
    keeps: bool,
) {
    let Some(from) = view
        .thing(caller)
        .filter(|thing| thing.character.is_some())
        .map(|thing| thing.position)
    else {
        return;
    };
    if view.pcs(&state.party).contains(&caller) {
        return;
    }
    out.changes.push(Change::Faction {
        who: caller,
        faction: crate::combat::sheet::factions::NEUTRAL,
    });
    let after = state.applied.len() as u32;
    for thing in view.things {
        let Some(c) = &thing.character else {
            continue;
        };
        if thing.id != caller && distance(from, thing.position) > SURRENDER_REACH {
            continue;
        }
        stop(state, out, view, thing.id);
        if !keeps {
            for lasting in c.lasting.iter().flatten().filter(|l| l.by == thing.id.0) {
                state.removed.push((thing.id.0, Some(lasting.id), after));
            }
        }
    }
}

/// Whether lasting effect `lasting` is the same as `effect` as TSL's
/// `RemoveEffectByExactMatch` compares them: of one kind, with the same
/// first two numbers (for a modifier, what it changes and by how much; a
/// regeneration's hit points and interval; a state's number and cutscene
/// flag; a visual's row). A link matches nothing.
fn exact_match(effect: &Effect, lasting: &Lasting) -> bool {
    let numbers = (lasting.modifies, lasting.of, lasting.amount);
    match *effect {
        // A faction modifier's slot keeps the faction it replaced in `of`.
        Effect::Modifier {
            modifies: Modifies::Faction,
            amount,
            ..
        } => (lasting.modifies, lasting.amount) == (Modifies::Faction, amount),
        Effect::Modifier {
            modifies,
            amount,
            of,
        } => numbers == (modifies, of, amount),
        Effect::State { state, cutscene } => {
            numbers == (Modifies::State, state, i16::from(cutscene))
        }
        Effect::Visual { row: Some(row), .. } => numbers == (Modifies::Visual, 0, row as i16),
        _ => false,
    }
}

/// The immunity a save of `SAVING_THROW_TYPE_*` `kind` meets, if any.
fn save_immunity(library: &Library, kind: i32) -> Option<u8> {
    use crate::combat::force::crowd::immunity;
    [
        ("SAVING_THROW_TYPE_FEAR", immunity::FEAR),
        ("SAVING_THROW_TYPE_MIND_AFFECTING", immunity::MIND_SPELLS),
        ("SAVING_THROW_TYPE_POISON", immunity::POISON),
        ("SAVING_THROW_TYPE_PARALYSIS", immunity::PARALYSIS),
    ]
    .iter()
    .find(|(name, _)| library.constant(name) == Some(kind))
    .map(|(_, immunity)| *immunity)
}

/// The kinds of walk `GetFirst…`/`GetNext…` keep (`Iterators`).
mod walk {
    pub const AREA: u8 = 0;
    pub const PERSISTENT: u8 = 1;
    pub const SHAPE: u8 = 2;
    pub const EFFECTS: u8 = 3;
    pub const INVENTORY: u8 = 4;
    pub const PCS: u8 = 5;
}

/// `GetEffectType`'s `EFFECT_TYPE_*` name for a lasting effect.
fn effect_type(modifies: Modifies, amount: i16) -> &'static str {
    let up = amount >= 0;
    match modifies {
        Modifies::Ability if up => "EFFECT_TYPE_ABILITY_INCREASE",
        Modifies::Ability => "EFFECT_TYPE_ABILITY_DECREASE",
        Modifies::Defence if up => "EFFECT_TYPE_AC_INCREASE",
        Modifies::Defence => "EFFECT_TYPE_AC_DECREASE",
        Modifies::Save if up => "EFFECT_TYPE_SAVING_THROW_INCREASE",
        Modifies::Save => "EFFECT_TYPE_SAVING_THROW_DECREASE",
        Modifies::Attack if up => "EFFECT_TYPE_ATTACK_INCREASE",
        Modifies::Attack => "EFFECT_TYPE_ATTACK_DECREASE",
        Modifies::Damage if up => "EFFECT_TYPE_DAMAGE_INCREASE",
        Modifies::Damage => "EFFECT_TYPE_DAMAGE_DECREASE",
        Modifies::Resistance => "EFFECT_TYPE_DAMAGE_RESISTANCE",
        Modifies::DamageImmunity if up => "EFFECT_TYPE_DAMAGE_IMMUNITY_INCREASE",
        Modifies::DamageImmunity => "EFFECT_TYPE_DAMAGE_IMMUNITY_DECREASE",
        Modifies::Immunity => "EFFECT_TYPE_IMMUNITY",
        Modifies::Speed if up => "EFFECT_TYPE_MOVEMENT_SPEED_INCREASE",
        Modifies::Speed => "EFFECT_TYPE_MOVEMENT_SPEED_DECREASE",
        Modifies::Attacks => "EFFECT_TYPE_HASTE",
        Modifies::Skill if up => "EFFECT_TYPE_SKILL_INCREASE",
        Modifies::Skill => "EFFECT_TYPE_SKILL_DECREASE",
        Modifies::AssuredHit => "EFFECT_TYPE_ASSUREDHIT",
        Modifies::ForceResistance => "EFFECT_TYPE_FORCE_RESISTANCE_INCREASE",
        // K1 names no type for it.
        Modifies::ForcePoints => "EFFECT_TYPE_INVALIDEFFECT",
        Modifies::Visual => "EFFECT_TYPE_VISUALEFFECT",
        Modifies::Vitality => "EFFECT_TYPE_TEMPORARY_HITPOINTS",
        Modifies::Poison => "EFFECT_TYPE_POISON",
        Modifies::Disguise => "EFFECT_TYPE_DISGUISE",
        Modifies::TrueSeeing => "EFFECT_TYPE_TRUESEEING",
        Modifies::Blind => "EFFECT_TYPE_BLINDNESS",
        Modifies::Regenerate => "EFFECT_TYPE_REGENERATE",
        // TSL's executable names no type for these (a state's is its own,
        // `GetEffectType`).
        Modifies::State
        | Modifies::Fury
        | Modifies::ForceSight
        | Modifies::ForceBody
        | Modifies::BodyFuel
        | Modifies::PsychicStatic
        | Modifies::Faction
        | Modifies::VpRegen
        | Modifies::FpRegen => "EFFECT_TYPE_INVALIDEFFECT",
    }
}

/// How one character regards another (`repute.2da`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Regard {
    Friend,
    Enemy,
    Neutral,
}

const REGARDS: [(&str, Regard); 3] = [
    ("REPUTATION_TYPE_FRIEND", Regard::Friend),
    ("REPUTATION_TYPE_ENEMY", Regard::Enemy),
    ("REPUTATION_TYPE_NEUTRAL", Regard::Neutral),
];

/// How near `ActionStartConversation` brings its speaker (metres).
pub const CONVERSATION_RANGE: f32 = rules::scripts::talk::CONVERSATION_RANGE;
/// How near `ActionMoveToLocation` comes to its place (metres): it takes
/// no range; `ActionMoveToObject`'s default `fRange`.
pub(crate) const MOVE_TO_LOCATION_RANGE: f32 = rules::scripts::moves::MOVE_TO_LOCATION_RANGE;
/// One play of an animation whose clip the body's model has no length for
/// (ms).
pub(super) const ANIMATION_MS: u32 = rules::scripts::animations::UNKNOWN_MS;
/// The looping `ANIMATION_…` constants (the low range) a negative duration
/// plays at once, outside the queue, until something else plays.
const LOOPS_AT_ONCE: u16 = 32;
/// A queued animation's time from which the same one queued after it is
/// dropped (ms, `queued_long`).
const LONG_ANIMATION_MS: u32 = rules::scripts::animations::LONG_MS;

/// The host scripts run against: the library, the view, the state.
pub(super) struct Engine<'a> {
    pub(super) library: &'a Library,
    pub(super) view: &'a View<'a>,
    pub(super) state: &'a mut State,
    pub(super) rng: &'a mut SimRng,
    pub(super) report: &'a mut Report,
    pub(super) out: &'a mut Out,
    pub(super) iterators: &'a mut super::Iterators,
    /// The cast whose impact runs, if this is one.
    pub(super) spell: Option<&'a super::Cast>,
    /// The running script (whose code deferred blocks are).
    pub(super) script: &'a str,
    pub(super) event: Event,
    pub(super) depth: u8,
}

/// An argument by place, of a type the VM checked.
pub(super) fn int(args: &[Value], at: usize) -> i32 {
    match args.get(at) {
        Some(Value::Int(v)) => *v,
        _ => 0,
    }
}

pub(super) fn float(args: &[Value], at: usize) -> f32 {
    match args.get(at) {
        Some(Value::Float(v)) => *v,
        _ => 0.0,
    }
}

pub(super) fn text(args: &[Value], at: usize) -> &str {
    match args.get(at) {
        Some(Value::String(v)) => v,
        _ => "",
    }
}

pub(super) fn object(args: &[Value], at: usize) -> ObjectId {
    match args.get(at) {
        Some(Value::Object(v)) => *v,
        _ => ObjectId::INVALID,
    }
}

pub(super) fn vector(args: &[Value], at: usize) -> [f32; 3] {
    match args.get(at) {
        Some(Value::Vector(v)) => *v,
        _ => [0.0; 3],
    }
}

/// A talent as a value: 0 none, else one more than its row, feats from
/// `FEAT_TALENTS` on.
const FEAT_TALENTS: u32 = 0x1_0001;

fn talent_value(talent: Option<Use>) -> Value {
    Value::Talent(Handle(match talent {
        None => 0,
        Some(Use::Power(row)) => 1 + u32::from(row),
        Some(Use::Feat(row)) => FEAT_TALENTS + u32::from(row),
    }))
}

fn talent(args: &[Value], at: usize) -> Option<Use> {
    match args.get(at) {
        Some(Value::Talent(Handle(n))) if *n >= FEAT_TALENTS => {
            u16::try_from(n - FEAT_TALENTS).ok().map(Use::Feat)
        }
        Some(Value::Talent(Handle(n))) if *n > 0 => u16::try_from(n - 1).ok().map(Use::Power),
        _ => None,
    }
}

fn handle(args: &[Value], at: usize) -> Handle {
    match args.get(at) {
        Some(Value::Location(h) | Value::Event(h)) => *h,
        _ => Handle(0),
    }
}

fn action(args: &[Value], at: usize) -> Option<Deferred> {
    match args.get(at) {
        Some(Value::Action(deferred)) => Some((**deferred).clone()),
        _ => None,
    }
}

pub(super) fn string(text: impl Into<String>) -> Value {
    Value::String(Arc::from(text.into()))
}

pub(super) fn truth(value: bool) -> Value {
    Value::Int(i32::from(value))
}

/// How a thing `DestroyObject` takes out of the world is drawn going
/// (KOTOR's executable: the call sets the object's no-fade flag and its
/// delay until the fade, in ms, then queues its destroy event after
/// `fDelay`; the server removes it then, and the client fades the model it
/// holds): a creature or placeable is drawn whole for `fDelayUntilFade`,
/// then fades out over the engine's fade (`remains::FADE_MS`), unless
/// `bNoFade`; anything else is not drawn after.
pub(super) fn destroy_going(
    kind: Option<ObjectKind>,
    no_fade: bool,
    until_fade: f32,
) -> Option<Going> {
    let fades = matches!(kind, Some(ObjectKind::Creature | ObjectKind::Placeable));
    (fades && !no_fade).then(|| {
        Going::Departs(Departure {
            whole_ms: seconds_ms(until_fade),
            fade_ms: crate::world::remains::FADE_MS,
        })
    })
}

pub(super) fn seconds_ms(seconds: f32) -> u32 {
    (seconds.max(0.0) * 1000.0).round() as u32
}

/// Whether `animation` asked for `seconds` plays at once and loops until
/// something else plays (`u32::MAX`): a looping constant of the low range
/// given a negative duration.
/// Who holds `item` (`GetItemPossessor`): the first PC for the party's
/// own, none for one on the floor or unknown.
fn possessor(state: &State, view: &View, item: ObjectId) -> ObjectId {
    match state.items.get(&item.0) {
        Some(held) if held.holder == inventory::PARTY => view.first_pc(&state.party),
        Some(held) if held.holder == AREA.0 => ObjectId::INVALID,
        Some(held) => ObjectId(held.holder),
        None => ObjectId::INVALID,
    }
}

fn loops_on(animation: u16, seconds: f32) -> bool {
    animation <= LOOPS_AT_ONCE && seconds < 0.0
}

/// How long `animation` (an `ANIMATION_…` constant) asked for `seconds` at
/// `speed` plays (ms), its clip `clip` ms long on the body where known: a
/// looping one given a duration exactly that, whatever the speed; one that
/// `loops_on` until replaced; any other one play of its clip at the speed
/// (a fire-and-forget clip, or a duration of 0 or less), `ANIMATION_MS`
/// with no length. A speed of 0 plays at 1 [verify].
fn animation_ms(animation: u16, (speed, seconds): (f32, f32), clip: Option<u32>) -> u32 {
    use crate::combat::idle::{FIRE_AND_FORGET, LOOPED_ROWS};
    let looping = !(FIRE_AND_FORGET..LOOPED_ROWS).contains(&animation);
    if looping && seconds > 0.0 {
        seconds_ms(seconds)
    } else if loops_on(animation, seconds) {
        u32::MAX
    } else {
        let speed = if speed == 0.0 { 1.0 } else { speed.abs() };
        clip.map_or(ANIMATION_MS, |ms| (ms as f32 / speed).round() as u32)
    }
}

/// An object id as a value, `INVALID` for none.
pub(super) fn maybe(id: Option<ObjectId>) -> Value {
    Value::Object(id.unwrap_or(ObjectId::INVALID))
}

/// Whether `module`'s global catalogue (`ModuleData::globals`) declares
/// `name` (any case) as a `kind` global; any is, without a catalogue.
pub(super) fn declared(module: &ModuleData, name: &str, kind: Global) -> bool {
    module.globals.is_empty() || module.globals.get(&key(name)) == Some(&kind)
}
impl Engine<'_> {
    /// The row `target` answers `caller`'s `CutsceneAttack` of `row` with,
    /// as the script's result (`answer`) says (`combat::rounds::Table::
    /// answer`), `combat::rounds::NONE` for none.
    fn cutscene_answer(
        &self,
        view: &View,
        (caller, target): (ObjectId, ObjectId),
        row: u16,
        answer: crate::combat::rounds::Answer,
    ) -> u16 {
        use crate::combat::rounds::{NONE, Side};
        let side = |id| {
            let sheet = view.thing(id)?.character.as_ref()?.sheet;
            let creature = view.facts(id).is_some_and(|facts| facts.strike.is_some());
            Some((sheet, creature))
        };
        let (Some(_), Some((sheet, creature))) = (side(caller), side(target)) else {
            return NONE;
        };
        let target = Side {
            sheet: &sheet,
            creature,
        };
        self.library
            .rounds()
            .answer(row, answer, target)
            .unwrap_or(NONE)
    }

    pub(super) fn trace(&mut self, text: &str) {
        let line = format!("{:>8.2}s {}", self.view.now_ms as f64 / 1000.0, text);
        self.report.line(line);
    }

    /// The `nth` (from 0) object tagged `tag` (`script::tagged`), of
    /// `kind` when given.
    fn by_tag(&self, tag: &str, nth: i32, kind: Option<ObjectKind>) -> ObjectId {
        self.tagged(tag)
            .filter(|tagged| kind.is_none_or(|kind| tagged.kind == Some(kind)))
            .nth(usize::try_from(nth).unwrap_or(usize::MAX))
            .map_or(ObjectId::INVALID, |tagged| tagged.id)
    }

    /// Every object tagged `tag` (`script::tagged`), then the items lying
    /// on this area's floor so tagged.
    fn tagged<'a>(&'a self, tag: &str) -> impl Iterator<Item = super::Tagged> + 'a {
        let catalog = self.library.items();
        let wanted = key(tag);
        let wanted_held = wanted.clone();
        let lying = self
            .state
            .lying_in(&self.view.module.name)
            .filter(move |(id, _)| {
                self.state
                    .items
                    .get(id)
                    .and_then(|item| self.state.item_facts(catalog, &item.template))
                    .is_some_and(|facts| facts.tag == wanted)
            })
            .map(|(id, lying)| super::Tagged {
                id: ObjectId(id),
                kind: None,
                at: lying.at,
            });
        // Then the items held here, as the games keep every item an object
        // (a key or a datapad in a pocket is found): the party's, at the
        // first PC, and those of an object in this area, at it. TSL's
        // Czerka droid B-4D4 asks whether Opo Chano's credentials are
        // anywhere (`c_havecreds`: `GetObjectByTag("DroidTechCreds")`).
        let pc = self.view.first_pc(&self.state.party);
        let held = self
            .state
            .items
            .iter()
            .filter(move |(_, item)| {
                self.state
                    .item_facts(catalog, &item.template)
                    .is_some_and(|facts| facts.tag == wanted_held)
            })
            .filter_map(move |(id, item)| {
                let at = if item.holder == crate::inventory::PARTY {
                    self.position(pc)?
                } else {
                    self.position(ObjectId(item.holder))?
                };
                Some(super::Tagged {
                    id: ObjectId(*id),
                    kind: None,
                    at,
                })
            });
        tagged(self.view, self.out, tag).chain(lying).chain(held)
    }

    /// The `nth` (from 1) nearest object to `from` tagged `tag`, but itself.
    fn nearest_by_tag(&self, tag: &str, from: ObjectId, nth: i32) -> ObjectId {
        let Some(origin) = self.position(from) else {
            return ObjectId::INVALID;
        };
        let found = self
            .tagged(tag)
            .filter(|tagged| tagged.id != from)
            .map(|tagged| (distance(origin, tagged.at), tagged.id))
            .collect();
        nth_nearest(found, nth)
    }

    /// An `ACTION_*` constant's value; `ACTION_INVALID`'s for one the
    /// catalogue lacks.
    fn action_value(&self, name: &str) -> Value {
        let value = |name| self.library.constant(name);
        Value::Int(
            value(name)
                .or_else(|| value("ACTION_INVALID"))
                .unwrap_or(-1),
        )
    }

    pub(super) fn character(&self, id: ObjectId) -> Option<Character> {
        self.view.thing(id)?.character
    }

    /// `id`'s standard faction: a character's; a laid mine's, its layer's
    /// (`TrapState::faction`); else a door's, placeable's or trap's as its
    /// data sets it (Tatooine's "Sandperson Faction" placeable).
    fn faction(&self, id: ObjectId) -> Option<u8> {
        self.character(id)
            .map(|c| c.faction)
            .or_else(|| self.state.traps.get(&id.0).and_then(|kept| kept.faction))
            .or_else(|| {
                self.instance(id)
                    .map(|facts| facts.faction)
                    .filter(|faction| *faction != 0)
            })
    }

    /// How `from` regards `to` by their factions (a trap's as its script
    /// asks, `k_trp_generic`'s `GetIsNeutral`).
    fn regard(&self, from: ObjectId, to: ObjectId) -> Option<Regard> {
        let (from, to) = (self.faction(from)?, self.faction(to)?);
        let repute = self.view.module.repute.with(&self.state.reputation);
        Some(if repute.hostile(from, to) {
            Regard::Enemy
        } else if repute.friendly(from, to) {
            Regard::Friend
        } else {
            Regard::Neutral
        })
    }

    /// `GetNearestCreature`: the `nth` (from 1) nearest character to
    /// `target` but itself meeting every criterion given. The dead (and
    /// the downed, at 0 HP) only when a criterion asks about life
    /// (`CREATURE_TYPE_IS_ALIVE`), as the original engine leaves them out
    /// (xoreos' KOTOR area skips them) [verify]: else a creature's AI
    /// picks a fallen foe nearer than a standing one and attacks nobody.
    fn nearest_creature(&self, args: &[Value]) -> ObjectId {
        let target = object(args, 2);
        let Some(from) = self.view.thing(target) else {
            return ObjectId::INVALID;
        };
        let criteria =
            [(0, 1), (4, 5), (6, 7)].map(|(kind, value)| (int(args, kind), int(args, value)));
        let about_life = criteria
            .iter()
            .any(|(kind, _)| self.library.criteria.get(kind) == Some(&Criterion::Alive));
        let found: Vec<(f32, ObjectId)> = self
            .view
            .things
            .iter()
            .filter(|thing| thing.id != target && thing.character.is_some())
            .filter(|thing| about_life || thing.hp.is_some_and(|(hp, _)| hp > 0))
            .filter(|thing| {
                criteria
                    .iter()
                    .all(|&(kind, value)| self.meets(thing, target, kind, value))
            })
            .map(|thing| (distance(from.position, thing.position), thing.id))
            .collect();
        nth_nearest(found, int(args, 3))
    }

    /// Whether `thing` meets a `GetNearestCreature` criterion seen from
    /// `target`; a criterion not named (−1) always. An effect criterion
    /// asks after a lasting effect of that spell (as `GetHasSpellEffect`).
    fn meets(&self, thing: &Thing, target: ObjectId, kind: i32, value: i32) -> bool {
        let Some(criterion) = self.library.criteria.get(&kind) else {
            return true;
        };
        match criterion {
            Criterion::Race => thing.character.is_some_and(|c| i32::from(c.race) == value),
            Criterion::PlayerChar => (value != 0) == self.pcs().contains(&thing.id),
            Criterion::Class => thing.character.is_some_and(|c| {
                c.classes
                    .iter()
                    .any(|&(class, levels)| levels > 0 && i32::from(class) == value)
            }),
            Criterion::HasEffect | Criterion::LacksEffect => {
                let has = thing.character.is_some_and(|c| {
                    c.lasting
                        .iter()
                        .flatten()
                        .any(|l| i32::from(l.spell) == value)
                });
                has == matches!(criterion, Criterion::HasEffect)
            }
            Criterion::Reputation => {
                let wanted = REGARDS
                    .iter()
                    .find(|(name, _)| self.library.constant(name) == Some(value))
                    .map(|(_, regard)| *regard);
                wanted.is_some() && wanted == self.regard(target, thing.id)
            }
            Criterion::Alive => (value != 0) == thing.hp.is_some_and(|(hp, _)| hp > 0),
            Criterion::Perception => {
                let Some(&(seen, heard)) = self.library.perceptions.get(&value) else {
                    return false;
                };
                let perceived = self.state.perceived.get(&target.0);
                let has = |ids: fn(&super::Perceived) -> &Vec<u32>| {
                    perceived.is_some_and(|p| ids(p).contains(&thing.id.0))
                };
                seen.is_none_or(|seen| seen == has(|p| &p.seen))
                    && heard.is_none_or(|heard| heard == has(|p| &p.heard))
            }
        }
    }

    /// A talent's facts (`spells.2da` or `feat.2da`).
    fn facts(&self, talent: Use) -> super::Talent {
        let (table, row) = match talent {
            Use::Power(row) => (&self.library.talents.spells, row),
            Use::Feat(row) => (&self.library.talents.feats, row),
        };
        table
            .get(usize::from(row))
            .copied()
            .unwrap_or_else(super::Talent::default)
    }

    /// Whether character `who` knows power `row` and can pay for it now
    /// (`can_cast`).
    fn can_cast(&self, who: ObjectId, row: u16) -> bool {
        self.view
            .thing(who)
            .is_some_and(|thing| can_cast(self.view, thing, row))
    }

    /// Whether `who` has `talent` and can use it now: a power it knows and
    /// can pay for, or an item's spell with a use left; a feat it has.
    fn has_talent(&self, who: ObjectId, talent: Use) -> bool {
        let Some(c) = self.character(who) else {
            return false;
        };
        match talent {
            Use::Power(row) => {
                self.can_cast(who, row)
                    || self
                        .state
                        .item_talent(self.library.items(), self.view, who, row)
                        .is_some()
            }
            Use::Feat(row) => c.feats.has(row),
        }
    }

    /// `who`'s talents in `category` (each one's code within it: the AI's
    /// codes are masks), but those excluded (`exclude`: a type, `TALENT_TYPE_*`,
    /// and an id or −1 for all of it); powers by row, then feats.
    fn talents_in(&self, who: ObjectId, category: i32, exclude: (i32, i32)) -> Vec<(Use, u8)> {
        let mask = category as u32;
        let powers = (0..Rows::<5>::ROWS).map(Use::Power);
        let feats = (0..Rows::<4>::ROWS).map(Use::Feat);
        powers
            .chain(feats)
            .filter(|talent| self.has_talent(who, *talent))
            .filter_map(|talent| {
                let facts = self.facts(talent);
                let code = u32::from(facts.category);
                (code != 0 && code & mask == code).then_some((talent, facts.maxcr))
            })
            .filter(|(talent, _)| {
                let (Use::Power(row) | Use::Feat(row)) = *talent;
                let (kind, id) = (talent_type(self.library, *talent), i32::from(row));
                !(exclude.0 == kind && (exclude.1 == -1 || exclude.1 == id))
            })
            .collect()
    }

    /// An effect value for `effect`.
    fn effect_value(&mut self, effect: Effect) -> Value {
        Value::Effect(self.state.effect(effect))
    }

    /// The effect a value is (an empty one's: none).
    fn effect_arg(&self, args: &[Value], at: usize) -> Option<Effect> {
        match args.get(at) {
            Some(Value::Effect(handle)) => self.state.effect_at(*handle).cloned(),
            _ => None,
        }
    }

    /// `effect` applied on `target` by `by` (a `DURATION_TYPE_*`; a
    /// temporary one `seconds` long): what the sim carries out waits for
    /// the next step's pipeline (`script::effects`); visual ones change
    /// nothing; any other part is a gap, by its name.
    fn apply_effect(
        &mut self,
        duration: i32,
        effect: Effect,
        (target, at): (ObjectId, Option<[f32; 3]>),
        by: ObjectId,
        seconds: f32,
    ) {
        let named = |name: &str, fallback: i32| self.library.constant(name).unwrap_or(fallback);
        let until = if duration == named("DURATION_TYPE_INSTANT", 0) {
            None
        } else if duration == named("DURATION_TYPE_PERMANENT", 2) {
            Some(u64::MAX)
        } else {
            Some(self.view.now_ms + u64::from(seconds_ms(seconds)))
        };
        // A power's impact that lands more than a visual on someone: it
        // worked (`GetWasForcePowerSuccessful`; a target that resisted or
        // saved is left with the visual alone).
        if let Some(cast) = self.spell
            && effect
                .leaves()
                .iter()
                .any(|leaf| !matches!(leaf, Effect::Visual { .. }))
        {
            self.state
                .last
                .insert((cast.caster.0, last::POWER_LANDED), 1);
        }
        let mut carried = Vec::new();
        // A lasting part applied at once stays, as a permanent one: both
        // executables keep any effect its own apply does not discard, and
        // only a temporary one expires (an assured hit, a sleep).
        let mut kept = Vec::new();
        for leaf in effect.leaves() {
            // A poison takes hold once; its course runs on the character
            // (`combat::items::Poisoned`).
            let at_once = matches!(
                leaf,
                Effect::Damage { .. }
                    | Effect::Heal(_)
                    | Effect::Death
                    | Effect::HealForcePoints(_)
                    | Effect::DamageForcePoints(_)
                    | Effect::Resurrection
                    | Effect::Poison { .. }
                    | Effect::Pushed { .. }
            );
            let lasting = matches!(
                leaf,
                Effect::Modifier { .. }
                    | Effect::Hold(_)
                    | Effect::State { .. }
                    | Effect::Shield { .. }
            );
            match leaf {
                Effect::DeathKept => {
                    self.state.kept.insert(target.0);
                    carried.push(Effect::Death);
                }
                Effect::Visual { row: None, .. } => {}
                Effect::Visual { .. } => carried.push(leaf.clone()),
                _ if at_once || (lasting && until.is_some()) => carried.push(leaf.clone()),
                _ if lasting => kept.push(leaf.clone()),
                _ => {
                    let what = if until.is_none() {
                        "at once"
                    } else {
                        "lasting"
                    };
                    *self
                        .report
                        .gaps
                        .entry(format!("{} ({what})", leaf.name()))
                        .or_insert(0) += 1;
                }
            }
        }
        if target == ObjectId::INVALID {
            return;
        }
        for (carried, until) in [(carried, until), (kept, Some(u64::MAX))] {
            self.carry(carried, (target, at), by, until);
        }
    }

    /// `carried` (an effect's parts the sim carries out) applied on
    /// `target` by `by` until `until` (`apply_effect`).
    fn carry(
        &mut self,
        mut carried: Vec<Effect>,
        (target, at): (ObjectId, Option<[f32; 3]>),
        by: ObjectId,
        until: Option<u64>,
    ) {
        if carried.is_empty() {
            return;
        }
        // A minigame's thing is no character: a visual on it, for a while,
        // goes where the thing is (or the place given), carried by the
        // party's leader (the turret's loss: the Hawk's blasts); a permanent
        // one would outlast the minigame on the leader.
        let (target, at) = match crate::minigame::thing(target.0) {
            Some(_)
                if until != Some(u64::MAX)
                    && carried
                        .iter()
                        .all(|leaf| matches!(leaf, Effect::Visual { .. })) =>
            {
                let there = self.view.minigame_place(target);
                (
                    self.view.party.first().copied().unwrap_or(target),
                    at.or(there),
                )
            }
            _ => (target, at),
        };
        let effect = match carried.len() {
            1 => carried.remove(0),
            _ => Effect::Linked(carried),
        };
        let id = self.state.next_effect;
        self.state.next_effect = self.state.next_effect.wrapping_add(1);
        self.state.applied.push(Applied {
            target,
            by,
            effect,
            until,
            spell: self.spell.map_or(u16::MAX, |cast| cast.row),
            id,
            at,
        });
    }

    /// A modifier effect, or one per damage type set in `types`
    /// (`DAMAGE_TYPE_*` bits: bit `n` is damage type row `n`).
    fn modifier(&mut self, modifies: Modifies, amount: i32, of: u8) -> Value {
        let amount = amount.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        self.effect_value(Effect::Modifier {
            modifies,
            amount,
            of,
        })
    }

    fn by_types(&mut self, modifies: Modifies, amount: i32, types: i32) -> Value {
        let amount = amount.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        let parts: Vec<Effect> = (0..crate::combat::sheet::DAMAGE_TYPES as u8)
            .filter(|row| types & (1 << row) != 0)
            .map(|of| Effect::Modifier {
                modifies,
                amount,
                of,
            })
            .collect();
        self.effect_value(Effect::Linked(parts))
    }

    /// The lasting effect a value names (`GetFirstEffect`'s), if on.
    fn slot(&self, args: &[Value], at: usize) -> Option<(ObjectId, Lasting)> {
        let Some(Effect::Slot { owner, id }) = self.effect_arg(args, at) else {
            return None;
        };
        let character = self.character(owner)?;
        // Its poison, as an effect of its poisoner's.
        if id == POISON_ID {
            return character.poisoned.map(|by| {
                let poison = Lasting {
                    modifies: Modifies::Poison,
                    amount: 0,
                    of: 0,
                    until: u64::MAX,
                    spell: u16::MAX,
                    by: by.0,
                    id,
                    at: 0,
                };
                (owner, poison)
            });
        }
        let lasting = character
            .lasting
            .into_iter()
            .flatten()
            .find(|l| l.id == id)?;
        Some((owner, lasting))
    }

    /// Whether `thing` is of a type in `filter` (`OBJECT_TYPE_*` bits; a
    /// player a creature).
    fn of_type(&self, thing: &Thing, filter: i32) -> bool {
        let kind = thing.kind.unwrap_or(ObjectKind::Creature);
        self.library
            .object_types
            .get(&kind)
            .is_some_and(|bits| bits & filter != 0)
    }

    /// Starts `caller`'s walk of `kind` over `objects`: the first of them.
    fn first(&mut self, caller: ObjectId, kind: u8, objects: Vec<ObjectId>) -> Value {
        let first = objects.first().copied();
        self.iterators.insert((caller.0, kind), (objects, 1));
        maybe(first)
    }

    /// The next of `caller`'s walk of `kind`.
    fn next(&mut self, caller: ObjectId, kind: u8) -> Value {
        let Some((objects, at)) = self.iterators.get_mut(&(caller.0, kind)) else {
            return maybe(None);
        };
        let next = objects.get(*at).copied();
        *at += 1;
        maybe(next)
    }

    /// The things within a `SHAPE_*` of `size` (metres) at `at`, nearest
    /// first: a sphere, a cube (half its side), a cylinder (across the
    /// floor), or a cone from `origin` toward `at` (within `size` of its
    /// axis, in front). Line of sight is not tested yet.
    fn in_shape(
        &self,
        shape: i32,
        size: f32,
        at: [f32; 3],
        origin: [f32; 3],
        filter: i32,
    ) -> Vec<ObjectId> {
        let named = |name: &str| self.library.constant(name) == Some(shape);
        let inside = |p: [f32; 3]| {
            let d: [f32; 3] = std::array::from_fn(|i| p[i] - at[i]);
            if named("SHAPE_SPHERE") {
                distance(p, at) <= size
            } else if named("SHAPE_CUBE") {
                d.iter().all(|v| v.abs() <= size)
            } else if named("SHAPE_SPELLCYLINDER") {
                flat(p, at) <= size
            } else {
                // A cone: along the axis from the origin, within `size` of it.
                let axis: [f32; 3] = std::array::from_fn(|i| at[i] - origin[i]);
                let to: [f32; 3] = std::array::from_fn(|i| p[i] - origin[i]);
                let length = (axis[0] * axis[0] + axis[1] * axis[1]).sqrt().max(0.01);
                let along = (to[0] * axis[0] + to[1] * axis[1]) / length;
                let across = (flat(p, origin).powi(2) - along * along).max(0.0).sqrt();
                along >= 0.0 && along <= length + size && across <= size
            }
        };
        let found = self
            .view
            .things
            .iter()
            .filter(|thing| self.of_type(thing, filter) && inside(thing.position))
            .map(|thing| (distance(thing.position, at), thing.id))
            .collect();
        nearest_first(found).collect()
    }

    /// `speaker` says `text` at `volume`: each listener within reach whose
    /// pattern fits hears it, its dialogue hook run with the pattern's
    /// number (`GetListenPatternNumber`) and the speaker
    /// (`GetLastSpeaker`). A creature with senses hears only within its
    /// hearing (its `ranges.2da` `secondaryrange`), as K1's exe hears a
    /// line only from one its perception has heard: a companion's
    /// silent shout in a fight no longer calls the Star Forge's light Jedi,
    /// 80 m off, away from the scene they stand in.
    fn speak(&mut self, speaker: ObjectId, text: &str, volume: i32) {
        let Some(from) = self.position(speaker) else {
            return;
        };
        let reach = volume_reach(self.library, volume);
        let listeners: Vec<(ObjectId, i32)> = self
            .state
            .listening
            .iter()
            .filter(|id| **id != speaker.0)
            .filter_map(|id| {
                let thing = self.view.thing(ObjectId(*id))?;
                let alive = thing.hp.is_none_or(|(hp, _)| hp > 0);
                let hearing = thing
                    .facts
                    .as_ref()
                    .and_then(|facts| facts.senses)
                    .filter(|_| thing.character.is_some())
                    .map_or(f32::MAX, |senses| senses.hearing);
                let near = distance(thing.position, from) <= reach.min(hearing);
                let number = self
                    .state
                    .patterns
                    .get(id)?
                    .iter()
                    .find(|(pattern, _)| heard(pattern, text))?
                    .1;
                (alive && near).then_some((thing.id, number))
            })
            .collect();
        for (listener, number) in listeners {
            if let Some(script) = self.view.hook(listener, Hook::Dialogue) {
                self.state
                    .last
                    .insert((listener.0, last::SPEAKER), speaker.0);
                let mut pending = Pending::new(&script, listener);
                pending.entering = Some(speaker);
                pending.number = number;
                self.state.pending.push(pending);
            }
        }
    }

    /// Changes the area's sound as this step's scripts leave it, and
    /// answers `then` of it.
    fn area_sound<T>(&mut self, change: impl FnOnce(&mut crate::snapshot::AreaSound) -> T) -> T {
        let mut sound = self.out.area_sound.unwrap_or(self.view.area_sound);
        let answer = change(&mut sound);
        self.out.area_sound = Some(sound);
        answer
    }

    pub(super) fn position(&self, id: ObjectId) -> Option<[f32; 3]> {
        self.view
            .thing(id)
            .map(|thing| thing.position)
            .or_else(|| self.lying(id).map(|lying| lying.at))
    }

    /// Where item `id` lies on this area's floor, if it does.
    fn lying(&self, id: ObjectId) -> Option<inventory::Lying> {
        self.state.lying(&self.view.module.name, id)
    }

    fn between(
        &self,
        a: ObjectId,
        b: ObjectId,
        measure: fn([f32; 3], [f32; 3]) -> f32,
    ) -> Option<f32> {
        Some(measure(self.position(a)?, self.position(b)?))
    }

    /// A location argument's place and facing.
    pub(super) fn place(&self, args: &[Value], at: usize) -> Option<([f32; 3], f32)> {
        self.state.location_at(handle(args, at))
    }

    /// `who`'s queue cleared (unless it is not commandable) and its fight
    /// over: a surrender's.
    fn stop(&mut self, who: ObjectId) {
        stop(self.state, self.out, self.view, who);
    }

    /// Whether `who`'s queue holds an animation of `LONG_ANIMATION_MS` or
    /// more playing `animation` for `ms`, so that this later copy is
    /// dropped. KOTOR compares the clip and its speed; the queue keeps the
    /// time the speed gave, not the speed [verify], and whether KOTOR also
    /// drops a copy queued while that one plays, as here [verify].
    fn queued_long(&self, who: ObjectId, (animation, ms): (u16, u32)) -> bool {
        let same = Action::Animate { animation, ms };
        ms >= LONG_ANIMATION_MS
            && (self.state.actions.get(&who.0))
                .is_some_and(|queue| queue.iter().any(|queued| queued.action == same))
    }

    /// Appends `action` to `who`'s queue, unless it is not commandable.
    pub(super) fn queue(&mut self, who: ObjectId, action: Action) -> Value {
        if self.takes(who, &action) {
            self.state.queue(who, action);
        }
        Value::Int(0)
    }

    /// Whether `who` takes `action`: it is commandable, and the action is
    /// not a driven companion's own AI (`own_ai_on_driven`).
    fn takes(&self, who: ObjectId, action: &Action) -> bool {
        !self.state.uncommandable.contains(&who.0) && !self.own_ai_on_driven(who, action)
    }

    /// Whether `action` is a driven companion's own AI fighting or moving
    /// it, which is skipped: KOTOR's controlled character runs no AI, its
    /// seat plays it. That is an attack, a talent, a move or a weapon swap
    /// (the party AI's `GN_CheckRangedPreferred`) queued by a run
    /// of its own AI (`Event::ai`: its event hooks and what they execute,
    /// `k_def_*`'s `k_ai_master`). A story script (a conversation's, the
    /// module's, an area's or a trigger's, another object's) still steers
    /// it.
    fn own_ai_on_driven(&self, who: ObjectId, action: &Action) -> bool {
        let driven = self.view.thing(who).is_some_and(|thing| {
            thing.kind.is_some() && thing.character.is_some_and(|c| c.driver.is_some())
        });
        let fights = matches!(
            action,
            Action::Attack { .. }
                | Action::Talent { .. }
                | Action::Move { .. }
                | Action::SwitchWeapons
        );
        driven && fights && self.event.ai == Some(who)
    }

    /// Where one standing at `from` flees `threat` to, `range` from it,
    /// and facing that way: straight away, else turned ever further from
    /// that (`rules::scripts::moves::away`'s steps) until a way leads
    /// there, else nearer; none when it is that far already or no way
    /// leads anywhere tried.
    fn away(&self, from: [f32; 3], threat: [f32; 3], range: f32) -> Option<([f32; 3], f32)> {
        use rules::scripts::moves::away;
        if flat(from, threat) >= range {
            return None;
        }
        let ahead = crate::math::atan2_deg(from[1] - threat[1], from[0] - threat[0]);
        let turns = (0..away::TURNS)
            .map(|step: i32| (step + 1) / 2 * away::TURN_STEP * if step % 2 == 0 { 1 } else { -1 });
        [range, range / away::SHORT_DIVISOR]
            .into_iter()
            .filter(|far| *far > flat(from, threat))
            .flat_map(|far| turns.clone().map(move |turn| (far, ahead + turn as f32)))
            .map(|(far, yaw)| {
                let (sin, cos) = crate::math::sin_cos_deg(yaw);
                ([threat[0] + cos * far, threat[1] + sin * far, from[2]], yaw)
            })
            .find(|(at, _)| (self.view.leads)(from, *at))
    }

    /// The weapon `who` has (worn in `slot` or carried) for `slot` whose
    /// blow deals the most on average, of the kind asked for (ranged or
    /// melee), else of the other: what it wears first, then the first
    /// carried; `None` when that is what it wears, or it has none.
    fn most_damaging(&self, who: ObjectId, ranged: bool, slot: u8) -> Option<ObjectId> {
        let catalog = self.library.items();
        let worn = self.state.in_slot(who.0, slot);
        let carried = self.state.carried(items::holder(self.view, who));
        let weapons: Vec<(ObjectId, crate::combat::Weapon)> = worn
            .into_iter()
            .chain(carried)
            .filter_map(|id| {
                let item = self.state.items.get(&id.0)?;
                let facts = self.state.item_facts(catalog, &item.template)?;
                (facts.slots & 1 << slot != 0).then_some((id, facts.weapon?))
            })
            .collect();
        let kind = |wanted: bool| {
            weapons
                .iter()
                .filter(|(_, weapon)| weapon.delivery.ranged() == wanted)
                .fold(None, |best: Option<&(ObjectId, _)>, next| match best {
                    Some(best) if twice_average(&best.1) >= twice_average(&next.1) => Some(best),
                    _ => Some(next),
                })
        };
        let best = kind(ranged).or_else(|| kind(!ranged))?.0;
        (Some(best) != worn).then_some(best)
    }

    fn call(&mut self, bound: Bound, caller: ObjectId, args: &[Value]) -> Value {
        use Bound::*;
        let view = self.view;
        let now = view.now_ms;
        let state = &mut *self.state;
        match bound {
            // A global the catalogue declares of its kind (`declared`): one
            // it lacks reads as nothing and never sets, as KOTOR's. A co-op
            // race's run keeps its own (`minigame::overlaid`, M10).
            GetGlobalNumber => {
                let name = text(args, 0);
                let own = super::minigame::overlaid(self, caller, |o| &o.numbers, name);
                Value::Int(
                    own.or_else(|| global(&self.state.numbers, name).copied())
                        .filter(|_| declared(view.module, name, Global::Number))
                        .unwrap_or(0),
                )
            }
            SetGlobalNumber => {
                // A number global holds a signed byte (-128 to +127, as
                // nwscript.nss says): a value past it keeps its low byte.
                let name = text(args, 0);
                if declared(view.module, name, Global::Number) {
                    let value = i32::from(int(args, 1) as i8);
                    if let Err(value) =
                        super::minigame::overlay(self, caller, |o| &mut o.numbers, name, value)
                    {
                        set_global(&mut self.state.numbers, name, value);
                    }
                } else {
                    self.trace(&format!("  {name}: not a number global (globalcat.2da)"));
                }
                Value::Int(0)
            }
            IncrementGlobalNumber | DecrementGlobalNumber => {
                // A sum past the byte fails (the game warns) and the global
                // keeps its value.
                let (name, by) = (text(args, 0), int(args, 1));
                let by = if bound == IncrementGlobalNumber {
                    by
                } else {
                    by.saturating_neg()
                };
                let own = super::minigame::overlaid(self, caller, |o| &o.numbers, name);
                let was = own
                    .or_else(|| global(&self.state.numbers, name).copied())
                    .unwrap_or(0);
                match was
                    .checked_add(by)
                    .filter(|sum| (-128..=127).contains(sum))
                    .filter(|_| declared(view.module, name, Global::Number))
                {
                    Some(sum) => {
                        if let Err(sum) =
                            super::minigame::overlay(self, caller, |o| &mut o.numbers, name, sum)
                        {
                            set_global(&mut self.state.numbers, name, sum);
                        }
                    }
                    None => self.trace(&format!("  {name}: {was} {by:+} is out of range")),
                }
                Value::Int(0)
            }
            GetGlobalBoolean => {
                let name = text(args, 0);
                let own = super::minigame::overlaid(self, caller, |o| &o.booleans, name);
                truth(
                    own.or_else(|| global(&self.state.booleans, name).copied())
                        .filter(|_| declared(view.module, name, Global::Boolean))
                        .unwrap_or(false),
                )
            }
            SetGlobalBoolean => {
                let name = text(args, 0);
                if declared(view.module, name, Global::Boolean) {
                    let value = int(args, 1) != 0;
                    if let Err(value) =
                        super::minigame::overlay(self, caller, |o| &mut o.booleans, name, value)
                    {
                        set_global(&mut self.state.booleans, name, value);
                    }
                } else {
                    self.trace(&format!("  {name}: not a boolean global (globalcat.2da)"));
                }
                Value::Int(0)
            }
            GetGlobalString => {
                let name = text(args, 0);
                let own = super::minigame::overlaid(self, caller, |o| &o.strings, name);
                string(
                    own.or_else(|| global(&self.state.strings, name).cloned())
                        .filter(|_| declared(view.module, name, Global::String))
                        .unwrap_or_else(String::new),
                )
            }
            SetGlobalString => {
                let name = text(args, 0);
                if declared(view.module, name, Global::String) {
                    let value = text(args, 1).to_string();
                    if let Err(value) =
                        super::minigame::overlay(self, caller, |o| &mut o.strings, name, value)
                    {
                        set_global(&mut self.state.strings, name, value);
                    }
                } else {
                    self.trace(&format!("  {name}: not a string global (globalcat.2da)"));
                }
                Value::Int(0)
            }
            GetGlobalLocation => match global(&state.places, text(args, 0))
                .filter(|_| declared(view.module, text(args, 0), Global::Location))
            {
                Some(&(at, yaw)) => Value::Location(state.location(at, yaw)),
                None => Value::Location(state.location([0.0; 3], 0.0)),
            },
            SetGlobalLocation => {
                let name = text(args, 0);
                match self.place(args, 1) {
                    Some(place) if declared(view.module, name, Global::Location) => {
                        set_global(&mut self.state.places, name, place);
                    }
                    _ => self.trace(&format!("  {name}: not a location global (globalcat.2da)")),
                }
                Value::Int(0)
            }
            // An object that is not (OBJECT_INVALID) keeps no locals: TSL's
            // executable's commands set nothing and read 0 when
            // `GetGameObject` finds none [verify in K1's].
            GetLocalBoolean | GetLocalNumber | SetLocalBoolean | SetLocalNumber
                if object(args, 0) == ObjectId::INVALID =>
            {
                Value::Int(0)
            }
            // A local boolean is a bit of the game's slots (`local_boolean`):
            // nothing kept past the last, which reads false. A true one reads
            // as its bit lies in its 32-bit word (`local_boolean_read`).
            GetLocalBoolean => {
                let index = int(args, 1);
                let key = (
                    local_owner(&self.view.module.name, object(args, 0)),
                    0,
                    index,
                );
                let kept = local_boolean(&view.module.game, index)
                    .then(|| state.locals.get(&key).copied())
                    .flatten();
                Value::Int(local_boolean_read(index, kept.unwrap_or(0) != 0))
            }
            // A local number is a byte in one of the game's slots
            // (`local_number`): read back unsigned, set only from -128 to
            // 127, nothing kept past the last slot.
            GetLocalNumber => {
                let key = (
                    local_owner(&self.view.module.name, object(args, 0)),
                    1,
                    int(args, 1),
                );
                let kept = state.locals.get(&key).copied().unwrap_or(0);
                Value::Int(local_number(&view.module.game, int(args, 1), kept).unwrap_or(0))
            }
            SetLocalNumber => {
                let key = (
                    local_owner(&self.view.module.name, object(args, 0)),
                    1,
                    int(args, 1),
                );
                let value = int(args, 2);
                if (-128..=127).contains(&value)
                    && let Some(byte) = local_number(&self.view.module.game, int(args, 1), value)
                {
                    state.locals.insert(key, byte);
                }
                Value::Int(0)
            }
            SetLocalBoolean => {
                let key = (
                    local_owner(&self.view.module.name, object(args, 0)),
                    0,
                    int(args, 1),
                );
                if local_boolean(&view.module.game, int(args, 1)) {
                    state.locals.insert(key, i32::from(int(args, 2) != 0));
                }
                Value::Int(0)
            }
            // No tag is the player's (its creature has none): reone's
            // `GetObjectByTag`; Trask's walk to the player at the Endar
            // Spire's start went to an untagged object elsewhere.
            GetObjectByTag if text(args, 0).is_empty() => {
                Value::Object(self.pcs().first().copied().unwrap_or(ObjectId::INVALID))
            }
            GetObjectByTag => Value::Object(self.by_tag(text(args, 0), int(args, 1), None)),
            GetWaypointByTag => {
                Value::Object(self.by_tag(text(args, 0), 0, Some(ObjectKind::Waypoint)))
            }
            GetNearestObjectByTag => {
                Value::Object(self.nearest_by_tag(text(args, 0), object(args, 1), int(args, 2)))
            }
            // The tag as the games keep it: lowercased as each object loads
            // (both exes' loaders lower an item's, creature's, door's,
            // placeable's, waypoint's, sound's, store's, encounter's and
            // area's `Tag`), but a trigger's as its data writes it (TSL's
            // `LoadTrigger` keeps it). Scripts compare it without case
            // (`script_ncs::vm`).
            GetTag => {
                let id = object(args, 0);
                match state.items.get(&id.0) {
                    Some(item) => string(
                        state
                            .item_facts(self.library.items(), &item.template)
                            .map_or_else(String::new, |facts| facts.tag.clone()),
                    ),
                    None => string(self.instance(id).map_or("", |facts| match facts.kind {
                        ObjectKind::Trigger => facts.written_tag(),
                        _ => facts.tag.as_str(),
                    })),
                }
            }
            GetIsObjectValid => {
                let id = object(args, 0);
                truth(
                    id == MODULE
                        || id == AREA
                        || state.items.contains_key(&id.0)
                        || self.exists(id),
                )
            }
            GetFirstPC => self.first(caller, walk::PCS, self.controlled_pcs()),
            GetNextPC => self.next(caller, walk::PCS),
            // The first seat's PC leads, or the companion made leader
            // (`SetPartyLeader`, `Action::SetLeader`), which the first seat
            // then moves (`world::lent`); in a script a guest's act raised,
            // the companion it drives (`Event::leads`).
            GetPartyLeader => maybe(self.party().first().copied()),
            GetIsPartyLeader => truth(self.party().first() == Some(&object(args, 0))),
            GetIsPlayerMadeCharacter => truth(view.party.contains(&object(args, 0))),
            // A seat's commands act at once and are never queued (C14): no
            // one has a user's actions waiting.
            GetUserActionsPending => Value::Int(0),
            ObjectToString => string(format!("{:x}", object(args, 0).0)),
            GetPartyMemberByIndex => Value::Object(
                usize::try_from(int(args, 0))
                    .ok()
                    .and_then(|at| self.party().get(at).copied())
                    .unwrap_or(ObjectId::INVALID),
            ),
            // A player, the companion leading (the one KOTOR's player
            // controls: T3-M4 at the Sith base's door), or the one the first
            // seat is switched to in place of its own character.
            GetIsPC => truth(self.is_pc(object(args, 0))),
            GetModule => Value::Object(MODULE),
            GetModuleName => string(view.module.title.as_str()),
            GetTimeHour | GetTimeMinute | GetTimeSecond | GetTimeMillisecond => {
                Value::Int(clock(view.now_ms, view.module.minutes_per_hour, bound))
            }
            // With its death screen (the default, which K1's older calls
            // take) the game is lost: the turret's hull, the krayt dragon.
            // Without it the game is over and the players go to the main
            // menu: K1's ending once its credits started, Manaan's sentence.
            EndGame => {
                if int(args, 0) != 0 {
                    self.out.lost = true;
                    Value::Int(0)
                } else {
                    self.tell(None, Notice::Show(Screen::End))
                }
            }
            // The credits roll on each player's screen; the game the scripts
            // end under them (`EndGame` once none rolls) is over for a player
            // when its own have rolled.
            StartCreditSequence => self.tell(None, Notice::Show(Screen::Credits)),
            IsCreditSequenceInProgress => truth(false),
            // A player's character has no name yet (no character creation).
            GetName => {
                let id = object(args, 0);
                let name = match id {
                    MODULE => Some(view.module.title.clone()),
                    AREA => Some(view.module.area_title.clone()),
                    _ => match state.items.get(&id.0) {
                        Some(item) => state
                            .item_facts(self.library.items(), &item.template)
                            .and_then(|facts| self.library.text(facts.name)),
                        None => self
                            .instance(id)
                            .map(|data| data.name.clone())
                            .or_else(|| self.persona(id).map(|persona| persona.name.clone())),
                    },
                };
                string(name.unwrap_or_else(String::new))
            }
            GetObjectPersonalSpace => Value::Float(
                self.instance(object(args, 0))
                    .map_or(0.0, |data| data.personal_space),
            ),
            RemoveHeartbeat => {
                state.heartless.insert(object(args, 0).0);
                Value::Int(0)
            }
            GetStringByStrRef => string(
                u32::try_from(int(args, 0))
                    .ok()
                    .and_then(|strref| self.library.text(strref))
                    .unwrap_or_else(String::new),
            ),
            GetArea => Value::Object(AREA),
            GetObjectType if state.items.contains_key(&object(args, 0).0) => {
                Value::Int(self.library.item_type)
            }
            GetObjectType => {
                let kind = view
                    .thing(object(args, 0))
                    .map(|thing| thing.kind.unwrap_or(ObjectKind::Creature));
                Value::Int(
                    kind.and_then(|kind| self.library.object_types.get(&kind).copied())
                        .unwrap_or(0),
                )
            }
            // No object is dead, as in the Aurora engine (NWN Lexicon): a
            // body faded, its creature gone, still counts (Korriban's
            // tomb opens on Yuthura's death only if Uthar's, long faded,
            // is dead).
            GetIsDead => truth(
                view.thing(object(args, 0))
                    .is_none_or(|thing| thing.hp.is_some_and(|(now, _)| now <= 0)),
            ),
            GetIsOpen => truth(
                view.thing(object(args, 0))
                    .and_then(|thing| thing.open)
                    .is_some_and(|open| open != 0),
            ),
            GetLocked => truth(
                view.thing(object(args, 0))
                    .and_then(|thing| thing.locked)
                    .unwrap_or(false),
            ),
            SetLocked => {
                self.out.changes.push(Change::Lock {
                    door: object(args, 0),
                    locked: int(args, 1) != 0,
                });
                Value::Int(0)
            }
            SetMapPinEnabled => {
                self.out.changes.push(Change::MapPin {
                    pin: object(args, 0),
                    enabled: int(args, 1) != 0,
                });
                Value::Int(0)
            }
            // The party's map, whoever it is for: every seat shares it.
            ExploreAreaForPlayer => {
                self.out
                    .changes
                    .push(Change::Reveal(crate::fog::Reveal::All));
                Value::Int(0)
            }
            // A grid radius round a point; none (the default, -1) the whole.
            RevealMap => {
                let ([x, y, _], radius) = (vector(args, 0), int(args, 1));
                self.out
                    .changes
                    .push(Change::Reveal(match u32::try_from(radius) {
                        Ok(radius) => crate::fog::Reveal::Around { at: [x, y], radius },
                        Err(_) => crate::fog::Reveal::All,
                    }));
                Value::Int(0)
            }
            GetPosition => Value::Vector(self.position(object(args, 0)).unwrap_or([0.0; 3])),
            GetFacing => Value::Float(
                view.thing(object(args, 0))
                    .map(|thing| thing.yaw)
                    .or_else(|| self.lying(object(args, 0)).map(|lying| lying.yaw))
                    .unwrap_or(0.0),
            ),
            GetLocation => {
                // A minigame's thing stands where its course has it.
                let id = object(args, 0);
                let place = view
                    .thing(id)
                    .map(|t| (t.position, t.yaw))
                    .or_else(|| {
                        let lying = state.lying(&view.module.name, id)?;
                        Some((lying.at, lying.yaw))
                    })
                    .or_else(|| view.minigame_place(id).map(|at| (at, 0.0)));
                let (at, yaw) = place.unwrap_or(([0.0; 3], 0.0));
                Value::Location(state.location(at, yaw))
            }
            Location => Value::Location(state.location(vector(args, 0), float(args, 1))),
            GetPositionFromLocation => {
                Value::Vector(self.place(args, 0).map_or([0.0; 3], |(at, _)| at))
            }
            GetFacingFromLocation => Value::Float(self.place(args, 0).map_or(0.0, |(_, yaw)| yaw)),
            GetDistanceBetween => Value::Float(
                self.between(object(args, 0), object(args, 1), distance)
                    .unwrap_or(0.0),
            ),
            GetDistanceToObject => Value::Float(
                self.between(object(args, 0), caller, distance)
                    .unwrap_or(-1.0),
            ),
            GetDistanceBetween2D => Value::Float(
                self.between(object(args, 0), object(args, 1), flat)
                    .unwrap_or(0.0),
            ),
            GetDistanceToObject2D => {
                Value::Float(self.between(object(args, 0), caller, flat).unwrap_or(-1.0))
            }
            GetDistanceBetweenLocations => {
                let (a, b) = (self.place(args, 0), self.place(args, 1));
                Value::Float(a.zip(b).map_or(0.0, |((a, _), (b, _))| distance(a, b)))
            }
            GetCurrentHitPoints | GetMaxHitPoints => {
                // A creature's fighter's; a door's or placeable's, its data's.
                let id = object(args, 0);
                let hp = view
                    .thing(id)
                    .and_then(|thing| thing.hp)
                    .or_else(|| self.instance(id).and_then(|facts| facts.hit_points));
                Value::Int(hp.map_or(0, |(now, most)| {
                    if bound == GetCurrentHitPoints {
                        now
                    } else {
                        most
                    }
                }))
            }
            // The module's acquire hook's item rides as its event's
            // entering one, as does a clicked trigger's clicker.
            // An encounter's last to come in, whatever its event
            // (`LastEntered`).
            GetEnteringObject if crate::spawner::of(view, caller).is_some() => maybe(
                state
                    .encounters
                    .get(&caller.0)
                    .and_then(|spawn| spawn.last_entered)
                    .map(ObjectId),
            ),
            // The module's acquire hook asked who came by it: the item's
            // possessor, as TSL's own acquire scripts read it
            // (`GetEnteringObject() == GetFirstPC()`: 402DXN's k_check_item
            // for the zakkeg's ear, 404DXN's k_get_item), K1's asking for the
            // item itself by `GetModuleItemAcquired` [verify in the exe].
            GetEnteringObject
                if caller == crate::script::MODULE
                    && let Some(item) = self.event.entering
                    && state.items.contains_key(&item.0) =>
            {
                Value::Object(possessor(state, view, item))
            }
            GetEnteringObject
            | GetExitingObject
            | GetModuleItemAcquired
            | GetModuleItemLost
            | GetClickingObject => maybe(self.event.entering),
            GetLastOpenedBy | GetLastUsedBy | GetLastClosedBy | GetLastDisturbed
            | GetLastKiller | GetLastDamager | GetLastAttacker | GetLastUnlocked
            | GetLastLocked => {
                let kind = match bound {
                    GetLastOpenedBy => last::OPENED,
                    GetLastUnlocked => last::UNLOCKED,
                    GetLastLocked => last::LOCKED,
                    GetLastUsedBy => last::USED,
                    GetLastClosedBy => last::CLOSED,
                    GetLastDisturbed => last::DISTURBED,
                    GetLastKiller => last::KILLER,
                    GetLastAttacker => last::ATTACKER,
                    _ => last::DAMAGER,
                };
                // `GetLastAttacker` and `GetLastDamager` name the object;
                // the others ask of the caller.
                let of = match args.first() {
                    Some(Value::Object(id)) => *id,
                    _ => caller,
                };
                maybe(state.last.get(&(of.0, kind)).copied().map(ObjectId))
            }
            GetLastPerceived => maybe(
                state
                    .last
                    .get(&(caller.0, last::PERCEIVED))
                    .copied()
                    .map(ObjectId),
            ),
            GetLastPerceptionSeen
            | GetLastPerceptionVanished
            | GetLastPerceptionHeard
            | GetLastPerceptionInaudible => {
                let kind = match bound {
                    GetLastPerceptionSeen => perception::SEEN,
                    GetLastPerceptionVanished => perception::VANISHED,
                    GetLastPerceptionHeard => perception::HEARD,
                    _ => perception::INAUDIBLE,
                };
                let last = state.last.get(&(caller.0, last::PERCEPTION));
                truth(last == Some(&u32::from(kind)))
            }
            GetObjectSeen | GetObjectHeard => {
                let target = object(args, 0).0;
                truth(state.perceived.get(&object(args, 1).0).is_some_and(|p| {
                    if bound == GetObjectSeen {
                        p.seen.contains(&target)
                    } else {
                        p.heard.contains(&target)
                    }
                }))
            }
            GetAttackTarget | GetAttemptedAttackTarget => {
                let who = if bound == GetAttackTarget {
                    object(args, 0)
                } else {
                    caller
                };
                maybe(self.character(who).and_then(|c| c.target))
            }
            GetNearestCreature => Value::Object(self.nearest_creature(args)),
            // In combat, fake combat (`SetFakeCombatState`) too unless TSL's
            // second argument asks for real combat only.
            GetIsInCombat => {
                let only_real = args.len() > 1 && int(args, 1) != 0;
                truth(
                    self.character(object(args, 0))
                        .is_some_and(|c| c.in_combat && !(only_real && c.fake_combat)),
                )
            }
            ActionAttack => {
                let target = object(args, 0);
                if target == ObjectId::INVALID {
                    Value::Int(0)
                } else {
                    let passive = int(args, 1) != 0;
                    self.queue(caller, Action::Attack { target, passive })
                }
            }
            GetIsEnemy | GetIsFriend | GetIsNeutral => {
                let wanted = match bound {
                    GetIsEnemy => Regard::Enemy,
                    GetIsFriend => Regard::Friend,
                    _ => Regard::Neutral,
                };
                truth(self.regard(object(args, 1), object(args, 0)) == Some(wanted))
            }
            // How the source's faction regards the target's (0 to 100);
            // −1 unless both are characters (only they have factions here).
            GetReputation => {
                let (source, target) = (
                    self.character(object(args, 0)),
                    self.character(object(args, 1)),
                );
                let repute = view.module.repute.with(&self.state.reputation);
                Value::Int(source.zip(target).map_or(-1, |(source, target)| {
                    i32::from(repute.of(source.faction, target.faction))
                }))
            }
            // How the source's faction and the target's stand changed, both
            // ways (`module::Adjustments`): Tatooine's Sand People made
            // peaceable by one in their robes, hostile again without.
            AdjustReputation => {
                if let (Some(target), Some(source)) =
                    (self.faction(object(args, 0)), self.faction(object(args, 1)))
                {
                    let by = int(args, 2).clamp(-100, 100) as i16;
                    let pair = crate::module::pair(source, target);
                    let now = self.state.reputation.entry(pair).or_insert(0);
                    *now = (*now + by).clamp(-100, 100);
                    if *now == 0 {
                        self.state.reputation.remove(&pair);
                    }
                }
                Value::Int(0)
            }
            GetStandardFaction => Value::Int(self.faction(object(args, 0)).map_or(-1, i32::from)),
            // Whether two are of one faction (both characters).
            GetFactionEqual => {
                let (first, second) =
                    (self.faction(object(args, 0)), self.faction(object(args, 1)));
                truth(first.is_some() && first == second)
            }
            // How the source's faction regards the target (its members are
            // one faction here, so its average is the faction's); −1 unless
            // both are characters.
            GetFactionAverageReputation => {
                let repute = view.module.repute.with(&self.state.reputation);
                let (source, target) =
                    (self.faction(object(args, 0)), self.faction(object(args, 1)));
                Value::Int(
                    source
                        .zip(target)
                        .map_or(-1, |(source, target)| i32::from(repute.of(source, target))),
                )
            }
            // The module's resref (`GetModuleName` is its title).
            GetModuleFileName => string(view.module.name.as_str()),
            // No downloadable content here (KOTOR's Xbox Live check).
            GetIsLiveContentAvailable => Value::Int(0),
            // The PC build (TSL's scripts pick their mouse-and-keyboard
            // lines by it), and no pad to rumble.
            GetIsXBox | PlayRumblePattern | StopRumblePattern => Value::Int(0),
            // How often the original stepped an object or a mind far from
            // the player (TSL's budget): here every one is stepped every
            // tick, so these change nothing.
            SetCreatureAILevel | ResetCreatureAILevel => Value::Int(0),
            // TSL's (its AI's gunners finding a place to shoot from): sight
            // between the two places as given (scripts pass positions: the
            // feet), as its executable traces them, through the doors as
            // they stand; raised `SIGHT_CLEAR_M` only, so the floor they
            // stand on never counts (the executable's walkable faces do
            // not). [verify] that what blocks its sight (non-walkable
            // walkmesh faces, its rooms' line-of-sight surfaces) is what our
            // solid geometry is.
            HasLineOfSight => {
                let lift = |at: [f32; 3]| [at[0], at[1], at[2] + SIGHT_CLEAR_M];
                truth((view.sees)(lift(vector(args, 0)), lift(vector(args, 1))))
            }
            // TSL's (`GetRandomDestination`): a place up to `range` metres
            // along each axis from the creature, as its executable picks
            // it (`random_destination`); (0, 0, 0) for one not a creature.
            GetRandomDestination => {
                let who = object(args, 0);
                match self.character(who).and(self.position(who)) {
                    Some(from) => Value::Vector(super::force::random_destination(
                        from,
                        int(args, 1),
                        &mut |below| self.rng.below(below),
                        (view.floor, view.walks),
                    )),
                    None => Value::Vector([0.0; 3]),
                }
            }
            // TSL's: an object drawn or not. Every object is drawn here, so
            // turning it on changes nothing (`k_fab_fade_sp` does, for the
            // creatures it fades in); turning it off is still a gap.
            EnableRendering => {
                if int(args, 1) == 0 {
                    *self
                        .report
                        .gaps
                        .entry("EnableRendering (off)".into())
                        .or_insert(0) += 1;
                }
                Value::Int(0)
            }
            // Whether the party may not leave the area for the Ebon Hawk
            // (`SetAreaUnescapable`: Korriban's tomb, the Star Forge).
            GetAreaUnescapable => truth(state.unescapable.contains(&key(&view.module.name))),
            SetAreaUnescapable => {
                let name = key(&view.module.name);
                if int(args, 0) != 0 {
                    state.unescapable.insert(name);
                } else {
                    state.unescapable.remove(&name);
                }
                Value::Int(0)
            }
            GetRacialType => Value::Int(
                self.character(object(args, 0))
                    .map(|c| i32::from(c.race))
                    .or_else(|| self.library.constant("RACIAL_TYPE_INVALID"))
                    .unwrap_or(-1),
            ),
            GetHitDice => Value::Int(
                self.character(object(args, 0))
                    .map_or(0, |c| i32::from(c.level)),
            ),
            // Known and paid for now, as K1's exe has it, not known alone:
            // the AI asks it before each power it queues (evil Bastila, 1 FP,
            // queued one she could not cast every round and never fought).
            GetHasSpell => {
                let row = u16::try_from(int(args, 0)).ok();
                truth(row.is_some_and(|row| self.can_cast(object(args, 1), row)))
            }
            IsObjectPartyMember => truth(self.character(object(args, 0)).is_some_and(|c| c.party)),
            FindSubString => {
                let (text, part) = (text(args, 0), text(args, 1));
                let start = usize::try_from(int(args, 2)).unwrap_or(0);
                let chars: Vec<char> = text.chars().collect();
                let part: Vec<char> = part.chars().collect();
                let at = (start..=chars.len().saturating_sub(part.len()))
                    .find(|&at| chars.get(at..at + part.len()) == Some(&part[..]));
                Value::Int(at.map_or(-1, |at| at as i32))
            }
            TalentSpell => talent_value(u16::try_from(int(args, 0)).ok().map(Use::Power)),
            TalentFeat => talent_value(u16::try_from(int(args, 0)).ok().map(Use::Feat)),
            // Skills are not talents the sim uses.
            TalentSkill => talent_value(None),
            GetIsTalentValid => truth(talent(args, 0).is_some()),
            GetTypeFromTalent => {
                Value::Int(talent(args, 0).map_or(-1, |talent| talent_type(self.library, talent)))
            }
            GetIdFromTalent => Value::Int(match talent(args, 0) {
                Some(Use::Power(row) | Use::Feat(row)) => i32::from(row),
                None => -1,
            }),
            GetCategoryFromTalent => Value::Int(
                talent(args, 0).map_or(0, |talent| i32::from(self.facts(talent).category)),
            ),
            GetCreatureHasTalent => truth(
                talent(args, 0).is_some_and(|talent| self.has_talent(object(args, 1), talent)),
            ),
            GetCreatureTalentBest => {
                // The best: the highest challenge rating within the cap.
                let found =
                    self.talents_in(object(args, 2), int(args, 0), (int(args, 4), int(args, 5)));
                let cap = int(args, 1);
                let best = found
                    .iter()
                    .filter(|(_, maxcr)| i32::from(*maxcr) <= cap)
                    .fold(
                        None,
                        |best: Option<(Use, u8)>, &(talent, maxcr)| match best {
                            Some((_, most)) if most >= maxcr => best,
                            _ => Some((talent, maxcr)),
                        },
                    );
                talent_value(best.map(|(talent, _)| talent))
            }
            GetCreatureTalentRandom => {
                let found = self.talents_in(object(args, 1), int(args, 0), (-1, -1));
                let picked = (!found.is_empty())
                    .then(|| found[self.rng.below(found.len() as u32) as usize].0);
                talent_value(picked)
            }
            ActionUseTalentOnObject | ActionUseTalentAtLocation => {
                // At a location: the mind uses it at whom it fights.
                let target = if bound == ActionUseTalentOnObject {
                    object(args, 1)
                } else {
                    caller
                };
                match talent(args, 0) {
                    Some(talent) if target != ObjectId::INVALID => {
                        self.queue(caller, Action::Talent { talent, target })
                    }
                    _ => Value::Int(0),
                }
            }
            GetLastHostileTarget => maybe(self.character(object(args, 0)).and_then(|c| c.target)),
            GetIsDebilitated => truth(
                self.character(object(args, 0))
                    .is_some_and(|c| c.debilitated),
            ),
            GetLevelByClass => Value::Int(self.character(object(args, 1)).map_or(0, |c| {
                c.classes
                    .iter()
                    .filter(|(class, _)| i32::from(*class) == int(args, 0))
                    .map(|(_, levels)| i32::from(*levels))
                    .sum()
            })),
            GetClassByPosition => {
                // Positions count from 1; CLASS_TYPE_INVALID past its classes.
                let at = usize::try_from(int(args, 0) - 1).ok();
                let class = at
                    .zip(self.character(object(args, 1)))
                    .and_then(|(at, c)| c.classes.get(at).copied())
                    .filter(|(_, levels)| *levels > 0);
                let invalid = || self.library.constant("CLASS_TYPE_INVALID").unwrap_or(-1);
                Value::Int(class.map_or_else(invalid, |(class, _)| i32::from(class)))
            }
            GetLastSpellCaster => maybe(
                state
                    .last
                    .get(&(caller.0, last::SPELL_CASTER))
                    .copied()
                    .map(ObjectId),
            ),
            GetLastSpell | GetLastSpellHarmful => {
                let kind = if bound == GetLastSpell {
                    last::SPELL
                } else {
                    last::SPELL_HARMFUL
                };
                Value::Int(state.last.get(&(caller.0, kind)).map_or(-1, |v| *v as i32))
            }
            // Its last attack action (`script::Step::attacking`): the AI's
            // round reads its last talent's code by it (`k_inc_generic`).
            GetLastAttackAction => {
                let name = match state.last.get(&(object(args, 0).0, last::ATTACK_ACTION)) {
                    Some(&last::ATTACK_POWER) => "ACTION_CASTSPELL",
                    Some(_) => "ACTION_ATTACKOBJECT",
                    None => "ACTION_INVALID",
                };
                self.action_value(name)
            }
            // Its last blow's result as `ATTACK_RESULT_*` names it (a
            // parried or deflected one as the game counts them; none yet:
            // ATTACK_RESULT_INVALID).
            GetLastAttackResult => {
                use crate::combat::Outcome;
                let kept = state.last.get(&(object(args, 0).0, last::ATTACK_RESULT));
                let name = match kept.copied() {
                    Some(o) if o == Outcome::Hit as u32 => "ATTACK_RESULT_HIT_SUCCESSFUL",
                    Some(o) if o == Outcome::Critical as u32 => "ATTACK_RESULT_CRITICAL_HIT",
                    Some(o) if o == Outcome::Automatic as u32 => "ATTACK_RESULT_AUTOMATIC_HIT",
                    Some(o) if o == Outcome::Parried as u32 => "ATTACK_RESULT_PARRIED",
                    Some(o) if o == Outcome::Deflected as u32 => "ATTACK_RESULT_DEFLECTED",
                    Some(o) if o == Outcome::Refused as u32 => "ATTACK_RESULT_ATTACK_RESISTED",
                    _ => "ATTACK_RESULT_INVALID",
                };
                Value::Int(self.library.constant(name).unwrap_or(0))
            }
            // What `appearance.2da` has of a creature's body (`module::Body`):
            // its row (0 for none), its size (`CREATURE_SIZE_INVALID`), its
            // speed (`MOVEMENT_SPEED_IMMOBILE` for no creature, as the
            // game documents).
            GetAppearanceType | GetCreatureSize | GetCreatureMovmentType => {
                let who = object(args, 0);
                let body = self.instance(who).and_then(|facts| facts.body);
                // A character with no template here (a player's body): a
                // medium creature moving at the PC's speed, as KOTOR's
                // player is, of its persona's appearance; no creature at
                // all is immobile.
                let player = self
                    .persona(who)
                    .map(|persona| i32::from(persona.appearance));
                let character = self.character(who).is_some();
                let named =
                    |name: &str, fallback: i32| self.library.constant(name).unwrap_or(fallback);
                // A worn item's disguise stands for its appearance.
                let disguise = self.character(who).and_then(|c| c.sheet.gear.disguise);
                // One a script changed it to stands for its template's.
                let changed = self.character(who).and_then(|c| c.look.appearance);
                Value::Int(match (bound, body) {
                    (GetAppearanceType, _) if let Some(row) = disguise => i32::from(row),
                    (GetAppearanceType, _) if let Some(row) = changed => i32::from(row),
                    (GetAppearanceType, Some(body)) => i32::from(body.appearance),
                    (GetAppearanceType, None) => player.unwrap_or(0),
                    (GetCreatureSize, Some(body)) => i32::from(body.size),
                    (GetCreatureMovmentType, Some(body)) => i32::from(body.movement),
                    (GetCreatureSize, None) if character => named("CREATURE_SIZE_MEDIUM", 3),
                    (GetCreatureMovmentType, None) if character => named("MOVEMENT_SPEED_PC", 0),
                    (GetCreatureMovmentType, None) => named("MOVEMENT_SPEED_IMMOBILE", 1),
                    _ => 0,
                })
            }
            GetWasForcePowerSuccessful => {
                truth(state.last.get(&(object(args, 0).0, last::POWER_LANDED)) == Some(&1))
            }
            GetLastForcePowerUsed | GetLastCombatFeatUsed => {
                let kind = if bound == GetLastForcePowerUsed {
                    last::FORCE_POWER
                } else {
                    last::COMBAT_FEAT
                };
                let row = state.last.get(&(object(args, 0).0, kind));
                Value::Int(row.map_or(-1, |row| *row as i32))
            }
            GetTotalDamageDealt => Value::Int(
                state
                    .last
                    .get(&(caller.0, last::DAMAGE))
                    .map_or(0, |hp| *hp as i32),
            ),
            EffectDamage => self.effect_value(Effect::Damage {
                amount: effects::amount(int(args, 0)),
                kind: effects::amount(int(args, 1)),
            }),
            EffectHeal => self.effect_value(Effect::Heal(effects::amount(int(args, 0)))),
            // Its third argument, `nNoFadeAway`, keeps the body.
            EffectDeath => self.effect_value(match int(args, 2) {
                0 => Effect::Death,
                _ => Effect::DeathKept,
            }),
            EffectHealForcePoints => {
                self.effect_value(Effect::HealForcePoints(effects::amount(int(args, 0))))
            }
            EffectDamageForcePoints => {
                self.effect_value(Effect::DamageForcePoints(effects::amount(int(args, 0))))
            }
            EffectLinkEffects => {
                let parts: Vec<Effect> = [1, 0]
                    .into_iter()
                    .filter_map(|at| self.effect_arg(args, at))
                    .collect();
                self.effect_value(Effect::Linked(parts))
            }
            // The icon is the views' (the buff bar); the effect is the same.
            SetEffectIcon => args.first().cloned().unwrap_or(Value::Int(0)),
            ApplyEffectToObject => {
                if let Some(effect) = self.effect_arg(args, 1) {
                    let seconds = float(args, 3);
                    self.apply_effect(
                        int(args, 0),
                        effect,
                        (object(args, 2), None),
                        caller,
                        seconds,
                    );
                }
                Value::Int(0)
            }
            ApplyEffectAtLocation => {
                // At a place: carried by the caller's character, if it is
                // one (a visual's event there, `script::effects`); from a
                // trigger, door or placeable (a blast set off as someone
                // walks in), by the party's leader, as a minigame's thing
                // hands its visuals over.
                if let (Some(effect), Some((at, _))) =
                    (self.effect_arg(args, 1), self.place(args, 2))
                {
                    let carrier = if self.character(caller).is_some() {
                        caller
                    } else {
                        self.view.party.first().copied().unwrap_or(caller)
                    };
                    self.apply_effect(
                        int(args, 0),
                        effect,
                        (carrier, Some(at)),
                        caller,
                        float(args, 3),
                    );
                }
                Value::Int(0)
            }
            CutsceneAttack => {
                // The script's outcome (C16): the attacker faces its target
                // for the round, and a hit's damage lands as its effect (a
                // miss, parry or deflection none); a gunner fires either as
                // its shot (`effects::Strike`). The attacker plays the
                // animation the script names (an `animations.2da` row: a
                // swing, a gunner's `b7a1`, played as given whatever the
                // row, as K1's executable plays it) and its target the
                // table's answer to it (`combat::rounds::Table::answer`; a
                // gun's, its plain damage or dodge clip), none where the
                // table has none; one whose target is attacking it back
                // takes the round's second half. The executable lands the
                // blow at once [verify: read in TSL only]; half a round on
                // here, as the Endar Spire's approved duels play.
                let target = object(args, 0);
                let landed = [
                    "ATTACK_RESULT_HIT_SUCCESSFUL",
                    "ATTACK_RESULT_CRITICAL_HIT",
                    "ATTACK_RESULT_AUTOMATIC_HIT",
                ]
                .iter()
                .any(|name| self.library.constant(name) == Some(int(args, 2)));
                if target != ObjectId::INVALID {
                    let animation = u16::try_from(int(args, 1))
                        .ok()
                        .filter(|row| *row > 0 && *row < effects::blows::ROWS)
                        .unwrap_or(0);
                    let parried = ["ATTACK_RESULT_PARRIED", "ATTACK_RESULT_DEFLECTED"]
                        .iter()
                        .any(|name| self.library.constant(name) == Some(int(args, 2)));
                    let kind = match (landed, parried) {
                        (true, _) => crate::combat::rounds::Answer::Damage,
                        (false, true) => crate::combat::rounds::Answer::Parry,
                        (false, false) => crate::combat::rounds::Answer::Dodge,
                    };
                    let answer = self.cutscene_answer(view, (caller, target), animation, kind);
                    let now = view.now_ms;
                    let swung_back = self
                        .state
                        .strikes
                        .iter()
                        .filter(|strike| {
                            strike.animation != 0
                                && strike.by == target
                                && strike.target == caller
                                && strike.due_ms > now
                        })
                        .map(|strike| strike.due_ms)
                        .max();
                    let begins = swung_back.filter(|_| animation != 0).unwrap_or(now);
                    self.queue(
                        caller,
                        Action::Attack {
                            target,
                            passive: true,
                        },
                    );
                    self.state.strikes.push(effects::Strike {
                        target,
                        by: caller,
                        amount: effects::amount(int(args, 3)),
                        due_ms: begins + super::actions::ROUND_MS / 2,
                        assured: false,
                        landed,
                        animation,
                        answer,
                        hit_ms: crate::combat::rounds::HALF_ROUND_MS,
                        began: animation == 0,
                        shots: Default::default(),
                    });
                }
                Value::Int(0)
            }
            SetListening => {
                let id = object(args, 0).0;
                if int(args, 1) != 0 {
                    state.listening.insert(id);
                } else {
                    state.listening.remove(&id);
                }
                Value::Int(0)
            }
            GetIsListening => truth(state.listening.contains(&object(args, 0).0)),
            SetListenPattern => {
                let patterns = state.patterns.entry(object(args, 0).0).or_default();
                let pattern = text(args, 1).to_string();
                patterns.retain(|(own, _)| *own != pattern);
                patterns.push((pattern, int(args, 2)));
                Value::Int(0)
            }
            GetListenPatternNumber => Value::Int(self.event.number),
            GetLastSpeaker => maybe(
                state
                    .last
                    .get(&(caller.0, last::SPEAKER))
                    .copied()
                    .map(ObjectId),
            ),
            SpeakString | ActionSpeakString => {
                self.speak(caller, text(args, 0), int(args, 1));
                Value::Int(0)
            }
            GetLastHostileActor => {
                // Never a dead or destroyed one.
                let actor = state
                    .last
                    .get(&(object(args, 0).0, last::HOSTILE))
                    .map(|id| ObjectId(*id))
                    .filter(|id| {
                        view.thing(*id)
                            .is_some_and(|thing| thing.hp.is_none_or(|(hp, _)| hp > 0))
                    });
                maybe(actor)
            }
            // TSL's: a creature's only, as its executable keeps it (which
            // keeps it out of saves, and reads an unset one as object 0
            // [verify what that is]; here kept, and unset is none).
            SetHealTarget => {
                let healer = object(args, 0);
                if view
                    .thing(healer)
                    .is_some_and(|thing| thing.character.is_some())
                {
                    state
                        .last
                        .insert((healer.0, last::HEAL_TARGET), object(args, 1).0);
                }
                Value::Int(0)
            }
            GetHealTarget => {
                let healer = object(args, 0);
                maybe(
                    view.thing(healer)
                        .and_then(|thing| thing.character)
                        .and_then(|_| state.last.get(&(healer.0, last::HEAL_TARGET)))
                        .map(|id| ObjectId(*id)),
                )
            }
            // `GetHasFeat` asks the feat's uses left today, `GetFeatAcquired`
            // whether it is known; no feat of either game's table has daily
            // uses (`usesperday` blank throughout), so both are whether it
            // is known.
            GetHasFeat | GetFeatAcquired => truth(
                u16::try_from(int(args, 0))
                    .ok()
                    .zip(self.character(object(args, 1)))
                    .is_some_and(|(feat, c)| c.feats.has(feat)),
            ),
            // TSL's: the feat without its prerequisites, and what learning
            // it gives (`passive::learn`: Toughness for every level).
            GrantFeat => {
                let who = object(args, 1);
                if let (Ok(feat), Some(c)) = (u16::try_from(int(args, 0)), self.character(who)) {
                    let mut sheet = self.out.sheet(who, c.worn);
                    if !sheet.feats.has(feat) && feat < Rows::<4>::ROWS {
                        sheet.feats.insert(feat);
                        let levels = sheet.level;
                        crate::combat::passive::learn(&mut sheet, feat, levels);
                        self.out.changes.push(Change::Sheet { who, sheet });
                    }
                }
                Value::Int(0)
            }
            GetHasSkill => truth(
                usize::try_from(int(args, 0))
                    .ok()
                    .zip(self.character(object(args, 1)))
                    .is_some_and(|(skill, c)| c.sheet.skills.get(skill).is_some_and(|r| *r > 0)),
            ),
            // TSL's: the rank with its effects and worn items, feats and
            // the key ability's modifier, as its executable reads it
            // (`force::skill_rank`); 0 for a skill it has no row for.
            GetSkillRank if view.module.game == TSL => Value::Int(
                match (
                    usize::try_from(int(args, 0)).ok(),
                    self.character(object(args, 1)),
                ) {
                    (Some(skill), Some(c)) if skill < c.sheet.skills.len() => {
                        self.skill_rank(&c, skill)
                    }
                    (_, Some(_)) => 0,
                    (_, None) => -1,
                },
            ),
            // K1's: the rank its sheet holds as its effects and worn items
            // leave it; −1 for one that is not a character or a skill it has
            // no row for.
            GetSkillRank => Value::Int(
                usize::try_from(int(args, 0))
                    .ok()
                    .zip(self.character(object(args, 1)))
                    .and_then(|(skill, c)| c.sheet.skills.get(skill).copied())
                    .map_or(-1, i32::from),
            ),
            GetCurrentForcePoints => Value::Int(
                self.character(object(args, 0))
                    .map_or(0, |c| c.fp / HUNDREDTHS),
            ),
            // With TSL's bonus Force points (`Sheet::force_points`).
            GetMaxForcePoints => Value::Int(
                self.character(object(args, 0))
                    .map_or(0, |c| i32::from(c.sheet.force_points())),
            ),
            GetLevelByPosition => {
                let at = usize::try_from(int(args, 0) - 1).ok();
                Value::Int(
                    at.zip(self.character(object(args, 1)))
                        .and_then(|(at, c)| c.classes.get(at).copied())
                        .map_or(0, |(_, levels)| i32::from(levels)),
                )
            }
            // What the power under way costs its caster: its row's cost as
            // TSL adjusts it (`power_cost`), under Force Body the share its
            // Force points pay (`effects::power_cost`), worked out as the
            // impact asks (the executable records it as the cast lands).
            GetSpellForcePointCost => Value::Int(
                self.spell
                    .and_then(|cast| match self.character(cast.caster) {
                        Some(c) => self.power_cost(cast.caster, &c, cast.row).map(|cost| {
                            let lasting = &c.lasting;
                            crate::combat::effects::power_cost(
                                lasting,
                                cost * HUNDREDTHS,
                                view.now_ms,
                            )
                            .0 / HUNDREDTHS
                        }),
                        None => view
                            .module
                            .force_costs
                            .get(usize::from(cast.row))
                            .map(|cost| i32::from(*cost)),
                    })
                    .unwrap_or(0),
            ),
            // TSL's leaves a player's own character as it is: its
            // `ExecuteCommandChangeToStandardFaction` (and `ChangeFaction`)
            // passes over a creature whose stats are a PC's, so Dxun's
            // battle circle (`a_circle_fight`, `tr_circle`: the PC to
            // Friendly 1 after each match) does not turn every Friendly 2
            // creature on the Exile (Dantooine's salvagers at the Enclave)
            // [verify in K1's exe: K1 keeps setting it].
            ChangeToStandardFaction => {
                let who = object(args, 0);
                let tsl = crate::combat::passive::Game::named(&view.module.game)
                    == crate::combat::passive::Game::Tsl;
                let own = view.thing(who).is_some_and(|thing| thing.kind.is_none());
                if let (Some(_), Ok(faction), false) =
                    (self.character(who), u8::try_from(int(args, 1)), tsl && own)
                {
                    self.out.changes.push(Change::Faction { who, faction });
                }
                Value::Int(0)
            }
            // Every character of one standard faction moved to another
            // (Korriban's academy turning on a spy: its neutrals and friends
            // made hostile). The party is not the friends' row's: it stays.
            ChangeFactionByFaction => {
                if let (Ok(from), Ok(faction)) =
                    (u8::try_from(int(args, 0)), u8::try_from(int(args, 1)))
                {
                    for thing in view.things {
                        if thing
                            .character
                            .is_some_and(|c| !c.party && c.faction == from)
                        {
                            self.out.changes.push(Change::Faction {
                                who: thing.id,
                                faction,
                            });
                        }
                    }
                }
                Value::Int(0)
            }
            CancelCombat => {
                let who = object(args, 0);
                if self.character(who).is_some() {
                    self.out.changes.push(Change::CancelCombat { who });
                }
                Value::Int(0)
            }
            // With the override the script lights the blade (`bPowered`) and
            // holds it lit over the game's own rule until it lets go (out of
            // a fight the game puts a saber out); an override putting it out
            // puts it out, and the game may light it again (no held-unlit
            // state); with no override the game decides again, and out of a
            // fight that is unlit [verify]. The transition (`bShowTransition`)
            // is the blade growing or shrinking as it always does.
            SetLightsaberPowered => {
                let who = object(args, 0);
                if self.character(who).is_some() {
                    let on = int(args, 1) != 0 && int(args, 2) != 0;
                    self.out
                        .changes
                        .push(Change::SaberPower { who, on, held: on });
                }
                Value::Int(0)
            }
            SetMinOneHP => {
                let who = object(args, 0);
                if let Some(c) = self.character(who) {
                    let mut sheet = self.out.sheet(who, c.worn);
                    if int(args, 1) != 0 {
                        sheet.flags |= flags::MIN_ONE_HP;
                    } else {
                        sheet.flags &= !flags::MIN_ONE_HP;
                    }
                    self.out.changes.push(Change::Sheet { who, sheet });
                }
                Value::Int(0)
            }
            // Its most hit points, and whole at them (xoreos's reading;
            // K1's `k_def_buff` raises a foe's by 100 for a strong player).
            SetMaxHitPoints => {
                let who = object(args, 0);
                if let Some(c) = self.character(who) {
                    let mut sheet = self.out.sheet(who, c.worn);
                    sheet.max_hp = int(args, 1).clamp(1, i32::from(u16::MAX)) as u16;
                    let hp = sheet.max_hp;
                    self.out.changes.push(Change::Sheet { who, sheet });
                    self.out.changes.push(Change::HitPoints { who, hp });
                }
                Value::Int(0)
            }
            // Kept after death (a scene's body) or not: its own, the
            // caller's (`world::scripts`: the dead fade otherwise).
            SetIsDestroyable => {
                if int(args, 0) == 0 {
                    state.kept.insert(caller.0);
                } else {
                    state.kept.remove(&caller.0);
                }
                Value::Int(0)
            }
            GetIsPoisoned => truth(
                self.character(object(args, 0))
                    .is_some_and(|c| c.poisoned.is_some()),
            ),
            GetMinOneHP => {
                let who = object(args, 0);
                truth(
                    self.character(who).is_some_and(|c| {
                        self.out.sheet(who, c.worn).flags & flags::MIN_ONE_HP != 0
                    }),
                )
            }
            // 0 (dark) to 100 (light); -1 for what is not a creature.
            GetGoodEvilValue => {
                let who = object(args, 0);
                Value::Int(
                    self.character(who)
                        .map_or(-1, |c| i32::from(self.out.sheet(who, c.worn).good_evil)),
                )
            }
            GetAlignmentGoodEvil => {
                let who = object(args, 0);
                Value::Int(self.character(who).map_or(-1, |c| {
                    alignment::side(self.out.sheet(who, c.worn).good_evil)
                }))
            }
            // TSL's: its companions follow through influence.
            AdjustAlignment if self.library.influences() => {
                let quiet = args.len() > 3 && int(args, 3) != 0;
                self.adjust_alignment(object(args, 0), (int(args, 1), int(args, 2)), quiet);
                Value::Int(0)
            }
            // K1's moves only the one named.
            AdjustAlignment => {
                let who = object(args, 0);
                if let Some(c) = self.character(who) {
                    let mut sheet = self.out.sheet(who, c.worn);
                    let was = sheet.good_evil;
                    sheet.good_evil =
                        alignment::shifted(sheet.good_evil, int(args, 1), int(args, 2));
                    // A player's character's shift is told, light or dark.
                    if sheet.good_evil != was && self.is_pc(who) {
                        let light = sheet.good_evil > was;
                        self.out.statuses.push(StatusKind::Alignment { light });
                    }
                    self.out.changes.push(Change::Sheet { who, sheet });
                }
                Value::Int(0)
            }
            SetGoodEvilValue => {
                let who = object(args, 0);
                if let Some(c) = self.character(who) {
                    let mut sheet = self.out.sheet(who, c.worn);
                    sheet.good_evil = int(args, 1).clamp(0, 100) as u8;
                    self.out.changes.push(Change::Sheet { who, sheet });
                }
                Value::Int(0)
            }
            GetSubRace => Value::Int(
                self.character(object(args, 0))
                    .map_or(0, |c| i32::from(c.subrace)),
            ),
            GetGender => Value::Int(
                self.character(object(args, 0))
                    .map_or(0, |c| i32::from(c.gender)),
            ),
            // One an encounter made (`crate::spawner`), while its link
            // stands: a save keeps none (QUIRKS.md Q-0261).
            GetIsEncounterCreature => Value::Int(i32::from(
                state.encounter_creatures.contains_key(&object(args, 0).0),
            )),
            GetEncounterActive
            | GetEncounterSpawnsMax
            | GetEncounterSpawnsCurrent
            | GetEncounterDifficulty => {
                let id = object(args, 0);
                let Some((_, encounter)) = crate::spawner::of(view, id) else {
                    return Value::Int(0);
                };
                let spawn = crate::spawner::spawn_of(state, id, encounter);
                Value::Int(match bound {
                    GetEncounterActive => i32::from(spawn.active),
                    GetEncounterSpawnsMax => spawn.respawns,
                    GetEncounterSpawnsCurrent => spawn.current_spawns,
                    _ => spawn.difficulty,
                })
            }
            // Made active, it spawns at once if a qualifying creature
            // already stands in it.
            SetEncounterActive => {
                let on = int(args, 0) != 0;
                crate::spawner::set_active(view, state, &mut *self.rng, object(args, 1), on);
                Value::Int(0)
            }
            SetEncounterSpawnsMax | SetEncounterSpawnsCurrent | SetEncounterDifficulty => {
                let (value, id) = (int(args, 0), object(args, 1));
                let Some((_, encounter)) = crate::spawner::of(view, id) else {
                    return Value::Int(0);
                };
                let difficulties = &view.module.encounter_tables.difficulties;
                let spawn = crate::spawner::spawn_of(state, id, encounter);
                match bound {
                    SetEncounterSpawnsMax => spawn.respawns = value,
                    SetEncounterSpawnsCurrent => spawn.current_spawns = value,
                    // `encdifficulty.2da`'s value for the row; without the
                    // table 0, a row past it no change (`SetDifficulty`).
                    _ if difficulties.is_empty() => spawn.difficulty = 0,
                    _ => {
                        if let Some(&found) = usize::try_from(value)
                            .ok()
                            .and_then(|row| difficulties.get(row))
                        {
                            spawn.difficulty = found;
                        }
                    }
                }
                Value::Int(0)
            }
            GetAttemptedSpellTarget => maybe(
                view.intents
                    .get(&caller.0)
                    .filter(|intent| intent.talent.is_some())
                    .map(|intent| intent.target),
            ),
            GetAttemptedMovementTarget => maybe(
                state
                    .actions
                    .get(&caller.0)
                    .and_then(|queue| queue.first())
                    .and_then(|queued| match queued.action {
                        Action::Move {
                            to: Target::Object(id),
                            ..
                        } => Some(id),
                        _ => None,
                    }),
            ),
            GetFirstObjectInArea | GetFirstInPersistentObject | GetFirstObjectInShape => {
                let (kind, objects) = match bound {
                    // The area's objects of the type, but the dead for good
                    // [verify in the exe]: TSL's battle of Khoonda ends when
                    // a mercenary's death script, counting the area's
                    // hostile creatures, finds none (`k_battle_death`), its
                    // own dying body among them otherwise.
                    GetFirstObjectInArea => {
                        let filter = int(args, 1);
                        let all = view.things.iter().filter(|t| {
                            self.of_type(t, filter) && !(t.character.is_some() && t.dead_for_good())
                        });
                        (walk::AREA, all.map(|t| t.id).collect())
                    }
                    // An encounter's: the area's objects of the type over
                    // its outline (`GetInActivateArea`).
                    GetFirstInPersistentObject
                        if let Some((facts, _)) = crate::spawner::of(view, object(args, 0)) =>
                    {
                        let filter = int(args, 1);
                        let inside = view
                            .things
                            .iter()
                            .filter(|t| crate::spawner::inside(facts, t.position))
                            .filter(|t| self.of_type(t, filter))
                            .map(|t| t.id)
                            .collect();
                        (walk::PERSISTENT, inside)
                    }
                    GetFirstInPersistentObject => {
                        let filter = int(args, 1);
                        let inside = state
                            .occupants
                            .get(&object(args, 0).0)
                            .cloned()
                            .unwrap_or_else(Vec::new);
                        let objects = inside
                            .into_iter()
                            .filter_map(|id| view.thing(ObjectId(id)))
                            .filter(|t| self.of_type(t, filter))
                            .map(|t| t.id)
                            .collect();
                        (walk::PERSISTENT, objects)
                    }
                    // In a cast's impact, whom JA's delivery caught (C10).
                    _ if let Some(cast) = self.spell => (walk::SHAPE, cast.caught.clone()),
                    _ => {
                        let at = self.place(args, 2).map_or([0.0; 3], |(at, _)| at);
                        let given = vector(args, 5);
                        let origin = if given == [0.0; 3] {
                            self.position(caller).unwrap_or(at)
                        } else {
                            given
                        };
                        (
                            walk::SHAPE,
                            self.in_shape(int(args, 0), float(args, 1), at, origin, int(args, 4)),
                        )
                    }
                };
                self.first(caller, kind, objects)
            }
            GetNextObjectInArea => self.next(caller, walk::AREA),
            GetNextInPersistentObject => self.next(caller, walk::PERSISTENT),
            GetNextObjectInShape => self.next(caller, walk::SHAPE),
            GetNearestObject => {
                let (filter, target) = (int(args, 0), object(args, 1));
                let Some(from) = self.position(target) else {
                    return maybe(None);
                };
                let found = view
                    .things
                    .iter()
                    .filter(|t| t.id != target && self.of_type(t, filter))
                    .map(|t| (distance(from, t.position), t.id))
                    .collect();
                Value::Object(nth_nearest(found, int(args, 2)))
            }
            ActionRandomWalk => self.queue(
                caller,
                Action::RandomWalk {
                    from: None,
                    to: None,
                },
            ),
            // The retail build: scripts skip their debug paths.
            ShipBuild => Value::Int(1),
            EffectAbilityIncrease | EffectAbilityDecrease => {
                let sign = if bound == EffectAbilityIncrease {
                    1
                } else {
                    -1
                };
                self.modifier(
                    Modifies::Ability,
                    sign * int(args, 1),
                    int(args, 0).clamp(0, 5) as u8,
                )
            }
            // Force points over the pool while it lasts (`k_def_buff`'s for
            // a strong player's foes): gained when it lands, lost past the
            // pool when it ends (`world::powers`).
            EffectTemporaryForcePoints => {
                self.modifier(Modifies::ForcePoints, int(args, 0).max(0), 0)
            }
            // Hit points taken first while it lasts (a battle stim's).
            EffectTemporaryHitpoints => self.modifier(Modifies::Vitality, int(args, 0).max(0), 0),
            // An energy shield: its `forceshields.2da` row's numbers.
            EffectForceShield => {
                let row = usize::try_from(int(args, 0)).ok();
                match row.and_then(|row| self.library.shield(row)) {
                    Some(shield) => self.effect_value(Effect::Shield {
                        types: shield.types,
                        points: shield.points,
                    }),
                    None => self.effect_value(Effect::Other("EffectForceShield".into())),
                }
            }
            EffectResurrection => self.effect_value(Effect::Resurrection),
            EffectACIncrease | EffectACDecrease => {
                let sign = if bound == EffectACIncrease { 1 } else { -1 };
                self.modifier(Modifies::Defence, sign * int(args, 0), 0)
            }
            EffectSavingThrowIncrease | EffectSavingThrowDecrease => {
                let sign = if bound == EffectSavingThrowIncrease {
                    1
                } else {
                    -1
                };
                let save = int(args, 0);
                let of = SAVES
                    .iter()
                    .find(|(name, _)| self.library.constant(name) == Some(save))
                    .map_or(ALL_SAVES, |(_, of)| *of);
                self.modifier(Modifies::Save, sign * int(args, 1), of)
            }
            EffectAttackIncrease | EffectAttackDecrease => {
                let sign = if bound == EffectAttackIncrease { 1 } else { -1 };
                self.modifier(Modifies::Attack, sign * int(args, 0), 0)
            }
            EffectDamageIncrease | EffectDamageDecrease => {
                let sign = if bound == EffectDamageIncrease { 1 } else { -1 };
                self.modifier(Modifies::Damage, sign * int(args, 0), 0)
            }
            EffectDamageResistance => {
                self.by_types(Modifies::Resistance, int(args, 1), int(args, 0))
            }
            EffectDamageImmunityIncrease | EffectDamageImmunityDecrease => {
                let sign = if bound == EffectDamageImmunityIncrease {
                    1
                } else {
                    -1
                };
                self.by_types(Modifies::DamageImmunity, sign * int(args, 1), int(args, 0))
            }
            EffectImmunity => {
                self.modifier(Modifies::Immunity, 1, int(args, 0).clamp(0, 255) as u8)
            }
            EffectMovementSpeedIncrease | EffectMovementSpeedDecrease => {
                let sign = if bound == EffectMovementSpeedIncrease {
                    1
                } else {
                    -1
                };
                self.modifier(Modifies::Speed, sign * int(args, 0), 0)
            }
            EffectModifyAttacks => self.modifier(Modifies::Attacks, int(args, 0), 0),
            EffectSkillIncrease | EffectSkillDecrease => {
                let sign = if bound == EffectSkillIncrease { 1 } else { -1 };
                self.modifier(
                    Modifies::Skill,
                    sign * int(args, 1),
                    int(args, 0).clamp(0, 255) as u8,
                )
            }
            EffectAssuredHit => self.modifier(Modifies::AssuredHit, 1, 0),
            EffectForceResistanceIncrease => {
                self.modifier(Modifies::ForceResistance, int(args, 0), 0)
            }
            // TSL's: percent on the Force point regeneration's rate while
            // it lasts (`world::powers`).
            EffectFPRegenModifier => self.modifier(Modifies::FpRegen, int(args, 0), 0),
            EffectStunned => self.effect_value(Effect::Hold(Hold::Stun)),
            EffectChoke => self.effect_value(Effect::Hold(Hold::Choke)),
            EffectDroidStun => self.effect_value(Effect::Hold(Hold::DroidStun)),
            EffectWhirlWind => self.effect_value(Effect::Hold(Hold::Whirlwind)),
            EffectParalyze => self.effect_value(Effect::Hold(Hold::Paralyze)),
            // Held fast where it stands (the adhesive grenade's): a
            // paralysis here.
            EffectEntangle => self.effect_value(Effect::Hold(Hold::Paralyze)),
            // A `poison.2da` row's course.
            EffectPoison => match usize::try_from(int(args, 0))
                .ok()
                .and_then(|row| self.library.poison(row))
            {
                Some(course) => self.effect_value(Effect::Poison(course)),
                None => self.effect_value(Effect::Other("EffectPoison".into())),
            },
            EffectHorrified => self.effect_value(Effect::Hold(Hold::Fear)),
            EffectSleep => self.effect_value(Effect::Hold(Hold::Sleep)),
            // A cutscene's hold: the plain one's state, which no immunity
            // keeps off and nothing caps (`combat::effects::state`).
            EffectCutSceneParalyze | EffectCutSceneStunned | EffectCutSceneHorrified => {
                let state = match bound {
                    EffectCutSceneParalyze => state::PARALYZED,
                    EffectCutSceneStunned => state::STUNNED,
                    _ => state::HORRIFIED,
                };
                self.effect_value(Effect::State {
                    state,
                    cutscene: true,
                })
            }
            // TSL's own states: Force Crush's, a droid's confusion, a mind
            // trick and a droid's scramble.
            EffectCrush | EffectDroidConfused | EffectMindTrick | EffectDroidScramble => {
                let state = match bound {
                    EffectCrush => state::CRUSH,
                    EffectDroidConfused => state::DROID_CONFUSED,
                    EffectMindTrick => state::MIND_TRICK,
                    _ => state::DROID_SCRAMBLE,
                };
                self.effect_value(Effect::State {
                    state,
                    cutscene: false,
                })
            }
            EffectBlind => self.modifier(Modifies::Blind, 0, 0),
            EffectTrueSeeing => self.modifier(Modifies::TrueSeeing, 0, 0),
            EffectFury => self.modifier(Modifies::Fury, 0, 0),
            EffectForceSight => self.modifier(Modifies::ForceSight, 0, 0),
            EffectBodyFuel => self.modifier(Modifies::BodyFuel, 0, 0),
            EffectPsychicStatic => self.modifier(Modifies::PsychicStatic, 0, 0),
            EffectForceBody => {
                self.modifier(Modifies::ForceBody, 0, int(args, 0).clamp(0, 2) as u8)
            }
            EffectFactionModifier => self.modifier(Modifies::Faction, int(args, 0), 0),
            EffectVPRegenModifier => self.modifier(Modifies::VpRegen, int(args, 0), 0),
            // Its hit points, and its interval in ms (the seconds × 1000,
            // cut).
            EffectRegenerate => {
                let interval = (float(args, 1) * 1000.0) as i32;
                self.modifier(
                    Modifies::Regenerate,
                    interval,
                    int(args, 0).clamp(0, 255) as u8,
                )
            }
            // The appearance it draws its target as, while it lasts.
            EffectDisguise => {
                let row = int(args, 0).clamp(0, i32::from(i16::MAX)) as i16;
                self.effect_value(Effect::Modifier {
                    modifies: Modifies::Disguise,
                    amount: row,
                    of: 0,
                })
            }
            EffectVisualEffect => self.effect_value(Effect::Visual {
                row: u16::try_from(int(args, 0)).ok(),
                from: None,
                part: 0,
            }),
            EffectBeam => self.effect_value(Effect::Visual {
                row: u16::try_from(int(args, 0)).ok(),
                from: Some(object(args, 1)),
                part: int(args, 2).clamp(0, 255) as u8,
            }),
            GetFirstEffect => {
                let owner = object(args, 0);
                // Its lasting effects, then its poison (`POISON_ID`).
                let slots: Vec<ObjectId> = self
                    .character(owner)
                    .map(|c| {
                        let poison = c.poisoned.map(|_| ObjectId(POISON_ID));
                        c.lasting
                            .iter()
                            .flatten()
                            .map(|l| ObjectId(l.id))
                            .chain(poison)
                            .collect()
                    })
                    .unwrap_or_else(Vec::new);
                match self.first(owner, walk::EFFECTS, slots) {
                    Value::Object(id) if id != ObjectId::INVALID => {
                        self.effect_value(Effect::Slot { owner, id: id.0 })
                    }
                    _ => Value::Effect(Handle(0)),
                }
            }
            GetNextEffect => {
                let owner = object(args, 0);
                match self.next(owner, walk::EFFECTS) {
                    Value::Object(id) if id != ObjectId::INVALID => {
                        self.effect_value(Effect::Slot { owner, id: id.0 })
                    }
                    _ => Value::Effect(Handle(0)),
                }
            }
            GetIsEffectValid => truth(match self.effect_arg(args, 0) {
                Some(Effect::Slot { .. }) => self.slot(args, 0).is_some(),
                Some(_) => true,
                None => false,
            }),
            GetEffectType => Value::Int(match self.slot(args, 0) {
                // A state's by its number (the executable's table).
                Some((_, l)) if l.modifies == Modifies::State => state::effect_type(l.of),
                Some((_, l)) => self
                    .library
                    .constant(effect_type(l.modifies, l.amount))
                    .unwrap_or(0),
                None => 0,
            }),
            GetEffectCreator => maybe(self.slot(args, 0).map(|(_, l)| ObjectId(l.by))),
            GetEffectSpellId => Value::Int(self.slot(args, 0).map_or(-1, |(_, l)| {
                if l.spell == u16::MAX {
                    -1
                } else {
                    i32::from(l.spell)
                }
            })),
            RemoveEffect => {
                if let Some((owner, lasting)) = self.slot(args, 1) {
                    let after = self.state.applied.len() as u32;
                    self.state.removed.push((owner.0, Some(lasting.id), after));
                }
                Value::Int(0)
            }
            // TSL's: the lasting visuals of that `visualeffects.2da` row on
            // it (`k_fab_fade_sp` takes its fade-in's shimmer, 8000, off
            // again). [verify] what other effects' IDs are.
            RemoveEffectByID => {
                let (owner, row) = (object(args, 0), int(args, 1));
                let ids: Vec<u32> = self.character(owner).map_or(Vec::new(), |c| {
                    c.lasting
                        .iter()
                        .flatten()
                        .filter(|l| l.modifies == Modifies::Visual && i32::from(l.amount) == row)
                        .map(|l| l.id)
                        .collect()
                });
                let after = self.state.applied.len() as u32;
                for id in ids {
                    self.state.removed.push((owner.0, Some(id), after));
                }
                Value::Int(0)
            }
            ClearAllEffects => {
                let after = self.state.applied.len() as u32;
                self.state.removed.push((caller.0, None, after));
                Value::Int(0)
            }
            // TSL's: the first lasting effect on it of the given effect's
            // kind and first two numbers (`exact_match`), nothing else
            // compared; the given one never applied.
            RemoveEffectByExactMatch => {
                let owner = object(args, 0);
                let found = self.effect_arg(args, 1).and_then(|effect| {
                    let c = self.character(owner)?;
                    c.lasting
                        .into_iter()
                        .flatten()
                        .find(|l| exact_match(&effect, l))
                });
                if let Some(lasting) = found {
                    let after = self.state.applied.len() as u32;
                    self.state.removed.push((owner.0, Some(lasting.id), after));
                }
                Value::Int(0)
            }
            // Its immunity effects and worn items' alone (an immunity to all
            // counting), for a type within 1 to 33; no feats, no mind
            // umbrella. `oVersus` limits nothing: no immunity here is kept
            // for a race or side only.
            GetIsImmune => {
                let kind = int(args, 1);
                let immune = (1..34).contains(&kind)
                    && self
                        .character(object(args, 0))
                        .is_some_and(|c| immune_effect(&c, kind as u8, now));
                truth(immune)
            }
            // At once, when both are in the area: turned along the line from
            // the other to it (in 3D; one standing on the other turns not).
            FaceObjectAwayFromObject => {
                let facer = object(args, 0);
                if let (Some(from), Some(other)) =
                    (self.position(facer), self.position(object(args, 1)))
                {
                    let away: [f32; 3] = std::array::from_fn(|i| from[i] - other[i]);
                    if away.iter().map(|v| v * v).sum::<f32>() > 1e-4 {
                        let yaw = crate::math::atan2_deg(away[1], away[0]);
                        self.out.changes.push(Change::Face { who: facer, yaw });
                    }
                }
                Value::Int(0)
            }
            GetHasSpellEffect => {
                let spell = int(args, 0);
                truth(self.character(object(args, 1)).is_some_and(|c| {
                    c.lasting
                        .iter()
                        .flatten()
                        .any(|l| i32::from(l.spell) == spell)
                }))
            }
            EventSpellCastAt => Value::Event(self.state.spell_event((
                object(args, 0),
                int(args, 1),
                int(args, 2) != 0,
            ))),
            GetSpellId => Value::Int(self.spell.map_or(-1, |cast| i32::from(cast.row))),
            GetSpellTargetObject => maybe(self.spell.map(|cast| cast.target)),
            // Whom its power under way is for (its script's talent), else
            // whom its last power reached (companions turn on the leader's).
            GetSpellTarget => {
                let who = object(args, 0);
                let using = view
                    .intents
                    .get(&who.0)
                    .filter(|intent| matches!(intent.talent, Some(Use::Power(_))));
                let reached = state.last.get(&(who.0, last::SPELL_TARGET));
                maybe(
                    using
                        .map(|intent| intent.target)
                        .or(reached.map(|id| ObjectId(*id))),
                )
            }
            GetSpellTargetLocation => {
                let at = self.spell.map_or([0.0; 3], |cast| cast.at);
                Value::Location(self.state.location(at, 0.0))
            }
            GetSpellSaveDC => Value::Int(
                self.character(caller)
                    .map_or(rules::scripts::answers::NO_CASTER_DC, |c| {
                        crate::combat::force::save_dc(&c.sheet)
                    }),
            ),
            GetCasterLevel => Value::Int(
                self.character(object(args, 0))
                    .map_or(0, |c| i32::from(c.sheet.level)),
            ),
            ResistForce => {
                // 1 when the target resists (`combat::force::resistance`).
                let (source, target) = (
                    self.character(object(args, 0)),
                    self.character(object(args, 1)),
                );
                let Some((source, target)) = source.zip(target) else {
                    return Value::Int(0);
                };
                let granted: i32 = target
                    .lasting
                    .iter()
                    .flatten()
                    .filter(|l| l.until > now && l.modifies == Modifies::ForceResistance)
                    .map(|l| i32::from(l.amount))
                    .sum();
                let resistance =
                    crate::combat::force::resistance(target.sheet.level, target.absorbing, granted);
                let d20 = self.rng.below(20) as i32 + 1;
                truth(resistance > 0 && d20 + i32::from(source.sheet.level) < resistance)
            }
            FortitudeSave | ReflexSave | WillSave => {
                // 0 failed, 1 saved, 2 immune.
                let index = match bound {
                    FortitudeSave => 0,
                    ReflexSave => 1,
                    _ => 2,
                };
                let Some(them) = self.character(object(args, 0)) else {
                    return Value::Int(0);
                };
                if save_immunity(self.library, int(args, 2))
                    .is_some_and(|kind| immune(&them, kind, now))
                {
                    return Value::Int(2);
                }
                let d20 = self.rng.below(20) as i32 + 1;
                truth(d20 + them.sheet.save(index) >= int(args, 1))
            }
            // A Reflex save against the DC divides the damage
            // (`REFLEX_DIVISOR`, rounded down); one that is not a
            // character takes it whole.
            GetReflexAdjustedDamage => {
                let damage = int(args, 0);
                let Some(them) = self.character(object(args, 1)) else {
                    return Value::Int(damage);
                };
                let d20 = self.rng.below(20) as i32 + 1;
                let saved = d20 + them.sheet.save(1) >= int(args, 2);
                Value::Int(if saved {
                    damage / rules::scripts::answers::REFLEX_DIVISOR
                } else {
                    damage
                })
            }
            // A creature's score (`ABILITY_*` 0 to 5); 0 for anything else.
            GetAbilityScore => Value::Int(
                usize::try_from(int(args, 1))
                    .ok()
                    .filter(|ability| *ability < 6)
                    .zip(self.character(object(args, 0)))
                    .map_or(0, |(ability, c)| i32::from(c.sheet.abilities[ability])),
            ),
            GetAbilityModifier => {
                let ability = int(args, 0).clamp(0, 5) as usize;
                Value::Int(self.character(object(args, 1)).map_or(0, |c| {
                    c.sheet
                        .modifier(crate::combat::sheet::Ability::ALL[ability])
                }))
            }
            GetIsLinkImmune => {
                let immune_to = self.character(object(args, 0)).is_some_and(|c| {
                    self.effect_arg(args, 1).is_some_and(|effect| {
                        effect.leaves().iter().any(|leaf| match leaf {
                            Effect::Hold(hold) => {
                                hold.immunity().is_some_and(|kind| immune(&c, kind, now))
                            }
                            // A state's link's immunities (a cutscene's too:
                            // only applying it skips them).
                            Effect::State { state, .. } => state::link_immunities(*state)
                                .iter()
                                .any(|kind| immune_effect(&c, *kind, now)),
                            _ => false,
                        })
                    })
                });
                truth(immune_to)
            }
            // A push outside a cast (a scene's blast, a story's shove) is
            // the script's alone: it throws its target down, away from its
            // centre or its maker. In a cast's impact (a power, a grenade's
            // burst) the shove is the delivery's, JA's (I22 C10).
            EffectForcePushed | EffectForcePushTargeted if self.spell.is_none() => {
                let from = (bound == EffectForcePushTargeted)
                    .then(|| self.place(args, 0).map(|(at, _)| at))
                    .flatten();
                self.effect_value(Effect::Pushed { from })
            }
            // A deliberate deviation (I22 C10: JA delivers, KOTOR decides):
            // the shove and the thrown blade are JA's own delivery, so these
            // change nothing here. The fizzle's and the resist's own looks
            // (K1's exe: `v_fizzle_imp` on the hand's `fxfail` hook,
            // `v_fresist_imp` on `fxresist`, with the resist's animation)
            // are not drawn yet [verify].
            EffectForceFizzle
            | EffectForceResisted
            | EffectForcePushed
            | EffectForcePushTargeted
            | EffectLightsaberThrow => self.effect_value(Effect::Visual {
                row: None,
                from: None,
                part: 0,
            }),
            // A room's own clip (the turret's skybox, `scriptloop0<n>`),
            // played by the view.
            PlayRoomAnimation => {
                let room = crate::snapshot::room_key(text(args, 0));
                let anim = int(args, 1).clamp(0, 255) as u8;
                let since_ms = self.view.now_ms;
                self.area_sound(|area| {
                    area.room = Some(crate::snapshot::RoomAnim {
                        room,
                        anim,
                        since_ms,
                    })
                });
                Value::Int(0)
            }
            SoundObjectPlay
            | SoundObjectStop
            | SoundObjectFadeAndStop
            | SoundObjectSetVolume
            | SoundObjectSetFixedVariance => {
                use crate::snapshot::sound;
                let (state, volume, pitch) = match bound {
                    SoundObjectPlay => (Some(sound::PLAYING), None, None),
                    SoundObjectSetVolume => {
                        (None, Some(1 + int(args, 1).clamp(0, 127) as u8), None)
                    }
                    // A speed the sound plays at from then on (the swoop's
                    // engine rising with its gear).
                    SoundObjectSetFixedVariance => {
                        let hundredths = (float(args, 1) * 100.0).round().clamp(0.0, 65534.0);
                        (None, None, Some(1 + hundredths as u16))
                    }
                    _ => (Some(sound::STOPPED), None, None),
                };
                // A stop's fade: the swoop's last engine loop eases out as
                // the next begins.
                let fade = match bound {
                    SoundObjectFadeAndStop => {
                        Some((float(args, 1) * 1000.0).round().clamp(0.0, 65535.0) as u16)
                    }
                    SoundObjectStop => Some(0),
                    _ => None,
                };
                let object = object(args, 0);
                // A co-op race's run keeps its own (each seat hears its
                // run's, M10).
                let kept = super::minigame::voice(
                    self,
                    caller,
                    crate::minigame::Voice {
                        object: object.0,
                        state,
                        volume,
                        pitch,
                        plays: 0,
                        fade,
                    },
                    bound == SoundObjectPlay,
                );
                if kept {
                    return Value::Int(0);
                }
                self.out.changes.push(Change::Sound {
                    object,
                    state,
                    volume,
                    pitch,
                    play: bound == SoundObjectPlay,
                    fade,
                });
                Value::Int(0)
            }
            // Heard at the caller by every player (`Notice::Sound`).
            PlaySound => {
                let sound = crate::notice::Sound::new(caller.0, text(args, 0));
                self.tell(None, Notice::Sound(sound))
            }
            SoundObjectGetVolume => Value::Int(
                view.thing(object(args, 0))
                    .and_then(|thing| thing.sound)
                    .filter(|(_, volume)| *volume > 0)
                    .map_or(127, |(_, volume)| i32::from(volume) - 1),
            ),
            MusicBackgroundPlay | MusicBackgroundStop | MusicBattlePlay | MusicBattleStop
            | AmbientSoundPlay | AmbientSoundStop => {
                use crate::snapshot::sound;
                let state = match bound {
                    MusicBackgroundPlay | MusicBattlePlay | AmbientSoundPlay => sound::PLAYING,
                    _ => sound::STOPPED,
                };
                self.area_sound(|area| {
                    let track = match bound {
                        MusicBackgroundPlay | MusicBackgroundStop => &mut area.music,
                        MusicBattlePlay | MusicBattleStop => &mut area.battle,
                        _ => &mut area.ambient,
                    };
                    track.state = state;
                });
                Value::Int(0)
            }
            MusicBackgroundChangeDay
            | MusicBackgroundChangeNight
            | MusicBattleChange
            | AmbientSoundChangeDay
            | AmbientSoundChangeNight => {
                let row = u16::try_from(int(args, 1)).ok();
                self.area_sound(|area| match bound {
                    MusicBackgroundChangeDay => area.music.day = row,
                    MusicBackgroundChangeNight => area.music.night = row,
                    MusicBattleChange => area.battle.day = row,
                    AmbientSoundChangeDay => area.ambient.day = row,
                    _ => area.ambient.night = row,
                });
                Value::Int(0)
            }
            AmbientSoundSetDayVolume | AmbientSoundSetNightVolume => {
                let volume = 1 + int(args, 1).clamp(0, 127) as u8;
                self.area_sound(|area| area.ambient.volume = volume);
                Value::Int(0)
            }
            // The track scripts set, else none known here (−1: the ARE's).
            MusicBackgroundGetDayTrack
            | MusicBackgroundGetNightTrack
            | MusicBackgroundGetBattleTrack => {
                let area = self.out.area_sound.unwrap_or(view.area_sound);
                let row = match bound {
                    MusicBackgroundGetDayTrack => area.music.day,
                    MusicBackgroundGetNightTrack => area.music.night,
                    _ => area.battle.day,
                };
                Value::Int(row.map_or(-1, i32::from))
            }
            CreateItemOnObject
            | CreateItemOnFloor
            | GetItemInSlot
            | GetFirstItemInInventory
            | GetNextItemInInventory
            | GetItemPossessedBy
            | GetItemPossessor
            | GetBaseItemType
            | GetItemHasItemProperty
            | GetItemStackSize
            | GetNumStackedItems
            | SetItemStackSize
            | GetItemCharges
            | SetItemCharges
            | DestroyObject
            | ActionEquipItem
            | ActionUnequipItem
            | ActionGiveItem
            | GiveItem
            | ActionTakeItem
            | GetGold
            | GiveGoldToCreature
            | TakeGoldFromCreature
            | GetPlotFlag
            | SetPlotFlag
            | GetIdentified
            | SetIdentified
            | SetItemNonEquippable => self.item_call(bound, caller, args),
            AddAvailableNPCByObject
            | AddAvailableNPCByTemplate
            | RemoveAvailableNPC
            | IsAvailableCreature
            | AddPartyMember
            | RemovePartyMember
            | IsNPCPartyMember
            | GetPartyMemberCount
            | SetPartyLeader
            | GetNPCAIStyle
            | SetNPCAIStyle
            | GetPartyAIStyle
            | SetPartyAIStyle
            | GetSoloMode
            | SetSoloMode
            | GetPlayerRestrictMode
            | SetPlayerRestrictMode
            | GetNPCSelectability
            | SetNPCSelectability
            | RemoveNPCFromPartyToBase
            | RebuildPartyTable
            | SaveNPCByObject => self.party_call(bound, args),
            // TSL's duel rules (`party::forfeit`).
            SetForfeitConditions => {
                self.state.party.forfeit = int(args, 0) as u32;
                Value::Int(0)
            }
            GetLastForfeitViolation => Value::Int(self.state.party.violation as i32),
            GetInfluence | SetInfluence | ModifyInfluence => self.influence_call(bound, args),
            AddAvailablePUPByTemplate
            | AddAvailablePUPByObject
            | AssignPUP
            | SpawnAvailablePUP
            | AddPartyPuppet
            | GetPUPOwner
            | GetIsPuppet
            | SavePUPByObject
            | ActionFollowOwner => self.puppet_call(bound, caller, args),
            // TSL's: a creature that does not turn to face whom it talks
            // with (`Step::turn`), or does again.
            SetOrientOnClick => {
                let who = object(args, 0);
                if int(args, 1) != 0 {
                    self.state.unoriented.remove(&who.0);
                } else {
                    self.state.unoriented.insert(who.0);
                }
                Value::Int(0)
            }
            // TSL's: the object updated wherever it stands, and so found by a
            // conversation's line beyond the speakers' reach (the
            // executable's `ExecuteCommandForceAlwaysUpdate` sets the flag
            // `GetNearestObjectByName` lets past its radius): Kavar and
            // Talia in 512OND's throne room, 1400 m from Dhagon's office.
            SetForceAlwaysUpdate => {
                let who = object(args, 0);
                if who != ObjectId::INVALID {
                    if int(args, 1) != 0 {
                        self.state.always_update.insert(who.0);
                    } else {
                        self.state.always_update.remove(&who.0);
                    }
                }
                Value::Int(0)
            }
            // TSL's: whether the creature holds the meditative stance, which
            // only its stance action puts it in (the executable's
            // `AIActionStanceMeditative`): no action here takes that stance
            // yet, so none does.
            IsMeditating => Value::Int(0),
            // TSL's: the party's natural healing off, until turned on again
            // (`world::powers`' regeneration).
            DisableHealthRegen => {
                self.state.party.no_regen = int(args, 0) != 0;
                Value::Int(0)
            }
            AddJournalQuestEntry | RemoveJournalQuestEntry | GetJournalEntry => {
                self.journal_call(bound, args)
            }
            GiveXPToCreature | SetXP | GetXP | GivePlotXP | GetJournalQuestExperience => {
                self.xp_call(bound, args)
            }
            AddMultiClass => {
                let class = u8::try_from(int(args, 0)).ok();
                self.multiclass(object(args, 1), class);
                Value::Int(0)
            }
            GetLockKeyRequired
            | GetLockLockable
            | GetLockUnlockDC
            | GetLockLockDC
            | ActionUnlockObject
            | ActionLockObject
            | GetIsDoorActionPossible
            | DoDoorAction => self.lock_call(bound, caller, args),
            GetIsTrapped
            | GetTrapBaseType
            | GetTrapDetectable
            | GetTrapDisarmable
            | GetTrapFlagged
            | GetTrapOneShot
            | GetTrapDetectDC
            | GetTrapDisarmDC
            | GetTrapKeyTag
            | GetTrapCreator
            | GetTrapDetectedBy
            | SetTrapDetectedBy
            | SetTrapDisabled
            | GetLastTrapDetected
            | GetLastDisarmed
            | GetNearestTrapToObject
            | VersusTrapEffect
            | AwardStealthXP
            | GetStealthXPEnabled
            | SetStealthXPEnabled
            | GetCurrentStealthXP
            | SetCurrentStealthXP
            | GetMaxStealthXP
            | SetMaxStealthXP
            | GetStealthXPDecrement
            | SetStealthXPDecrement
            | IsStealthed
            | GetOwnerDemolitionsSkill
            | SpawnMine
            | DetonateMine => self.trap_call(bound, caller, args),
            GetSpellBaseForcePointCost
            | GetSpellFormMask
            | GetSpellAcquired
            | GrantSpell
            | IsFormActive
            | SetCurrentForm
            | IsInTotalDefense
            | AddBonusForcePoints
            | SetBonusForcePoints
            | GetBonusForcePoints
            | AdjustCreatureAttributes
            | AdjustCreatureSkills
            | ModifyFortitudeSavingThrowBase
            | ModifyReflexSavingThrowBase
            | ModifyWillSavingThrowBase
            | GetSkillRankBase
            | GetRacialSubType
            | IsRunning
            | GetCombatActionsPending
            | SetFakeCombatState
            | CreatureFlourishWeapon
            | ForceHeartbeat => self.force_call(bound, args),
            // The party table keeps one creature for each companion: one
            // spawned again while its creature is here is that creature,
            // its actions cleared and moved to the place, as TSL's
            // `CSWPartyTable::SpawnNPC` takes the table's own creature
            // (`GetNPCObject` makes one only for an empty slot), then
            // `ClearAllActions` and `AddToArea` at the location.
            // TSL's Bao-Dur joins by being spawned, added, removed and
            // spawned again (`a_baodur_join`): a second one had stayed at
            // the wreck, the one `GetObjectByTag("BaoDur")` then found. And
            // Nar Shaddaa's Atton, spawned to warn the player, is spawned
            // again by the first line of the warning he speaks
            // (`a_atton_cut_1`): another in his place had ended it.
            SpawnAvailableNPC => {
                let npc = u8::try_from(int(args, 0)).ok();
                let had = npc
                    .and_then(|npc| self.state.party.available.get(&npc))
                    .and_then(|slot| slot.object)
                    .map(ObjectId)
                    .filter(|old| self.exists(*old) && !self.destroyed(*old));
                let spawned = match (had, self.place(args, 1)) {
                    (Some(old), Some((at, yaw))) => {
                        clear_queue(self.state, self.view, old);
                        self.out.changes.push(Change::Clear { who: old });
                        match self.out.made.iter_mut().find(|made| made.id == old) {
                            Some(made) => (made.at, made.yaw) = (at, yaw),
                            None => self.out.changes.push(Change::Jump {
                                who: old,
                                to: at,
                                yaw,
                            }),
                        }
                        Some(old)
                    }
                    (None, Some((at, yaw))) => npc.and_then(|npc| {
                        super::make_npc(self.library, self.view, self.state, self.out, npc, at, yaw)
                    }),
                    (_, None) => None,
                };
                Value::Object(spawned.unwrap_or(ObjectId::INVALID))
            }
            // The party table keeps the companion as it stands now.
            SaveNPCState => {
                if let Ok(npc) = u8::try_from(int(args, 0)) {
                    super::keep_npc(self.view, self.state, self.out, npc);
                }
                Value::Int(0)
            }
            SwitchPlayerCharacter => self.switch_player(int(args, 0)),
            // Creatures for now; other kinds are not made in play yet.
            CreateObject => {
                // A creature or a placeable, of the templates the module's
                // plan gathered (`Library::creature`, `placeable`).
                let kind = [ObjectKind::Creature, ObjectKind::Placeable]
                    .into_iter()
                    .find(|kind| {
                        self.library.object_types.get(kind).copied() == Some(int(args, 0))
                    });
                let template = key(text(args, 1));
                let known = match kind {
                    Some(ObjectKind::Creature) => self.library.creature(&template).is_some(),
                    Some(_) => self.library.placeable(&template).is_some(),
                    None => false,
                };
                let made = known
                    .then(|| self.place(args, 2))
                    .flatten()
                    .and_then(|(at, yaw)| self.make(&template, at, yaw));
                if made.is_none() {
                    self.trace(&format!(
                        "  not made: type {} template {:?}",
                        int(args, 0),
                        text(args, 1)
                    ));
                }
                Value::Object(made.unwrap_or(ObjectId::INVALID))
            }
            GetRunScriptVar => Value::Int(self.event.script_var),
            GetUserDefinedEventNumber => Value::Int(self.event.number),
            GetLoadFromSaveGame => Value::Int(0),
            DelayCommand => {
                if let Some(code) = action(args, 1) {
                    let ms = u64::from(seconds_ms(float(args, 0)));
                    let talk = super::dialogue::running(&state.talk, &self.event);
                    state.defer(
                        now + ms,
                        caller,
                        self.script,
                        code,
                        (self.spell.cloned(), talk),
                    );
                }
                Value::Int(0)
            }
            AssignCommand => {
                let target = object(args, 0);
                if let Some(code) = action(args, 1).filter(|_| target != ObjectId::INVALID) {
                    let talk = super::dialogue::running(&state.talk, &self.event);
                    state.defer(now, target, self.script, code, (self.spell.cloned(), talk));
                }
                Value::Int(0)
            }
            ExecuteScript => {
                self.execute(&key(text(args, 0)), object(args, 1), int(args, 2));
                Value::Int(0)
            }
            SignalEvent if handle(args, 1).0 & SPELL_EVENT != 0 => {
                // A spell cast at the target: its hook, the caster, the
                // spell and whether harmful kept for `GetLastSpell…`.
                let target = object(args, 0);
                if let Some((caster, spell, harmful)) = self.state.spell_event_at(handle(args, 1))
                    && let Some(script) = view.hook(target, Hook::SpellCastAt)
                {
                    let last = &mut self.state.last;
                    last.insert((target.0, last::SPELL_CASTER), caster.0);
                    last.insert((target.0, last::SPELL), spell as u32);
                    last.insert((target.0, last::SPELL_HARMFUL), u32::from(harmful));
                    self.state.pending.push(Pending::new(&script, target));
                }
                Value::Int(0)
            }
            SignalEvent => {
                let target = object(args, 0);
                if let Some(script) = view.hook(target, Hook::UserDefined) {
                    let mut pending = Pending::new(&script, target);
                    pending.number = handle(args, 1).0 as i32;
                    pending.ai = self.event.ai == Some(target);
                    state.pending.push(pending);
                }
                Value::Int(0)
            }
            // The event carries its number (only user-defined events are
            // made so far).
            EventUserDefined => Value::Event(Handle(int(args, 0) as u32)),
            ActionDoCommand => match action(args, 0) {
                Some(deferred) => self.queue(
                    caller,
                    Action::Code {
                        script: self.script.to_string(),
                        deferred,
                    },
                ),
                None => Value::Int(0),
            },
            ActionWait => self.queue(
                caller,
                Action::Wait {
                    ms: seconds_ms(float(args, 0)),
                },
            ),
            ActionMoveToObject | ActionForceMoveToObject => {
                let force_ms =
                    (bound == ActionForceMoveToObject).then(|| seconds_ms(float(args, 3)));
                self.queue(
                    caller,
                    Action::Move {
                        to: Target::Object(object(args, 0)),
                        run: int(args, 1) != 0,
                        range: float(args, 2),
                        force_ms,
                    },
                )
            }
            ActionMoveToLocation | ActionForceMoveToLocation => match self.place(args, 0) {
                Some((at, yaw)) => {
                    let force_ms =
                        (bound == ActionForceMoveToLocation).then(|| seconds_ms(float(args, 2)));
                    self.queue(
                        caller,
                        Action::Move {
                            to: Target::Point(at, yaw),
                            run: int(args, 1) != 0,
                            range: MOVE_TO_LOCATION_RANGE,
                            force_ms,
                        },
                    )
                }
                None => Value::Int(0),
            },
            // A move to a place `fMoveAwayRange` from what it flees, picked
            // as it is asked for (`away`); nothing when it is that far
            // already, or what it flees is not here.
            ActionMoveAwayFromObject => {
                let (from, threat) = (self.position(caller), self.position(object(args, 0)));
                let to = from
                    .zip(threat)
                    .and_then(|(from, threat)| self.away(from, threat, float(args, 2)));
                match to {
                    Some((at, yaw)) => self.queue(
                        caller,
                        Action::Move {
                            to: Target::Point(at, yaw),
                            run: int(args, 1) != 0,
                            range: MOVE_TO_LOCATION_RANGE,
                            force_ms: None,
                        },
                    ),
                    None => Value::Int(0),
                }
            }
            // A companion runs back to within its following distance of the
            // party's leader (its mind follows on from there); the leader
            // and those outside the party stay.
            ActionFollowLeader => {
                let party = self.party();
                // A companion a seat drives is its player's to move (I30
                // P2): the AI's following is for those nobody plays.
                let driven = self
                    .character(caller)
                    .is_some_and(|character| character.driver.is_some());
                match party.first() {
                    Some(&leader) if leader != caller && party.contains(&caller) && !driven => self
                        .queue(
                            caller,
                            Action::Move {
                                to: Target::Object(leader),
                                run: true,
                                range: crate::combat::ai::FOLLOW_NEAR * collision::UNIT,
                                force_ms: None,
                            },
                        ),
                    _ => Value::Int(0),
                }
            }
            ActionEquipMostDamagingMelee | ActionEquipMostDamagingRanged => {
                use crate::inventory::slots;
                let ranged = bound == ActionEquipMostDamagingRanged;
                let off_hand = !ranged && int(args, 1) != 0;
                let slot = if off_hand {
                    slots::LEFT_WEAPON
                } else {
                    slots::RIGHT_WEAPON
                };
                // One of a stack, as `ActionEquipItem` (Juhani's two
                // lightsabers on Dantooine are one stack).
                if let Some(best) = self.most_damaging(caller, ranged, slot) {
                    let best = self.state.one_of(best);
                    let back = items::holder(view, caller);
                    self.state.equip(self.out, best, caller.0, slot, back);
                    super::forfeit_equip(self.library, view, self.state, caller, (best.0, slot));
                }
                Value::Int(0)
            }
            // TSL's: the weapon sets exchanged when it comes to the front
            // (`actions`: the queue cleared, an attack on again in combat).
            ActionSwitchWeapons => self.queue(caller, Action::SwitchWeapons),
            ActionJumpToObject => self.queue(
                caller,
                Action::Jump {
                    to: Target::Object(object(args, 0)),
                },
            ),
            ActionJumpToLocation => match self.place(args, 0) {
                Some((at, yaw)) => self.queue(
                    caller,
                    Action::Jump {
                        to: Target::Point(at, yaw),
                    },
                ),
                None => Value::Int(0),
            },
            JumpToObject | JumpToLocation => {
                let place = if bound == JumpToObject {
                    view.thing(object(args, 0))
                        .map(|thing| (thing.position, thing.yaw))
                } else {
                    self.place(args, 0)
                };
                if let Some((to, yaw)) = place {
                    self.out.changes.push(Change::Jump {
                        who: caller,
                        to,
                        yaw,
                    });
                }
                Value::Int(0)
            }
            ActionOpenDoor | ActionCloseDoor => self.queue(
                caller,
                Action::Door {
                    door: object(args, 0),
                    open: bound == ActionOpenDoor,
                },
            ),
            // Shuts the door at once, queued behind nothing and with no
            // walk to it, as K1's Yavin station's abort script has it shut
            // behind the party (nwscript.nss: "a hack").
            YavinHackCloseDoor => {
                self.out.changes.push(Change::Door {
                    door: object(args, 0),
                    open: false,
                    by: caller,
                });
                Value::Int(0)
            }
            ActionUseObject | ActionInteractObject => self.queue(
                caller,
                Action::UseObject {
                    object: object(args, 0),
                },
            ),
            // KOTOR's script semantics, everywhere, not only while the world
            // is held: queued, `animation_ms` long, except one that
            // loops on, which plays at once as a queued one would be taken
            // (`takes`). `PlayAnimation` plays at once, a negative duration
            // looping until stopped; its lengths are the queued ones'
            // (neither surveyed for it [verify]).
            ActionPlayAnimation | PlayAnimation => {
                let tsl = crate::combat::passive::Game::named(&view.module.game)
                    == crate::combat::passive::Game::Tsl;
                let animation = self.library.script_animation(int(args, 0), tsl);
                let (speed, seconds) = (float(args, 1), float(args, 2));
                let clip = (self.view.appearance(caller))
                    .and_then(|row| self.library.clip_ms(row, animation));
                let ms = match bound == PlayAnimation && seconds < 0.0 {
                    true => u32::MAX,
                    false => animation_ms(animation, (speed, seconds), clip),
                };
                let action = Action::Animate { animation, ms };
                let at_once = bound == PlayAnimation || loops_on(animation, seconds);
                if !at_once {
                    if !self.queued_long(caller, (animation, ms)) {
                        self.queue(caller, action);
                    }
                } else if bound == PlayAnimation || self.takes(caller, &action) {
                    self.out.changes.push(Change::Animate {
                        who: caller,
                        play: (animation, ms),
                    });
                }
                Value::Int(0)
            }
            ActionStartConversation
            | BeginConversation
            | SpeakOneLinerConversation
            | BarkString
            | GetIsInConversation
            | GetIsConversationActive
            | GetPCSpeaker
            | ActionPauseConversation
            | ActionResumeConversation
            | SetCustomToken
            | SetDialogPlaceableCamera
            | SetLockOrientationInDialog
            | SetLockHeadFollowInDialog
            | GetScriptParameter
            | GetScriptStringParameter => self.talk(bound, caller, args),
            // The caller gives up (`surrender`).
            SurrenderToEnemies | SurrenderRetainBuffs => {
                let keeps = bound == SurrenderRetainBuffs;
                surrender(view, self.state, self.out, caller, keeps);
                Value::Int(0)
            }
            // Queued: the surrender comes when it is reached.
            ActionSurrenderToEnemies => self.queue(caller, Action::Surrender),
            // A cast at an object or a place (`Action::Cast`), its row within
            // `spells.2da` and its projectile's path type within 0 to 4: a
            // fake one only looks cast (and needs its caller commandable no
            // more than it needs the power); a real one, `bCheat` aside, is a
            // power its caller knows.
            ActionCastSpellAtObject
            | ActionCastSpellAtLocation
            | ActionCastFakeSpellAtObject
            | ActionCastFakeSpellAtLocation => {
                let at_object =
                    matches!(bound, ActionCastSpellAtObject | ActionCastFakeSpellAtObject);
                let fake = matches!(
                    bound,
                    ActionCastFakeSpellAtObject | ActionCastFakeSpellAtLocation
                );
                let path = int(
                    args,
                    if fake {
                        2
                    } else if at_object {
                        5
                    } else {
                        4
                    },
                );
                let row = u16::try_from(int(args, 0))
                    .ok()
                    .filter(|row| usize::from(*row) < self.library.talents.spells.len());
                let target = if at_object {
                    let id = object(args, 1);
                    view.thing(id).map(|_| Target::Object(id))
                } else {
                    self.place(args, 1).map(|(at, yaw)| Target::Point(at, yaw))
                };
                let cheat = !fake && int(args, 3) != 0;
                match (row, target) {
                    (Some(row), Some(target)) if (0..=4).contains(&path) => {
                        let cast = Action::Cast {
                            row,
                            target,
                            fake,
                            cheat,
                        };
                        if fake {
                            self.state.queue(caller, cast);
                        } else {
                            self.queue(caller, cast);
                        }
                        Value::Int(0)
                    }
                    _ => Value::Int(0),
                }
            }
            // At once, as TSL's executable uses a skill: stealth turns its
            // mode off, or on where the player's stealth key would (ranks,
            // a stealth unit worn, out of combat); the other skills' uses
            // are not kept (a gap, by skill).
            ActionUseSkill => {
                let skill = int(args, 0);
                let stealth = self.library.constant("SKILL_STEALTH") == Some(skill);
                match self.character(caller) {
                    Some(c) if stealth => {
                        use crate::combat::sheet::{flags, skills};
                        let on = !c.stealth
                            && c.worn.skills[skills::STEALTH] > 0
                            && c.worn.flags & flags::STEALTH_UNIT != 0
                            && !c.in_combat;
                        self.out.changes.push(Change::Stealth { who: caller, on });
                    }
                    _ => {
                        let gap = format!("ActionUseSkill (skill {skill})");
                        *self.report.gaps.entry(gap).or_insert(0) += 1;
                    }
                }
                Value::Int(0)
            }
            // What the events kept (`last`): the attack's weapon, the
            // blocking door (its walker's `OnBlocked`, `world::scripts`), the
            // blocking creature and the hostile met going round one (none
            // here: the walker steps round bodies itself, and
            // `k_def_pathfail01` never runs), the item a disturbance moved.
            GetLastWeaponUsed | GetBlockingCreature | GetFoundEnemyCreature => {
                let kind = match bound {
                    GetLastWeaponUsed => last::WEAPON,
                    GetBlockingCreature => last::BLOCKING_CREATURE,
                    _ => last::FOUND_ENEMY,
                };
                maybe(
                    state
                        .last
                        .get(&(object(args, 0).0, kind))
                        .map(|id| ObjectId(*id)),
                )
            }
            GetBlockingDoor | GetInventoryDisturbItem => {
                let kind = if bound == GetBlockingDoor {
                    last::BLOCKING_DOOR
                } else {
                    last::DISTURB_ITEM
                };
                maybe(state.last.get(&(caller.0, kind)).map(|id| ObjectId(*id)))
            }
            // Every creature here of one faction joins another and stops
            // (the Star Forge's turned Jedi).
            SurrenderByFaction => {
                if let (Ok(from), Ok(faction)) =
                    (u8::try_from(int(args, 0)), u8::try_from(int(args, 1)))
                {
                    for thing in view.things {
                        if thing
                            .character
                            .as_ref()
                            .is_some_and(|c| !c.party && c.faction == from)
                        {
                            self.out.changes.push(Change::Faction {
                                who: thing.id,
                                faction,
                            });
                            self.stop(thing.id);
                        }
                    }
                }
                Value::Int(0)
            }
            ClearAllActions => {
                // What it plays stops too, even a clip that outlasts its
                // queued time.
                if !state.uncommandable.contains(&caller.0) {
                    clear_queue(state, self.view, caller);
                    self.out.changes.push(Change::Clear { who: caller });
                }
                Value::Int(0)
            }
            GetCurrentAction => {
                let front = state
                    .actions
                    .get(&object(args, 0).0)
                    .and_then(|queue| queue.first());
                let name = front.map_or("ACTION_INVALID", |queued| queued.action.constant());
                self.action_value(name)
            }
            SetCommandable => {
                let target = object(args, 1);
                if int(args, 0) != 0 {
                    state.uncommandable.remove(&target.0);
                } else {
                    state.uncommandable.insert(target.0);
                }
                Value::Int(0)
            }
            GetCommandable => truth(!state.uncommandable.contains(&object(args, 0).0)),
            SetFacing => {
                self.out.changes.push(Change::Face {
                    who: caller,
                    yaw: float(args, 0),
                });
                Value::Int(0)
            }
            SetFacingPoint => {
                if let Some(from) = self.position(caller) {
                    let to = vector(args, 0);
                    let yaw = crate::math::atan2_deg(to[1] - from[1], to[0] - from[0]);
                    self.out.changes.push(Change::Face { who: caller, yaw });
                }
                Value::Int(0)
            }
            Random => {
                let bound = int(args, 0);
                Value::Int(if bound > 0 {
                    self.rng.below(bound as u32) as i32
                } else {
                    0
                })
            }
            Dice(sides) => {
                let n = int(args, 0).max(1);
                Value::Int((0..n).map(|_| self.rng.below(sides) as i32 + 1).sum())
            }
            IntToString => string(int(args, 0).to_string()),
            FloatToString => {
                let (width, decimals) = (
                    int(args, 1).max(0) as usize,
                    int(args, 2).clamp(0, 9) as usize,
                );
                // With no decimals the game keeps the point ("4."): the
                // swoop's heartbeat builds "camshake4" and "cDistL4" from
                // it and cuts its last character off [verify in the game].
                let point = if decimals == 0 { "." } else { "" };
                let number = format!("{:.decimals$}{point}", float(args, 0));
                string(format!("{number:>width$}"))
            }
            IntToFloat => Value::Float(int(args, 0) as f32),
            FloatToInt => Value::Int(float(args, 0) as i32),
            StringToInt => Value::Int(leading_int(text(args, 0))),
            GetStringLength => Value::Int(text(args, 0).chars().count() as i32),
            GetSubString => {
                let skip = int(args, 1).max(0) as usize;
                let take = int(args, 2).max(0) as usize;
                string(
                    text(args, 0)
                        .chars()
                        .skip(skip)
                        .take(take)
                        .collect::<String>(),
                )
            }
            GetStringLeft => string(
                text(args, 0)
                    .chars()
                    .take(int(args, 1).max(0) as usize)
                    .collect::<String>(),
            ),
            GetStringRight => {
                let chars: Vec<char> = text(args, 0).chars().collect();
                let take = (int(args, 1).max(0) as usize).min(chars.len());
                string(chars[chars.len() - take..].iter().collect::<String>())
            }
            Vector => Value::Vector([float(args, 0), float(args, 1), float(args, 2)]),
            VectorMagnitude => Value::Float(distance(vector(args, 0), [0.0; 3])),
            // A zero vector stays zero.
            VectorNormalize => {
                let v = vector(args, 0);
                let length = distance(v, [0.0; 3]);
                Value::Vector(if length > 0.0 {
                    v.map(|axis| axis / length)
                } else {
                    v
                })
            }
            Abs => Value::Int(int(args, 0).wrapping_abs()),
            Fabs => Value::Float(float(args, 0).abs()),
            SendMessageToPC => {
                let said = text(args, 1).to_string();
                self.trace(&format!("  tells {}: {said:?}", name(object(args, 0))));
                self.tell(Some(object(args, 0)), Notice::Said(said))
            }
            OpenStore => self.open_store(
                object(args, 0),
                object(args, 1),
                [int(args, 2), int(args, 3)],
            ),
            // TSL's, its item creation not disabled: the workbench
            // (`script::workbench`), of the character given, else the
            // player; upgrading too unless disabled; the table a script
            // names, else the bench's own.
            ShowUpgradeScreen | ShowChemicalUpgradeScreen
                if bound == ShowChemicalUpgradeScreen || (args.len() > 1 && int(args, 2) == 0) =>
            {
                let lab = bound == ShowChemicalUpgradeScreen;
                let given = object(args, usize::from(!lab));
                let maker = Some(given)
                    .filter(|given| *given != ObjectId::INVALID)
                    .or_else(|| self.pcs().first().copied())
                    .unwrap_or(ObjectId::INVALID);
                let list = match lab {
                    true => crate::inventory::workshop::LAB.to_string(),
                    false => text(args, 4).to_ascii_lowercase(),
                };
                let bench = crate::action::Workbench {
                    list,
                    lab,
                    maker: maker.0,
                };
                let item = Some(object(args, 0)).filter(|item| !lab && *item != ObjectId::INVALID);
                let screen =
                    self.state
                        .bench(&bench, item.map(|item| item.0), !lab && int(args, 3) == 0);
                self.tell(None, Notice::Show(screen))
            }
            ShowUpgradeScreen | ShowChemicalUpgradeScreen => {
                let item = object(args, 0);
                let item = (item != ObjectId::INVALID).then_some(item.0);
                self.tell(None, Notice::Show(Screen::Upgrade { item }))
            }
            ShowLevelUpGUI => self.tell(None, Notice::Show(Screen::LevelUp)),
            // The galaxy map (`script::state::Galaxy`): the planets scripts
            // offer, and the one a player picked to travel to.
            ShowGalaxyMap => match u8::try_from(int(args, 0)) {
                Ok(planet) => {
                    let shown = self.state.galaxy.shown(planet);
                    self.tell(None, Notice::Show(shown))
                }
                Err(_) => Value::Int(0),
            },
            SetPlanetSelectable | SetPlanetAvailable => {
                if let Ok(planet) = u8::try_from(int(args, 0)) {
                    let galaxy = &mut self.state.galaxy;
                    let set = match bound {
                        SetPlanetSelectable => &mut galaxy.selectable,
                        _ => &mut galaxy.available,
                    };
                    if int(args, 1) != 0 {
                        set.insert(planet);
                    } else {
                        set.remove(&planet);
                    }
                }
                Value::Int(0)
            }
            GetPlanetSelectable | GetPlanetAvailable => {
                let galaxy = &self.state.galaxy;
                let set = match bound {
                    GetPlanetSelectable => &galaxy.selectable,
                    _ => &galaxy.available,
                };
                truth(u8::try_from(int(args, 0)).is_ok_and(|planet| set.contains(&planet)))
            }
            // The dialogue window's state: nothing kept here to reset.
            ResetDialogState => Value::Int(0),
            GetSelectedPlanet => Value::Int(self.state.galaxy.selected.map_or(-1, i32::from)),
            // Kept until a player closes it (`Action::ChooseParty`).
            ShowPartySelectionGUI => {
                let forced: Vec<u8> = [int(args, 1), int(args, 2)]
                    .into_iter()
                    .filter_map(|npc| u8::try_from(npc).ok())
                    .collect();
                let roster = self.state.party.roster(&forced, true, &self.state.items);
                self.state.party.selection = Some(super::party::Selection {
                    exit: key(text(args, 0)),
                    forced,
                });
                self.tell(None, Notice::Show(Screen::PartySelection(roster)))
            }
            // For the party, one player at the table (I30 P20, R9); its end
            // script runs for the opponent named, else this caller.
            PlayPazaak => {
                let call = super::pazaak::Call {
                    deck: int(args, 0),
                    end_script: text(args, 1),
                    max_wager: int(args, 2),
                    tutorial: int(args, 3) != 0,
                };
                let opponent = object(args, 4);
                let runs_for = if view.thing(opponent).is_some() {
                    opponent
                } else {
                    caller
                };
                // Whoever asked for it plays, alone: the guest whose pick
                // or use led here, else the PC speaking (I30 P8).
                let pc =
                    self.state.talk.running.unwrap_or_else(|| {
                        self.pcs().first().copied().unwrap_or(ObjectId::INVALID)
                    });
                let player = self.state.talk.asker_of_running(pc).unwrap_or(pc);
                let players = (runs_for, player);
                if super::pazaak::start(self.library, self.state, &call, players) {
                    let talk = super::dialogue::running(&self.state.talk, &self.event);
                    super::pazaak::watch_begun(view, self.state, player, talk);
                    self.tell(Some(player), Notice::Show(Screen::Pazaak))
                } else {
                    self.trace("  no pazaak: its player plays a match already, or no decks");
                    Value::Int(0)
                }
            }
            GetLastPazaakResult => Value::Int(i32::from(self.state.pazaak_won)),
            PopUpDeathGUIPanel => self.tell(Some(object(args, 0)), Notice::Show(Screen::Death)),
            DisplayMessageBox => self.tell(
                None,
                Notice::Show(Screen::MessageBox {
                    strref: int(args, 0),
                }),
            ),
            PlayMovie => {
                let movie = text(args, 0);
                self.trace(&format!("  plays the movie {movie}"));
                self.tell(None, Notice::Show(Screen::movie(movie)))
            }
            // Seconds of wait and of ramp, then the colour; from as covered
            // as the screen is now.
            SetGlobalFadeOut | SetGlobalFadeIn => {
                let fade = *self.out.set_fade(
                    view,
                    (float(args, 0), float(args, 1)),
                    [2, 3, 4].map(|at| float(args, at)),
                    bound == SetGlobalFadeOut,
                );
                self.trace(&format!(
                    "  fades {} after {} ms over {} ms",
                    if fade.out { "out" } else { "in" },
                    fade.wait_ms,
                    fade.length_ms
                ));
                Value::Int(0)
            }
            // Only as the module begins: its fade in waits for a
            // conversation (`snapshot::WorldFadeIn`).
            HoldWorldFadeInForDialog => {
                use crate::snapshot::WorldFadeIn;
                if self.out.world_fade_in(view) == WorldFadeIn::AfterArrival {
                    self.out.world_fade_in = Some(WorldFadeIn::AtDialog);
                }
                Value::Int(0)
            }
            // The fade standing (the LevelLoad one, in a module's load
            // scripts) is lifted only by `SetGlobalFadeIn`.
            SetFadeUntilScript => {
                if let Some(fade) = self.out.fade(view) {
                    self.out.fade = Some(crate::snapshot::Fade {
                        locked: true,
                        ..fade
                    });
                }
                Value::Int(0)
            }
            // A `videoeffects.2da` row, on until disabled
            // (`snapshot::Snapshot::video_effect`).
            EnableVideoEffect => {
                let row = int(args, 0);
                self.trace(&format!("  video effect {row}"));
                if let Ok(row) = u16::try_from(row) {
                    self.out.video_effect = Some(Some(row));
                }
                Value::Int(0)
            }
            DisableVideoEffect => {
                self.out.video_effect = Some(None);
                Value::Int(0)
            }
            // The movies (`sMovie1` to `sMovie6`) play over the load.
            StartNewModule => {
                let (module, tag) = (key(text(args, 0)), text(args, 1));
                self.trace(&format!("  starts the module {module} at {tag:?}"));
                let movies: Vec<String> = (2..8)
                    .map(|at| text(args, at).to_string())
                    .filter(|movie| !movie.is_empty())
                    .collect();
                // A co-op race's run finishing holds it (and its movies) for
                // the others.
                if !super::minigame::leave(self, caller, (&module, tag), &movies) {
                    for movie in &movies {
                        self.tell(None, Notice::Show(Screen::movie(movie)));
                    }
                    self.out.new_module = Some(super::NewModule {
                        by: caller,
                        module,
                        tag: tag.to_string(),
                    });
                }
                Value::Int(0)
            }
            RoundsToSeconds
            | HoursToSeconds
            | TurnsToSeconds
            | AngleToVector
            | GetGameDifficulty
            | IsMoviePlaying
            | QueueMovie
            | PlayMovieQueue
            | SetInputClass
            | UnlockAllSongs
            | PauseGame
            | EnableRain
            | ShowDemoScreen
            | YavinHackDoorClose
            | SoundObjectGetPitchVariance
            | SoundObjectSetPosition
            | NoClicksFor
            | ActionBarkString
            | SetKeepStealthInDialog
            | DisplayFeedBackText
            | ShowTutorialWindow
            | DisableMap
            | SetDisableTransit
            | SetReturnStrref
            | ChangeObjectAppearance
            | DuplicateHeadAppearance
            | PlayOverlayAnimation
            | DisplayDatapad
            | CancelPostDialogCharacterSwitch => self.presentation_call(bound, caller, args),
            Print => {
                let message = args.iter().find_map(|arg| match arg {
                    Value::String(text) => Some(text.to_string()),
                    _ => None,
                });
                self.trace(&format!("  says {:?}", message.unwrap_or_else(String::new)));
                Value::Int(0)
            }
        }
    }

    /// `OpenStore`: `player` trades with `store` (its markup and markdown
    /// raised by `bonus`), the store's `OnOpenStore` runs, and its screen
    /// opens for that player. Nothing for what is not a store or a player.
    fn open_store(&mut self, store: ObjectId, player: ObjectId, bonus: [i32; 2]) -> Value {
        // A guest who clicked the merchant trades: the store opens for the
        // body that asked, not for the host's character speaking (I30 P8).
        let asked = self.state.talk.asker_of_running(player);
        let player = asked.unwrap_or(player);
        let terms = self.instance(store).and_then(|facts| facts.store);
        let Some(terms) = terms.filter(|_| asked.is_some() || self.pcs().contains(&player)) else {
            self.trace(&format!("  no store {} for {}", name(store), name(player)));
            return Value::Int(0);
        };
        self.state
            .open_trade(player.0, store.0, terms, (bonus[0], bonus[1]));
        if let Some(script) = self.view.hook(store, Hook::OpenStore) {
            self.state.pending.push(Pending::new(&script, store));
        }
        self.tell(
            Some(player),
            Notice::Show(Screen::Store {
                store: store.0,
                markup: bonus[0],
                markdown: bonus[1],
            }),
        )
    }

    /// Something for one character's player, or (none) every player's.
    fn tell(&mut self, to: Option<ObjectId>, notice: Notice) -> Value {
        self.out.notices.push((to, notice));
        Value::Int(0)
    }

    /// `ExecuteScript`: `script` from its start for `caller`, nested, its
    /// `GetRunScriptVar` `script_var`.
    fn execute(&mut self, script: &str, caller: ObjectId, script_var: i32) {
        let library = self.library;
        let Some(program) = self.report.program(library, script) else {
            return;
        };
        if self.depth >= MOST_NESTED {
            self.report
                .faults
                .push(format!("{script}: nested too deep"));
            return;
        }
        *self.report.runs.entry(script.to_string()).or_insert(0) += 1;
        self.trace(&format!("run {script} for {}", name(caller)));
        let mut nested = Engine {
            library,
            view: self.view,
            state: &mut *self.state,
            rng: &mut *self.rng,
            report: &mut *self.report,
            out: &mut *self.out,
            iterators: &mut *self.iterators,
            spell: self.spell,
            script,
            event: Event {
                script_var,
                ..self.event.clone()
            },
            depth: self.depth + 1,
        };
        let outcome = vm::run(program, library.routines(), caller, &mut nested);
        finish(self.report, script, outcome);
    }
}

/// The distance between two points (metres).
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d: [f32; 3] = std::array::from_fn(|i| a[i] - b[i]);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// `StringToInt`: an optional sign and the digits that lead, else 0.
fn leading_int(text: &str) -> i32 {
    let text = text.trim_start();
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse::<i32>().map_or(0, |v| sign * v)
}

impl Engine<'_> {
    /// The item routines (I19, `script::items`). Equipping, giving and
    /// taking happen at once rather than queued (KOTOR's take no time).
    fn item_call(&mut self, bound: Bound, caller: ObjectId, args: &[Value]) -> Value {
        use Bound::*;
        let view = self.view;
        let catalog = self.library.items();
        let item = object(args, 0);
        fn facts<'a>(
            state: &'a State,
            catalog: &'a inventory::Catalog,
            item: ObjectId,
        ) -> Option<&'a inventory::ItemFacts> {
            state
                .items
                .get(&item.0)
                .and_then(|item| state.item_facts(catalog, &item.template))
        }
        let purse = |who: ObjectId| {
            view.thing(who)
                .and_then(|thing| thing.character)
                .map(|_| items::holder(view, who))
        };
        match bound {
            // A component or chemical made on a creature is a count, no
            // item (`State::pool_carried`).
            CreateItemOnObject => {
                let on = object(args, 1);
                let holder = items::holder(view, on);
                let stack = int(args, 2).clamp(1, i32::from(u16::MAX)) as u16;
                let mut made = self
                    .state
                    .make_item(catalog, text(args, 0), holder, stack, None);
                if self.is_creature(on) {
                    self.pool(holder);
                    made = made.filter(|made| self.state.items.contains_key(&made.0));
                    if made.is_none() {
                        return Value::Object(ObjectId::INVALID);
                    }
                }
                match made {
                    // TSL's fourth argument hides it; K1 has none.
                    Some(made) if self.is_pc(on) && int(args, 3) == 0 => self.received(made),
                    Some(_) => {}
                    None => self.trace(&format!("  no item template {:?}", text(args, 0))),
                }
                Value::Object(made.unwrap_or(ObjectId::INVALID))
            }
            // An object of the area, lying where the location is (Taris's
            // duel: Duncan's dropped pistol), drawn there (`Goods::floor`)
            // until a script destroys or takes it.
            CreateItemOnFloor => {
                let module = &view.module.name;
                let made = self
                    .place(args, 1)
                    .and_then(|place| self.state.lay_item(catalog, text(args, 0), module, place));
                if made.is_none() {
                    self.trace(&format!("  no item template {:?}", text(args, 0)));
                }
                maybe(made)
            }
            GetItemInSlot => Value::Object(
                u8::try_from(int(args, 0))
                    .ok()
                    .and_then(|slot| self.state.in_slot(object(args, 1).0, slot))
                    .unwrap_or(ObjectId::INVALID),
            ),
            GetFirstItemInInventory => {
                let carried = self.state.carried(items::holder(view, item));
                self.first(caller, walk::INVENTORY, carried)
            }
            GetNextItemInInventory => self.next(caller, walk::INVENTORY),
            GetItemPossessedBy => {
                let who = item;
                let tag = key(text(args, 1));
                let holder = items::holder(view, who);
                let tagged = |(_, held): &(u32, &inventory::Item)| {
                    self.state
                        .item_facts(catalog, &held.template)
                        .is_some_and(|facts| facts.tag == tag)
                };
                let items = &self.state.items;
                // Its own (worn or carried), or its holder's carried: the
                // first by id.
                let own = items.held_by(who.0).find(tagged);
                let carried = items
                    .held_by(holder)
                    .filter(|(_, held)| held.slot.is_none())
                    .find(tagged);
                let found = own.into_iter().chain(carried).map(|(id, _)| id).min();
                Value::Object(found.map_or(ObjectId::INVALID, ObjectId))
            }
            GetItemPossessor => Value::Object(possessor(self.state, view, item)),
            GetBaseItemType => Value::Int(facts(self.state, catalog, item).map_or_else(
                || self.library.constant("BASE_ITEM_INVALID").unwrap_or(256),
                |facts| i32::from(facts.base_item),
            )),
            // An `itempropdef.2da` row in force on it, its upgrades fitted
            // with theirs.
            GetItemHasItemProperty => truth(
                self.state
                    .items
                    .get(&item.0)
                    .zip(facts(self.state, catalog, item))
                    .zip(u8::try_from(int(args, 1)).ok())
                    .is_some_and(|((held, facts), row)| {
                        facts
                            .fitted(held.upgrades, &held.fitted, catalog)
                            .has_row(row)
                    }),
            ),
            GetItemStackSize | GetNumStackedItems => Value::Int(
                self.state
                    .items
                    .get(&item.0)
                    .map_or(0, |held| i32::from(held.stack)),
            ),
            // A player's character's stack grown is an item received, cut
            // items lost.
            SetItemStackSize => {
                let most = facts(self.state, catalog, item).map_or(1, |facts| facts.stacking);
                let told = self.pc_holds(item);
                let stack = int(args, 1).clamp(1, i32::from(most)) as u16;
                let was = self
                    .state
                    .items
                    .get_mut(&item.0)
                    .map(|mut held| std::mem::replace(&mut held.stack, stack));
                match was.map(|was| stack.cmp(&was)) {
                    Some(Ordering::Greater) if told => self.received(item),
                    Some(Ordering::Less) if told => self.out.statuses.push(StatusKind::Lost),
                    _ => {}
                }
                Value::Int(0)
            }
            GetItemCharges => Value::Int(
                self.state
                    .items
                    .get(&item.0)
                    .map_or(0, |held| i32::from(held.charges)),
            ),
            SetItemCharges => {
                if let Some(mut held) = self.state.items.get_mut(&item.0) {
                    held.charges = int(args, 1).clamp(0, 255) as u8;
                }
                Value::Int(0)
            }
            // An item goes at once; anything else after its delay, a
            // creature or placeable then fading out unless asked not to.
            DestroyObject => {
                if self.state.items.contains_key(&item.0) {
                    if self.pc_holds(item) {
                        self.out.statuses.push(StatusKind::Lost);
                    }
                    self.state.destroy_item(self.out, item);
                } else if let Some(thing) = view.thing(item).filter(|thing| {
                    !(thing.character.is_some() && self.state.kept.contains(&item.0))
                }) {
                    // A creature `SetIsDestroyable(FALSE)` keeps is not
                    // destroyed: both executables' `CSWSCreature::
                    // EventHandler` pass the destroy event over for one not
                    // destroyable (its flag, object +0x134, tested as the
                    // event fires; K1's Mac build at 0x10048cbab) (506OND's
                    // `a_end_game` destroys Tobin's body, then raises him for
                    // Kreia's scene).
                    self.out.changes.push(Change::Destroy {
                        who: item,
                        ms: seconds_ms(float(args, 1)),
                        going: destroy_going(thing.kind, int(args, 2) != 0, float(args, 3)),
                    });
                }
                Value::Int(0)
            }
            // One of a stack is worn, the rest left carried (KOTOR stacks
            // like weapons: Calo Nord's two heavy blasters are one stack his
            // scene equips twice, right hand then left).
            ActionEquipItem => {
                if let Ok(slot) = u8::try_from(int(args, 1)) {
                    let item = self.state.one_of(item);
                    let back = items::holder(view, caller);
                    self.state.equip(self.out, item, caller.0, slot, back);
                    super::forfeit_equip(self.library, view, self.state, caller, (item.0, slot));
                }
                Value::Int(0)
            }
            ActionUnequipItem => {
                let to = items::holder(view, caller);
                self.state.carry(self.out, item, to);
                Value::Int(0)
            }
            // Taken from a player's character: items lost.
            ActionTakeItem => {
                let to = items::holder(view, caller);
                let from = self.state.items.get(&item.0).map(|held| held.holder);
                let lost = self.pc_holds(item) && !self.is_pc(caller);
                self.state.carry(self.out, item, to);
                let moved = self.state.items.get(&item.0).map(|held| held.holder) != from;
                if lost && moved {
                    self.out.statuses.push(StatusKind::Lost);
                }
                if moved && self.is_creature(caller) {
                    self.pool(to);
                }
                Value::Int(0)
            }
            // Given a player's character: an item received (a component or
            // chemical: a count).
            ActionGiveItem | GiveItem => {
                let receiver = object(args, 1);
                let had = self.pc_holds(item);
                let to = items::holder(view, receiver);
                self.state.carry(self.out, item, to);
                if self.is_creature(receiver) {
                    self.pool(to);
                }
                if self.is_pc(receiver) && !had && self.pc_holds(item) {
                    self.received(item);
                }
                Value::Int(0)
            }
            // A character's gold: the party's for a member, else its own
            // (left with its remains); nothing else carries any.
            GetGold => Value::Int(purse(item).map_or(0, |holder| self.state.gold_of(holder))),
            // A player's character's credits moved are told by how many.
            GiveGoldToCreature => {
                if let Some(holder) = purse(item) {
                    let given = self.state.add_gold(holder, int(args, 1).max(0));
                    if given != 0 && self.is_pc(item) {
                        self.out.statuses.push(StatusKind::Credits(given));
                    }
                }
                Value::Int(0)
            }
            TakeGoldFromCreature => {
                let from = object(args, 1);
                if let Some(holder) = purse(from) {
                    let taken = self.state.add_gold(holder, -int(args, 0).max(0));
                    if taken != 0 && self.is_pc(from) {
                        self.out.statuses.push(StatusKind::Credits(taken));
                    }
                }
                Value::Int(0)
            }
            // An item's flag, or a character's (its sheet's `PLOT`: no
            // damage at all; a scene's creature made mortal when it acts).
            GetPlotFlag => truth(match self.state.items.get(&item.0) {
                Some(held) => held.plot,
                None => self
                    .character(item)
                    .is_some_and(|c| self.out.sheet(item, c.worn).flags & flags::PLOT != 0),
            }),
            SetPlotFlag => {
                if let Some(mut held) = self.state.items.get_mut(&item.0) {
                    held.plot = int(args, 1) != 0;
                } else if let Some(c) = self.character(item) {
                    let mut sheet = self.out.sheet(item, c.worn);
                    if int(args, 1) != 0 {
                        sheet.flags |= flags::PLOT;
                    } else {
                        sheet.flags &= !flags::PLOT;
                    }
                    self.out.changes.push(Change::Sheet { who: item, sheet });
                }
                Value::Int(0)
            }
            GetIdentified => truth(
                self.state
                    .items
                    .get(&item.0)
                    .is_some_and(|held| held.identified),
            ),
            SetIdentified => {
                if let Some(mut held) = self.state.items.get_mut(&item.0) {
                    held.identified = int(args, 1) != 0;
                }
                Value::Int(0)
            }
            // An item no one may put on (a player's equip refused); one worn
            // stays worn [verify: whether the exe takes it off]. Leaving
            // Taris's tar_m02ae makes the Sith armour so (its client-leave
            // script).
            SetItemNonEquippable => {
                if let Some(mut held) = self.state.items.get_mut(&item.0) {
                    held.non_equippable = int(args, 1) != 0;
                }
                Value::Int(0)
            }
            _ => Value::Int(0),
        }
    }
}

impl Engine<'_> {
    /// Object `id`'s facts (its instance's), those scripts made this step
    /// counted: they stand in the world from the step's end, but a script
    /// may use one at once (`GetTag`, `AddPartyMember`).
    /// A player's persona (`persona`), by its own character.
    fn persona(&self, id: ObjectId) -> Option<&crate::persona::Persona> {
        self.view.thing(id)?.persona.as_ref()
    }

    pub(super) fn instance(&self, id: ObjectId) -> Option<&crate::module::ObjectData> {
        match self.view.thing(id) {
            Some(_) => self.view.facts(id),
            None => self
                .out
                .made
                .iter()
                .find(|made| made.id == id)
                .map(|made| made.facts.as_ref()),
        }
    }

    /// Whether `id` is an object of the world, those made this step counted.
    fn exists(&self, id: ObjectId) -> bool {
        (self.view.thing(id).is_some() || self.out.made.iter().any(|made| made.id == id))
            && !self.out.deleted.contains(&id)
    }

    /// Whether a script destroyed `id` this step (`DestroyObject` with no
    /// delay): it goes as the step's changes apply.
    fn destroyed(&self, id: ObjectId) -> bool {
        self.out
            .changes
            .iter()
            .any(|change| matches!(change, Change::Destroy { who, ms: 0, .. } if *who == id))
    }

    /// Whether `id` is a creature (one made this step too).
    fn is_creature(&self, id: ObjectId) -> bool {
        self.view
            .thing(id)
            .is_some_and(|thing| thing.character.is_some())
            || self
                .out
                .made
                .iter()
                .any(|made| made.id == id && made.facts.kind == crate::module::ObjectKind::Creature)
    }

    /// What creature holder `holder` (the party's, or a creature's) has
    /// just received counted as TSL's inventory counts it
    /// (`State::pool_carried`): the party's gains told to every player.
    fn pool(&mut self, holder: u32) {
        let gained = self.state.pool_carried(self.library, holder);
        for notice in gained.told() {
            self.out.notices.push((None, notice));
        }
    }

    /// A creature of `template` made at `at` facing `yaw`, stocked with its
    /// items: in the world from the end of the step (its spawn script runs
    /// the next), its id the step's next.
    fn make(&mut self, template: &str, at: [f32; 3], yaw: f32) -> Option<ObjectId> {
        super::make(
            self.library,
            self.view,
            self.state,
            self.out,
            template,
            at,
            yaw,
        )
    }

    /// The scripts' PCs (`View::pcs`): the creature the first seat is
    /// switched to in its place while it is.
    fn pcs(&self) -> Vec<ObjectId> {
        self.view.pcs(&self.state.party)
    }

    /// The PCs `GetFirstPC` walks: the creature each seat controls, the
    /// first seat's the companion it leads while it does (`world::lent`).
    /// KOTOR's player is the body it controls: TSL's scripts make the PC
    /// lead again (`SetPartyLeader(NPC_PLAYER)`) before they move
    /// `GetFirstPC()` (a_master_setup), and a trigger asking for
    /// `GetFirstPC()` fires for the companion leading (Goto's yacht's
    /// command centre, Mira at the head for the mines) [verify in the exe].
    fn controlled_pcs(&self) -> Vec<ObjectId> {
        let mut pcs = self.pcs();
        if let Some(led) = crate::world::lent(&self.state.party).map(|body| ObjectId(body.0)) {
            match pcs.first_mut() {
                Some(first) => *first = led,
                None => pcs.push(led),
            }
        }
        pcs
    }

    /// The companion leading the party, if one does: the one whose guest's
    /// act this run answers (`Event::leads`), else the one made leader
    /// (`world::lent`).
    fn leading(&self) -> Option<ObjectId> {
        self.event
            .leads
            .or_else(|| crate::world::lent(&self.state.party).map(|body| ObjectId(body.0)))
    }

    /// Whether `id` is a player's character: a PC, or the companion
    /// leading in its player's hands (`leading`; `GetIsPC`). What a script gives or
    /// takes from one is told in the status summary.
    pub(super) fn is_pc(&self, id: ObjectId) -> bool {
        self.pcs().contains(&id) || self.leading() == Some(id)
    }

    /// Whether item `item` is a player's character's: the party's (its
    /// shared inventory) or worn by one.
    fn pc_holds(&self, item: ObjectId) -> bool {
        self.state.items.get(&item.0).is_some_and(|held| {
            held.holder == inventory::PARTY || self.is_pc(ObjectId(held.holder))
        })
    }

    /// Item `item`, given a player's character, told in the status
    /// summary by its template.
    fn received(&mut self, item: ObjectId) {
        if let Some(held) = self.state.items.get(&item.0) {
            let template = held.template.clone();
            self.out.statuses.push(StatusKind::Received(template));
        }
    }

    /// The party as scripts index it: the leader first (KOTOR's index 0),
    /// then the PCs by seat and the companions in joining order (the one
    /// the first seat is switched to once, as its PC).
    fn party(&self) -> Vec<ObjectId> {
        let mut party = self.pcs();
        for id in self.state.party.creatures().map(ObjectId) {
            if !party.contains(&id) {
                party.push(id);
            }
        }
        if let Some(leader) = self.leading() {
            party.retain(|id| *id != leader);
            party.insert(0, leader);
        }
        party
    }

    /// `SwitchPlayerCharacter`: the first seat plays companion `npc` in
    /// place of its own character from now (`world::switch`), on its
    /// creature here, else on one made from its template beside the PC (as
    /// the party selection screen makes those it calls); `NPC_PLAYER` (−1)
    /// switches back. False for a companion not available (or a template
    /// the module's plan lacks), and nothing changes. The conversations
    /// held with the PC before go on with the PC after (the Leviathan's
    /// switches run in its conversations' scripts).
    fn switch_player(&mut self, npc: i32) -> Value {
        let before = self.view.first_pc(&self.state.party);
        let switched = self.switched_to(npc);
        let after = self.view.first_pc(&self.state.party);
        if after != before {
            self.state.talk.retarget(before, after);
        }
        truth(switched)
    }

    /// `switch_player`'s change to the party table: whether it made one.
    fn switched_to(&mut self, npc: i32) -> bool {
        const NPC_PLAYER: i32 = -1;
        // The character switched to leads (the first seat plays it), until
        // a portrait click or `SetPartyLeader` lends the lead again.
        if npc == NPC_PLAYER {
            self.state.party.switched = None;
            self.state.party.leader = None;
            return true;
        }
        let slot = u8::try_from(npc)
            .ok()
            .and_then(|npc| Some((npc, self.state.party.available.get(&npc)?.clone())));
        let Some((npc, slot)) = slot else {
            return false;
        };
        let here = slot.object.map(ObjectId).filter(|id| {
            self.exists(*id)
                && self
                    .view
                    .thing(*id)
                    .is_none_or(|thing| thing.character.is_some())
        });
        let body = here.or_else(|| {
            // On the PC's soles (a character's place is its body's origin).
            let pc = self.view.first_pc(&self.state.party);
            let (at, yaw) = match self.view.thing(pc) {
                Some(thing) => {
                    let [x, y, z] = thing.position;
                    let soles = z + crate::motion::SOLE_Z * collision::UNIT;
                    ([x, y, soles], thing.yaw)
                }
                None => {
                    let made = self.out.made.iter().find(|made| made.id == pc)?;
                    (made.at, made.yaw)
                }
            };
            let side = rules::scripts::moves::BESIDE_M;
            self.make(&slot.template, super::beside(at, yaw, side), yaw)
        });
        let Some(body) = body else {
            return false;
        };
        let party = &mut self.state.party;
        if let Some(slot) = party.available.get_mut(&npc) {
            slot.object = Some(body.0);
        }
        party.switched = Some(npc);
        party.leader = None;
        true
    }

    /// TSL's influence (`Party::influence`), as its exe's commands keep
    /// it: read as it stands (-1 unset); set, or moved from what it is
    /// (`START_INFLUENCE` where unset), within 0 to `MAX_INFLUENCE`; the
    /// table's `INFLUENCE_SLOTS` only. A change is told in the status
    /// summary, gained or lost (a set by which way it went, a move by its
    /// sign), and the companion's creature here follows it
    /// (`influence::moved`).
    fn influence_call(&mut self, bound: Bound, args: &[Value]) -> Value {
        use super::party::{INFLUENCE_SLOTS, MAX_INFLUENCE, START_INFLUENCE};
        use Bound::*;
        let Some(npc) = u8::try_from(int(args, 0))
            .ok()
            .filter(|npc| *npc < INFLUENCE_SLOTS)
        else {
            return Value::Int(0);
        };
        let party = &mut self.state.party;
        let mut now = party.influence(npc);
        let by = int(args, 1);
        let to = match bound {
            SetInfluence => by,
            ModifyInfluence => {
                if now < 0 {
                    now = i32::from(START_INFLUENCE);
                    party.influence.insert(npc, START_INFLUENCE);
                }
                now + by
            }
            _ => return Value::Int(now),
        }
        .clamp(0, i32::from(MAX_INFLUENCE));
        party.influence.insert(npc, to as u8);
        if to != now {
            let gained = match bound {
                SetInfluence => now < to,
                _ => by > 0,
            };
            super::influence::moved(self.library, self.view, self.state, self.out, npc, gained);
        }
        Value::Int(0)
    }

    /// TSL's `AdjustAlignment` on `who`, `shift` toward side `toward` (to
    /// neutral no further than `sheet::NEUTRAL`), as its executable's command and
    /// `CSWSCreatureStats::ModifyAlignment` do it. A player's own
    /// character moves, and unless `quiet` the `G_PC_Light_Total` or
    /// `G_PC_Dark_Total` global counts the points (to 127) and
    /// `G_PC_Align_Val` takes its new alignment, which every companion
    /// here then follows (`influence::follow`). A companion in the party
    /// moves its base alignment and follows from it; anyone else simply
    /// moves. Unless `quiet`, a shift on one in the party is told in the
    /// status summary, light or dark, though the end of the scale held it.
    fn adjust_alignment(&mut self, who: ObjectId, (toward, shift): (i32, i32), quiet: bool) {
        use super::influence;
        use alignment::{DARK_SIDE, LIGHT_SIDE, NEUTRAL};
        const LIGHT_TOTAL: &str = "G_PC_Light_Total";
        const DARK_TOTAL: &str = "G_PC_Dark_Total";
        let Some(sheet) = influence::sheet(self.view, self.out, who) else {
            return;
        };
        let was = i32::from(sheet.good_evil);
        let neutral = i32::from(crate::combat::sheet::NEUTRAL);
        let delta = match toward {
            LIGHT_SIDE => shift,
            DARK_SIDE => -shift,
            NEUTRAL if was > neutral => -(was - neutral).min(shift),
            NEUTRAL => (neutral - was).min(shift),
            _ => 0,
        };
        // The executable carries the shift as a 16-bit number.
        let delta = i32::from(delta as i16);
        let made = self.view.party.contains(&who);
        if made && !quiet {
            let counted = match toward {
                LIGHT_SIDE => Some((LIGHT_TOTAL, shift)),
                DARK_SIDE => Some((DARK_TOTAL, shift)),
                NEUTRAL if delta > 0 => Some((LIGHT_TOTAL, delta)),
                NEUTRAL if delta < 0 => Some((DARK_TOTAL, -delta)),
                _ => None,
            };
            if let Some((total, by)) = counted
                && declared(self.view.module, total, Global::Number)
            {
                let now = global(&self.state.numbers, total).copied().unwrap_or(0);
                if now != i32::from(i8::MAX) {
                    let to = (now + by).min(i32::from(i8::MAX));
                    set_global(&mut self.state.numbers, total, i32::from(to as i8));
                }
            }
        }
        let member = self
            .state
            .party
            .available
            .iter()
            .find(|(npc, slot)| slot.object == Some(who.0) && self.state.party.has(**npc))
            .map(|(npc, _)| *npc);
        let moved = |from: i32| (from + delta).clamp(0, 100) as u8;
        if made {
            let to = moved(was);
            influence::set_alignment(self.view, self.out, who, to);
            if !quiet {
                if declared(self.view.module, influence::PC_ALIGNMENT, Global::Number) {
                    set_global(
                        &mut self.state.numbers,
                        influence::PC_ALIGNMENT,
                        i32::from(to),
                    );
                }
                let here: Vec<ObjectId> = (0..influence::NPC_TAGS.len() as u8)
                    .filter_map(|npc| self.state.party.available.get(&npc)?.object)
                    .map(ObjectId)
                    .filter(|id| self.exists(*id))
                    .collect();
                for companion in here {
                    self.follow_influence(companion);
                }
            }
        } else if let Some(npc) = member {
            if let Some(base) = influence::base(self.view, self.out, self.state, npc, who) {
                self.state.party.bases.insert(npc, moved(i32::from(base)));
            }
            self.follow_influence(who);
        } else {
            influence::set_alignment(self.view, self.out, who, moved(was));
        }
        if !quiet && delta != 0 && (made || member.is_some()) {
            let light = delta > 0;
            self.out.statuses.push(StatusKind::Alignment { light });
        }
    }

    /// Companion `npc` out of the party (`RemovePartyMember`), the table
    /// keeping it as it stands, the puppet with it leaving too; its
    /// creature, if it was in. In TSL its effects end, as the executable's
    /// `RemoveMember` ends them.
    fn remove_member(&mut self, npc: u8) -> Option<ObjectId> {
        super::keep_npc(self.view, self.state, self.out, npc);
        let left = self.state.party.leave(npc).map(ObjectId)?;
        self.out.changes.push(Change::Leave { who: left });
        super::puppet_leaves(self.view, self.state, self.out, npc);
        if self.view.module.game == TSL {
            let after = self.state.applied.len() as u32;
            self.state.removed.push((left.0, None, after));
        }
        Some(left)
    }

    /// TSL's companion `who` takes the alignment its influence gives it
    /// (`influence::follow`).
    fn follow_influence(&mut self, who: ObjectId) {
        super::influence::follow(self.library, self.view, self.state, self.out, who);
    }

    /// TSL's puppets (`Party::puppets`, `PUP_*` 0 to 2), as its executable's
    /// party table keeps them: made available once, by template or by a
    /// creature (moved into the puppets' faction; the slot's creature stays
    /// unset); given to a companion whose creature is here; made (its
    /// creature here moved instead); taken into the party (two at most,
    /// each once, following and fighting as a companion does); its owner
    /// asked after (a companion in the party). An owner joining or leaving
    /// brings it or sends it away (`script::puppet_joins`).
    fn puppet_call(&mut self, bound: Bound, caller: ObjectId, args: &[Value]) -> Value {
        use Bound::*;
        /// `STANDARD_FACTION_PARTYPUPPET`.
        const PUPPETS_FACTION: u8 = 23;
        let pup = u8::try_from(int(args, 0))
            .ok()
            .filter(|pup| *pup < party::PUPPETS);
        let available = pup.filter(|pup| self.state.party.puppets.contains_key(pup));
        match bound {
            AddAvailablePUPByTemplate | AddAvailablePUPByObject => {
                let template = match bound {
                    AddAvailablePUPByTemplate => Some(key(text(args, 1))),
                    _ => {
                        let who = object(args, 1);
                        let template = self.instance(who).map(|data| data.template.clone());
                        if template.is_some()
                            && self.character(who).is_some()
                            && available.is_none()
                        {
                            let faction = PUPPETS_FACTION;
                            self.out.changes.push(Change::Faction { who, faction });
                        }
                        template
                    }
                };
                let added =
                    pup.filter(|_| available.is_none())
                        .zip(template)
                        .map(|(pup, template)| {
                            let puppet = super::party::Puppet {
                                template,
                                object: None,
                                owner: None,
                                joined: false,
                            };
                            self.state.party.puppets.insert(pup, puppet);
                        });
                truth(added.is_some())
            }
            AssignPUP => {
                let npc = u8::try_from(int(args, 1))
                    .ok()
                    .filter(|npc| *npc < party::INFLUENCE_SLOTS);
                let party = &self.state.party;
                let owner_here = npc
                    .and_then(|npc| party.available.get(&npc)?.object)
                    .is_some_and(|id| self.exists(ObjectId(id)));
                match (available, npc) {
                    (Some(pup), Some(npc)) if owner_here => {
                        if let Some(puppet) = self.state.party.puppets.get_mut(&pup) {
                            puppet.owner = Some(npc);
                        }
                        truth(true)
                    }
                    _ => truth(false),
                }
            }
            SpawnAvailablePUP => {
                let Some(puppet) =
                    available.and_then(|pup| self.state.party.puppets.get(&pup).cloned())
                else {
                    return Value::Object(ObjectId::INVALID);
                };
                let here = puppet
                    .object
                    .map(ObjectId)
                    .filter(|who| self.view.thing(*who).is_some());
                let made = self.place(args, 1).and_then(|(at, yaw)| match here {
                    Some(who) => {
                        self.out.changes.push(Change::Jump { who, to: at, yaw });
                        Some(who)
                    }
                    None => self.make(&puppet.template, at, yaw),
                });
                if let (Some(pup), Some(made)) = (available, made)
                    && let Some(puppet) = self.state.party.puppets.get_mut(&pup)
                {
                    puppet.object = Some(made.0);
                }
                Value::Object(made.unwrap_or(ObjectId::INVALID))
            }
            AddPartyPuppet => {
                let who = object(args, 1);
                let party = &self.state.party;
                let room = party.puppet_creatures().count() < party::PUPPETS_JOINED;
                let another = party
                    .puppets
                    .iter()
                    .any(|(of, puppet)| Some(*of) != available && puppet.object == Some(who.0));
                let fits = room && !another && self.exists(who);
                match available.and_then(|pup| self.state.party.puppets.get_mut(&pup)) {
                    Some(puppet) if fits && !puppet.joined => {
                        puppet.object = Some(who.0);
                        puppet.joined = true;
                        self.out.changes.push(Change::Join { who });
                        truth(true)
                    }
                    _ => truth(false),
                }
            }
            // The table keeps no puppet but its template: nothing more to
            // keep, the slot's creature unchanged (no TSL script calls it).
            SavePUPByObject => Value::Int(0),
            GetIsPuppet => truth(
                self.state
                    .party
                    .puppet_of(object(args, 0).0, true)
                    .is_some(),
            ),
            // The companion in the party it was given to, by its creature.
            GetPUPOwner => maybe(
                self.state
                    .party
                    .puppet_owner(object(args, 0).0)
                    .map(ObjectId),
            ),
            // A puppet in the party follows its owner until its queue is
            // cleared (`Action::FollowOwner`); anyone else queues nothing.
            // The executable takes the range as given; ours keeps it to
            // `FOLLOW_LEAST`, since a run onto the owner's own spot never
            // arrives here (the walker stops at its body) and would requeue
            // every step.
            ActionFollowOwner => match self.state.party.puppet_of(caller.0, true) {
                Some(_) => self.queue(
                    caller,
                    Action::FollowOwner {
                        range: float(args, 0).max(FOLLOW_LEAST),
                    },
                ),
                None => Value::Int(0),
            },
            _ => Value::Int(0),
        }
    }

    /// The party table's routines (`script::party`). `NPC_PLAYER` (−1) and
    /// other numbers out of range name no companion.
    fn party_call(&mut self, bound: Bound, args: &[Value]) -> Value {
        use Bound::*;
        let npc = u8::try_from(int(args, 0)).ok();
        let party = &mut self.state.party;
        match bound {
            AddAvailableNPCByObject | AddAvailableNPCByTemplate => {
                let template = match bound {
                    AddAvailableNPCByTemplate => Some(key(text(args, 1))),
                    _ => self
                        .instance(object(args, 1))
                        .map(|data| data.template.clone()),
                };
                let object = (bound == AddAvailableNPCByObject).then(|| object(args, 1).0);
                // TSL: the companion takes its influence's alignment, its
                // influence started (by a creature of the template, whose
                // tag names it, when made available by template).
                let facts = template
                    .as_deref()
                    .and_then(|template| self.library.creature(template));
                let tag = facts.map(|facts| facts.tag.to_ascii_lowercase());
                // By template, the template kept with it (KOTOR's table
                // keeps the creature): a module that lacks it makes it too.
                let template_data = facts
                    .filter(|_| bound == AddAvailableNPCByTemplate)
                    .map(|facts| crate::pack::creature_bytes(facts).into());
                let party = &mut self.state.party;
                let added = npc.zip(template).map(|(npc, template)| {
                    party.available.insert(
                        npc,
                        party::Npc {
                            template,
                            object,
                            kept: None,
                            template_data,
                        },
                    );
                });
                use super::influence::{KREIA, NPC_TAGS};
                match object {
                    Some(who) => self.follow_influence(ObjectId(who)),
                    None if self.library.influences() => {
                        if let Some(tag) = tag.filter(|tag| tag != KREIA)
                            && let Some(npc) = NPC_TAGS.iter().position(|of| *of == tag)
                        {
                            super::influence::start(self.library, self.state, npc as u8, &tag);
                        }
                    }
                    None => {}
                }
                truth(added.is_some())
            }
            RemoveAvailableNPC => {
                let gone = npc.and_then(|npc| {
                    let left = party.leave(npc);
                    party.available.remove(&npc).map(|_| left)
                });
                if let Some(Some(who)) = gone {
                    self.out.changes.push(Change::Leave { who: ObjectId(who) });
                }
                truth(gone.is_some())
            }
            IsAvailableCreature => truth(npc.is_some_and(|npc| party.available.contains_key(&npc))),
            AddPartyMember => {
                let who = object(args, 1);
                // One made this step (spawned, then asked to join) counts.
                let exists = self.exists(who);
                let party = &mut self.state.party;
                let joined = exists && npc.is_some_and(|npc| party.join(npc, who.0));
                if let (true, Some(npc)) = (joined, npc) {
                    self.state.takes_in(who.0);
                    self.out.changes.push(Change::Join { who });
                    self.follow_influence(who);
                    super::puppet_joins(self.library, self.view, self.state, self.out, npc, who);
                }
                truth(joined)
            }
            RemovePartyMember => truth(npc.and_then(|npc| self.remove_member(npc)).is_some()),
            // TSL's: out of the party, its creature sent away (faded out),
            // the table keeping it as it stood; whether one was sent away.
            RemoveNPCFromPartyToBase => {
                let Some(npc) = npc.filter(|npc| *npc < party::INFLUENCE_SLOTS) else {
                    return truth(false);
                };
                self.remove_member(npc);
                let here = self
                    .state
                    .party
                    .available
                    .get(&npc)
                    .and_then(|slot| slot.object)
                    .map(ObjectId)
                    .and_then(|who| Some((who, self.view.thing(who)?.kind)));
                let sent = here.filter(|_| !self.state.party.has(npc));
                if let Some((who, kind)) = sent {
                    if let Some(slot) = self.state.party.available.get_mut(&npc) {
                        slot.object = None;
                    }
                    // Deleted at once (`KillNPCObject`): the area's enter
                    // script that sends the party to base then finds the
                    // companion gone and makes it anew (403DXN's
                    // `k_403_enter` on the return for the strike on the
                    // tomb: `SpawnAvailableNPC` where `GetObjectByTag`
                    // finds none).
                    self.out.deleted.push(who);
                    self.out.changes.push(Change::fade_away(who, kind));
                }
                truth(sent.is_some())
            }
            // TSL's: each available companion's (and the Remote's) creature
            // in this module found again by its tag.
            RebuildPartyTable => {
                for (npc, tag) in super::influence::NPC_TAGS.iter().enumerate() {
                    let found = self.by_tag(tag, 0, Some(ObjectKind::Creature));
                    if let Some(slot) = self.state.party.available.get_mut(&(npc as u8))
                        && found != ObjectId::INVALID
                    {
                        slot.object = Some(found.0);
                    }
                }
                let remote = self.by_tag(REMOTE, 0, Some(ObjectKind::Creature));
                if let Some(puppet) = self.state.party.puppets.get_mut(&0)
                    && remote != ObjectId::INVALID
                {
                    puppet.object = Some(remote.0);
                }
                Value::Int(0)
            }
            // TSL's: the creature given kept as the companion's, its slot's
            // creature unchanged.
            SaveNPCByObject => {
                if let Some(npc) = npc.filter(|npc| *npc < party::INFLUENCE_SLOTS) {
                    super::keep_creature(self.view, self.state, self.out, npc, object(args, 1));
                }
                Value::Int(0)
            }
            IsNPCPartyMember => truth(npc.is_some_and(|npc| party.has(npc))),
            // The PC switched to is counted once (`party`), a member
            // without a creature here too.
            GetPartyMemberCount => {
                let switched = party.switched.filter(|npc| party.has(*npc));
                let members = party.members.len() - usize::from(switched.is_some());
                Value::Int((self.view.party.len() + members) as i32)
            }
            SetPartyLeader => {
                party.leader = npc.filter(|npc| party.has(*npc));
                truth(npc.is_none() || party.leader.is_some())
            }
            GetNPCAIStyle => Value::Int(
                party
                    .ai_styles
                    .get(&object(args, 0).0)
                    .copied()
                    .unwrap_or(0),
            ),
            SetNPCAIStyle => {
                party.ai_styles.insert(object(args, 0).0, int(args, 1));
                Value::Int(0)
            }
            GetPartyAIStyle => Value::Int(party.party_ai),
            SetPartyAIStyle => {
                party.party_ai = int(args, 0);
                Value::Int(0)
            }
            GetSoloMode => truth(party.solo),
            SetSoloMode => {
                party.solo = int(args, 0) != 0;
                Value::Int(0)
            }
            GetPlayerRestrictMode => truth(party.restrict),
            SetPlayerRestrictMode => {
                party.restrict = int(args, 0) != 0;
                Value::Int(0)
            }
            // -1 for a companion not in the party table at all, else
            // whether it may be picked: TSL's `CSWPartyTable::
            // GetNPCSelectability` gives 0xFF for one not available (or past
            // the table), which the script command turns to -1. TSL's Ebon
            // Hawk takes 1 or 0 as "available" (k_003ebo_enter, Visas's
            // arrival asking she is not yet).
            GetNPCSelectability => Value::Int(match npc {
                Some(npc) if party.available.contains_key(&npc) => {
                    i32::from(!party.unselectable.contains(&npc))
                }
                _ => -1,
            }),
            SetNPCSelectability => {
                if let Some(npc) = npc {
                    if int(args, 1) != 0 {
                        party.unselectable.remove(&npc);
                    } else {
                        party.unselectable.insert(npc);
                    }
                }
                Value::Int(0)
            }
            _ => Value::Int(0),
        }
    }
}

impl Host for Engine<'_> {
    fn action(
        &mut self,
        routine: &Routine,
        caller: ObjectId,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let mut gap = false;
        // Where the executables stop the script (`minigame::aborts`).
        if super::minigame::aborts(&routine.name) {
            *self.report.calls.entry(routine.name.clone()).or_insert(0) += 1;
            return Err(super::minigame::ABORTED.into());
        }
        let bound = self
            .library
            .bound
            .get(usize::from(routine.id))
            .copied()
            .flatten();
        // The minigames' routines go to their own module first.
        let minigame = routine
            .name
            .starts_with("SWMG_")
            .then(|| super::minigame::call(self, routine, caller, &args))
            .flatten();
        let value = match bound {
            _ if let Some(value) = minigame => {
                *self.report.calls.entry(routine.name.clone()).or_insert(0) += 1;
                value
            }
            Some(bound) => {
                *self.report.calls.entry(routine.name.clone()).or_insert(0) += 1;
                self.call(bound, caller, &args)
            }
            // An effect the sim does not carry out yet is still a value
            // (`script::effects`), counted where it is applied.
            None if routine.ret == script_ncs::Type::Effect => {
                *self.report.calls.entry(routine.name.clone()).or_insert(0) += 1;
                self.effect_value(Effect::Other(routine.name.clone()))
            }
            None => {
                *self.report.gaps.entry(routine.name.clone()).or_insert(0) += 1;
                gap = true;
                // A gap called as an action still takes its place in the
                // queue (`GetCurrentAction`, what follows waits on it).
                if routine.name.starts_with("Action") {
                    self.queue(
                        caller,
                        Action::Instant {
                            routine: routine.name.clone(),
                        },
                    );
                }
                Value::empty(routine.ret)
            }
        };
        // Kept as its parts, written out only if the trace keeps it
        // (`super::Traced`).
        let shown = match &value {
            Value::Int(_) if routine.ret == script_ncs::Type::Void => None,
            Value::Int(_) | Value::Float(_) | Value::Object(_) | Value::String(_) => {
                Some(value.clone())
            }
            _ => None,
        };
        let call = super::CallLine {
            ms: self.view.now_ms,
            routine: routine.name.clone(),
            args: args
                .into_iter()
                .map(|arg| match arg {
                    Value::Action(_) => None,
                    arg => Some(arg),
                })
                .collect(),
            gap,
            value: shown,
        };
        self.report.keep(super::Traced::Call(Box::new(call)));
        Ok(value)
    }
}

/// Local number `index` of `game` holding `value`, as the games' var tables
/// keep one: a byte, read back unsigned (-1 reads 255; a save from before
/// keeps a wider number, read as its byte too); `None` past the last slot
/// (K1 keeps `rules::scripts::locals::K1_NUMBERS`, TSL `TSL_NUMBERS`), where
/// nothing is kept and 0 is read.
pub(crate) fn local_number(game: &str, index: i32, value: i32) -> Option<i32> {
    let slots = if game == TSL {
        rules::scripts::locals::TSL_NUMBERS
    } else {
        rules::scripts::locals::K1_NUMBERS
    };
    (0..slots).contains(&index).then_some(value & 0xff)
}

/// Whether `game`'s objects keep local boolean `index`: K1's
/// `rules::scripts::locals::K1_BOOLEANS` slots, TSL's `TSL_BOOLEANS` (both
/// executables' var tables test the index unsigned, so one below 0 is past
/// them too).
pub(crate) fn local_boolean(game: &str, index: i32) -> bool {
    let slots = if game == TSL {
        rules::scripts::locals::TSL_BOOLEANS
    } else {
        rules::scripts::locals::K1_BOOLEANS
    };
    (0..slots).contains(&index)
}

/// What `GetLocalBoolean` reads of local boolean `index`, `set` or not:
/// both executables' `CSWVarTable::GetLocalBoolean` mask the bit in its
/// word (`1 << index` masked, the shift's count taken modulo 32 as x86's
/// `shl` takes it) and `ExecuteCommandGetLocalValue` pushes that as it is,
/// not 1: true reads 1 only for indices 0, 32, 64 and so on, and a script's
/// `== TRUE` on another is false, as in the games.
pub(crate) fn local_boolean_read(index: i32, set: bool) -> i32 {
    if set {
        1i32.wrapping_shl(index as u32)
    } else {
        0
    }
}

/// Whose locals `id`'s are: an object's own; the module's and the
/// area's, each module's own (they share the ids `MODULE` and `AREA`
/// across modules): a key in the range above objects' ids, from the
/// module's name [a hash: two modules' could meet; verify none do].
fn local_owner(module: &str, id: ObjectId) -> u32 {
    if id != MODULE && id != AREA {
        return id.0;
    }
    let hash = key(module).bytes().fold(0x811c_9dc5u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    0x7c00_0000 | (hash & 0x00ff_fffe) | u32::from(id == AREA)
}
