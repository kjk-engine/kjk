//! The routines K1's own scripts call once or twice, each bound as both
//! executables run it (TSL's symbolled `KOTOR2sub` read first, K1's checked
//! where they could differ): what holds an inventory and how it was last
//! disturbed, the item a module's `OnActivateItem` reads, an item's worth
//! changed, NWN's player parties, a party slot's creature, a visual at a
//! place, blaster deflection, damage by type, metamagic, the in-game page,
//! the button mash check, the music's volume and the cheat code. Several are
//! NWN's leftovers that change nothing in KOTOR: bound as the empty or near
//! empty handlers they are.

use script_ncs::vm::{ObjectId, Value};

use super::effects::Effect;
use super::engine::{Bound, Engine, float, int, last, object, text, truth};
use crate::combat::effects::Modifies;
use crate::module::ObjectKind;
use crate::notice::Notice;

/// `GetMetaMagicFeat`'s answer for one that never cast: both executables'
/// objects start with the byte at 0xff.
const NO_METAMAGIC: i32 = 0xff;

/// The party slots `SetAvailableNPCId` takes (`CSWPartyTable::SetNPCObject`
/// refuses one past 11, as the table holds 12).
const TABLE_SLOTS: i32 = 12;

/// The damage types `GetDamageDealtByType` reads: a single `DAMAGE_TYPE_*`
/// bit below `DAMAGE_FLAG_BASE_DAMAGE` (both executables' 1 << 14).
const BASE_DAMAGE: i32 = 1 << 14;

impl Engine<'_> {
    /// The routines of the module documentation.
    pub(super) fn rare_call(&mut self, bound: Bound, caller: ObjectId, args: &[Value]) -> Value {
        use Bound::*;
        let view = self.view;
        match bound {
            // `ExecuteCommandGetHasInventory`: a creature or a store holds
            // one; a placeable as its `HasInventory` says; an item only if
            // its base item is a container, which none of either game's is.
            GetHasInventory => truth(view.thing(object(args, 0)).is_some_and(|thing| {
                thing.character.is_some()
                    || match thing.kind {
                        Some(ObjectKind::Creature | ObjectKind::Store) => true,
                        Some(ObjectKind::Placeable) => {
                            thing.facts.as_ref().is_some_and(|facts| facts.container)
                        }
                        _ => false,
                    }
            })),
            // The caller's own, as its last disturbed event kept it (0,
            // `INVENTORY_DISTURB_TYPE_ADDED`, before any).
            GetInventoryDisturbType => Value::Int(
                self.state
                    .last
                    .get(&(caller.0, last::DISTURB_TYPE))
                    .map_or(0, |kind| *kind as i32),
            ),
            // The module keeps the item and its activator from the event
            // `EventActivateItem` builds, signalled to it; nothing else in
            // either executable raises that event, and no script of either
            // game builds one, so neither is ever set.
            GetItemActivated | GetItemActivator => Value::Object(ObjectId::INVALID),
            // `ExecuteCommandChangeItemCost`: the template is loaded, and
            // its base item's cost multiplier set, for every item of that
            // base item from then on (`State::value`; for as long as the game
            // runs, so not saved). A template that does not load does
            // nothing.
            ChangeItemCost => {
                let catalog = self.library.items();
                if let Some(base) = self
                    .state
                    .item_facts(catalog, &text(args, 0).to_ascii_lowercase())
                    .map(|facts| facts.base_item)
                {
                    self.state.cost_multipliers.insert(base, float(args, 1));
                }
                Value::Int(0)
            }
            // NWN's player parties: `AddToParty` adds a player character to
            // another's faction when both are player characters, and
            // `RemoveFromParty` gives a player character a faction of its
            // own. A companion is no player character, so a script's call on
            // one (K1's Star Forge sends Bastila and Carth off with it) does
            // nothing; a party's players share the party's faction here, and
            // a player's own faction apart from it is not made [verify:
            // nothing calls either on a player].
            AddToParty | RemoveFromParty => Value::Int(0),
            // `CSWPartyTable::SetNPCObject`: the creature an available
            // companion's slot is, for a slot of 0 to 11 that is available.
            SetAvailableNPCId => {
                let slot = int(args, 0);
                let npc = object(args, 1);
                if (0..TABLE_SLOTS).contains(&slot)
                    && let Some(available) = self.state.party.available.get_mut(&(slot as u8))
                {
                    available.object = (npc != ObjectId::INVALID).then_some(npc.0);
                }
                Value::Int(0)
            }
            // `CSWSArea::PlayVisualEffect`: the row (as a 16-bit number) plays
            // once at the place, in the player's area, for the players near.
            // As an instant visual put there by the party's leader, as
            // `ApplyEffectAtLocation` carries one.
            PlayVisualAreaEffect => {
                let leader = view.party.first().copied();
                if let (Some(leader), Some((at, _))) = (leader, self.place(args, 1)) {
                    let visual = Effect::Visual {
                        row: Some(int(args, 0) as u16),
                        from: None,
                        part: 0,
                    };
                    let instant = self.library.constant("DURATION_TYPE_INSTANT").unwrap_or(0);
                    self.apply_effect(instant, visual, (leader, Some(at)), caller, 0.0);
                }
                Value::Int(0)
            }
            // Kept and walked; it changes no deflection (`Modifies::
            // BlasterDeflection`).
            EffectBlasterDeflectionIncrease => {
                self.modifier(Modifies::BlasterDeflection, int(args, 0), 0)
            }
            EffectBlasterDeflectionDecrease => self.modifier(
                Modifies::BlasterDeflection,
                int(args, 0).saturating_neg(),
                0,
            ),
            // The caller's last damage (`last::DAMAGE_TYPES`): what it took
            // if it was of the type, else -1 (as before any damage). A type
            // that is not one `DAMAGE_TYPE_*` bit reads 0, as does a caller
            // that is not a creature, placeable or door; a door's or
            // placeable's damage is never kept, as both executables'
            // `OnApplyDamage` keep a creature's only.
            GetDamageDealtByType => {
                let kind = int(args, 0);
                let damageable = view.thing(caller).is_some_and(|thing| {
                    thing.character.is_some()
                        || matches!(
                            thing.kind,
                            Some(ObjectKind::Creature | ObjectKind::Placeable | ObjectKind::Door)
                        )
                });
                if !damageable || kind <= 0 || kind >= BASE_DAMAGE || kind.count_ones() != 1 {
                    return Value::Int(0);
                }
                let kept = |which| self.state.last.get(&(caller.0, which)).copied();
                Value::Int(match (kept(last::DAMAGE_TYPES), kept(last::DAMAGE_DEALT)) {
                    (Some(types), Some(dealt)) if types & kind as u32 != 0 => dealt as i32,
                    _ => -1,
                })
            }
            // The caller's last cast's (`last::METAMAGIC`).
            GetMetaMagicFeat => Value::Int(
                self.state
                    .last
                    .get(&(caller.0, last::METAMAGIC))
                    .map_or(NO_METAMAGIC, |meta| *meta as i32),
            ),
            // The page a seat shows (`Action::SubScreen`).
            GetSubScreenID => Value::Int(i32::from(self.state.subscreen)),
            SetButtonMashCheck => {
                self.state.button_mash = int(args, 0);
                Value::Int(0)
            }
            GetButtonMashCheck => Value::Int(self.state.button_mash),
            // The host's music plays at it (`Notice::MusicVolume`).
            SetMusicVolume => self.tell(None, Notice::music_volume(float(args, 0))),
            // TSL's answers 0. K1's answers, for code 0, whether its console's
            // `dancedancemalak` was entered (with `EnableCheats` in its ini);
            // there is no such console here.
            GetCheatCode => Value::Int(0),
            _ => Value::Int(0),
        }
    }
}
