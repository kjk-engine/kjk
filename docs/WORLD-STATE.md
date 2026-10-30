# World state (I19)

What the game keeps about its world beyond what moves and fights: items and
inventories, equipment, gold, the party table, stores, the journal, XP
and levels. The games' own scripts drive all of it (the holistic rule:
the engine carries out what KOTOR's scripts ask, no per-module patches),
on the authority; a save carries it.

## Items (`sim::inventory`, `sim::script::items`)

- **Templates.** The session reads every UTI the install lists (K1: 562 in
  about 10 ms per plan) into `ItemFacts`: its base item (`baseitems.2da`:
  stacking, equipable slots, armour defence and DEX cap, a weapon's dice,
  crits, damage types, feats and reach), its tag and name strref, stack,
  charges, cost, plot flag, its properties (below: a weapon's bonuses,
  guards by damage type, what it gives its wearer) and what it casts when used
  (its Cast Spell property: the `spells.2da` row and the
  `iprp_chargecost.2da` cost of a use, `inventory::ItemSpell`; templates
  KJKT 11). The catalog (`inventory::Catalog`) rides in the script library
  (`Library::with_items`, in its digest); where an install cannot list
  its resources, the module's objects' templates and the strings its
  scripts hold stand in.
- **Starting items.** A creature's `Equip_ItemList` (the struct id's bit
  is the slot) and `ItemList`, a placeable's or store's `ItemList`, become
  `ObjectData::inventory` (`Held`). On a module's first visit (with its
  spawn scripts) every object's become items in play.
- **Items in play** are script objects with their own ids
  (`inventory::FIRST_ITEM` on), kept in the scripts' state (`State::items`;
  a save carries them, KJKV 9): template, holder, slot, stack, charges,
  flags. KOTOR's items never stand in the world. The party carries one
  inventory (`inventory::PARTY`; `GetItemPossessor` gives the leader); a
  party member's equipment is its own. A stackable item made where a stack
  of its template has room joins it. An item carried into a module whose
  catalog lacks its template (one a module's own archive held: Gadon's
  papers) keeps its facts in the scripts' state (`State::known`, KJKV 16;
  KOTOR's item keeps its own data), rebuilt at each install.
- **Sheets.** A creature's sheet is read without its items and dressed by
  them (`inventory::dress`): the weapons in its hands, its body armour's
  defence and DEX cap, every worn item's guards and gear; what it carries gives the
  four kinds it uses in a fight. When a script changes what a character
  wears, the step ends by dressing it anew relative to what it wore
  (`inventory::rewear`: a hand no item touched keeps its weapon, a dev
  sheet's), and the sheet rides in the snapshot from then on (a creature's
  too), so every view and prediction sees it.
- **What players see** (`inventory::Goods`, the snapshot's `goods`,
  KJKS 26): the party's gold, the items it carries and its characters
  wear, the trades under way and their stores' stock. The authority's
  scripts gather it again when they ran, a trade happened or the party
  changed; a client reads it, never the scripts' state.
- **Routines:** `CreateItemOnObject`, `GetItemInSlot`,
  `GetFirst/NextItemInInventory`, `GetItemPossessedBy`, `GetItemPossessor`,
  `GetBaseItemType`, `Get/SetItemStackSize`, `GetNumStackedItems` (the
  stack), `Get/SetItemCharges`,
  `DestroyObject` (an item at once; anything else after its delay,
  then gone to scripts; see "Destroyed" below),
  `ActionEquipItem`, `ActionUnequipItem`, `ActionGiveItem`,
  `ActionTakeItem` (at once: KOTOR's take no time), `GetGold`,
  `GiveGoldToCreature`, `TakeGoldFromCreature` (a party member's the
  party's gold, another creature's its own: `State::purses`, left with its
  remains),
  `Get/SetPlotFlag`, `Get/SetIdentified` and `SetItemNonEquippable` (a
  flag of the item's, `Item::non_equippable`, KJKS 69 and KJKV 56: a
  player's equip refused) on
  items; `GetTag`,
  `GetIsObjectValid` and `GetObjectType` know items.
- **Items on the floor** (`CreateItemOnFloor`, `State::floor`): an item
  made at a location is an object of the area, held by it (`AREA`), its
  own stack: `GetItemPossessor` finds none; `GetObjectByTag`,
  `GetNearestObjectByTag`, `GetPosition`, `GetFacing`, `GetLocation` and
  the distances find it where it lies, in its module only; taken, or
  destroyed, it lies there no more. The snapshot carries the area's
  (`Goods::floor`, KJKS 66; a save `State::floor`, KJKV 53) and the views
  draw each as its base item's model at its `ModelVariation`, turned as
  `baseitems.2da` `rotateonground` lays it (1 a quarter about its Y axis,
  2 about X, as KOTOR's client builds the item's orientation; then its
  facing). Taris's duel drops Duncan's pistol so, which `k_ptar_fight`
  destroys as Gerlon shoots. Probes `floor items`, `weapons view` `item on
  the floor`.
- **Using items** (`world::item_uses`): an item used leaves its holder's
  goods in the step it is used (`State::spend`, the first carried item
  casting the row): one of its stack (single use), or its charges (the
  last spent, the item is gone, K1's "automatically discarded"), or
  nothing (unlimited). A grenade goes as it is thrown, a medpac as its use
  begins (its heal lands at the end, `Pack::using` by row), shields and
  stims at once. Every character's four kinds (`Sheet::items`: the HUD's
  slots, the item buttons') are its holder's goods, the party's for a
  member (`Change::Usable`, from the step's goods), those for its race
  (`ItemFacts::fits`, as Use Item and item talents have them: no repair
  kit for a human, no medpac for a droid), wherever the goods hold
  anything for it (a world without kept goods, an arena, keeps its
  sheets' own); `Pack::used` counts only the uses the goods do not show
  yet, and `Pack::last` the last use (the view's sounds; KJKS 40). A
  seat's `Action::UseItem { who, item, target }` (wire 25; whose by the
  equipping rule, `may_dress`) uses an item the party carries, worn in no
  slot, or one `who` wears, whose spell it may cast on that target
  (`script::items::spell_for`: a spell that is no hostile one on itself, a
  hostile one, `spells.2da` `hostilesetting`, on another; by those its base
  item is for, `baseitems.2da` `droidorhuman`, `ItemFacts::droids`, KJKT
  12): one use taken, its spell cast in that step (`State::use_item`), else
  `Notice::CannotUse`. On another, within the spell's reach (`spells.2da`
  `range` in `ranges.2da`, `Talent::reach_dm`, and a metre). A spell with
  no impact script in the library is, on oneself, what `combat::items`
  knows of its row: the fighter takes it on the next step as its button's
  press (`Change::UseItem`, `Pack::queued`, KJKS 43: a heal once the one
  under way lands, a shield or stim in place of one up); a row it does not
  know is refused, the item kept.
- **Droid utility items** (`baseitems.2da` rows 75, 76: the belt and the
  arm slots): worn, with charges (one a use, ten each; the Baragwin ones
  unlimited). A worn shield is one of its wearer's four kinds (`combat::
  items::use_of` rows 110-115, their descriptions' points; the shield
  button raises it, `k_sup_bands` deciding); the weapons (stun ray, shield
  disruptor, flamethrower, carbonite projector, gravity generator,
  `k_sup_droid`) are used on a foe from the target block
  (`sim::target::Choice::Item`, below) as `Action::UseItem`. An item's
  spell is a talent (`TALENT_TYPE_FORCE`, its row) while an item with a
  use left casts it, for the AI's `GetCreatureTalentRandom(0x8000)` and
  `ActionUseTalentOnObject`: the mind hands it to the scripts' step
  (`world::scripts::ItemTalents`), which uses the item (`State::
  item_talent`) once a round. Approved test `item_uses` (`droids`).
- **Properties** (`inventory::properties`, `combat::gear`; journal
  2026-10-05-item-properties, read from the executables): the session
  reads every property in force (one without an `UpgradeType`, or with
  its row fitted) as a `Property`, its cost table resolved as the games
  read each (`session::items::read_property`), but those used when the
  item is used (Cast Spell, Thieves' Tools, Trap, Computer Spike: read
  apart) and those nothing worn reads (22, 23, 41, 42, 48, 58).
  `properties::make` folds them into the item's weapon (from its base
  item's, `ItemFacts::bare`), guards, what it gives its wearer
  (`combat::gear::Gear`) and its use limits: an item gives the best bonus
  of a kind (to hit; damage of a type, the weapon's own as its bonus dice
  and flat bonus, others as typed extras; a score, a skill, a save) and
  all its penalties; its defence, always dodge defence, adds up. Weapons:
  Enhancement (to hit and damage), bonuses against a race or a side
  (`gear::Versus`; "All" against everyone), Keen (threat range doubled),
  Massive Criticals, Mighty (a ranged weapon's STR bonus up to it, in
  either hand here, the right only in KOTOR; without it only STR's
  penalty), Monster Damage, on-hit effects (`gear::OnHit`),
  TSL's Door Cutting and Sabering (kept: bashing does not read them
  yet). Worn:
  scores, skills, saves, defence (and against races, sides or damage
  types, three at most), bonus feats, `IMMUNITY_TYPE_*` immunities (Freedom
  of Movement's four; criticals' and sneak attacks' read by the damage
  pipeline with the effects'), Force resistance (the higher of the
  effects' and the items', `powers::resists`), damage reduction,
  regeneration of hit and
  Force points every 6 s (in combat too, `effects::worn`), Light
  (`visualeffects.2da` 5000 lasting on the wearer, its light node lit by
  the view, `scene::CarriedLights`), True Seeing (spots the stealthed),
  Disguise (`GetAppearanceType`; the view draws the wearer, a player or a
  creature, as the disguise's `appearance.2da` row while it is worn, its
  own body again once it is off: `made` reads that body off the frame,
  `cast::Cast::disguise` swaps it in), Special Walk (no running), TSL's Dampen
  Sound (never heard), Sniper and Rapid Shot Bonus, DEX Bonus Max.
- **Sheets with items.** `inventory::rewear` sums every worn item's gear
  (`Gear::with`) into `Sheet::gear` (KJKS 47); `effects::sheet_now` folds
  its scores, skills, saves, dodge defence, feats and DEX cap in as
  effects of the items' (the stored sheet keeps its own: levels read
  ranks without items), then holds what effects and items move within
  the executables' caps (`gear::Caps`: K1's, three times in TSL,
  `flags::TSL`, the session's sheets and TSL's players). A Constitution
  moved moves the hit points (its modifier's change a level), current ones
  held within. Immunities and traits ride on the fighter
  (`Effects::worn_immune`, `worn_traits`) for the checks that read it.
- **On-hit** (`gear::on_hits`, `world::struck_by`): after a weapon's hit
  lands on a creature, each property in order: its chance, its save
  against its DC, its effect unless an immunity keeps it off (sleep, stun,
  paralysis held as the Force's, C11; confusion; fear; Slow; an ability
  drained a point for 30 s; a `poison.2da` row's poison; TSL's knockdown;
  death, which
  takes the hit points with the blow). A slay of a race takes the chance
  a timed kind before it left (QUIRKS.md Q-0158); a slay of a side does
  nothing (an empty handler in both games).
- **Use limits** (`properties::may_wear`): an equip is refused with the
  game's line (alignment 1512, class 1513, race 1514, feat 1475, TSL's
  gender 47993 and attribute 117590) unless, of each kind of limit the
  item has in force, one is met: a side (good-evil to 40 dark, from 60
  light; K1's "All" never, TSL's always, TSL's wholly dark and light), a
  class, a race (TSL's subrace too), a feat, TSL's gender, party member
  (`iprp_pc.2da` tag; the player character) and ability minimum, of the
  item as its upgrades leave it, by the wearer's sheet with what it wears
  and what runs on it.
  Proficiency All waives alignment, class and feat. A fitted upgrade's
  limits are the item's (the Solari crystal makes its saber light side's
  only). Limits never turn properties off; alignment changing while worn
  changes nothing.
- **Upgrades** (K1's, `inventory::properties`): an upgradable item's
  template lists every upgrade's properties with its `upgrade.2da` row;
  the session lists the rows a template has not fitted as
  `ItemFacts::upgrades` (row, `upgradetype` kind, the upgrade item's
  template, its properties and their rows). An upgrade item knows its row
  (`ItemFacts::upgrade`), a colour crystal the lightsabers of its colour
  (`recolours`, `upcrystals.2da`). An item in play carries its fitted rows
  (`Item::upgrades`, KJKS 41, KJKV 29: a bit a row, so a row from 32 on is
  dropped as the session reads it and refused in a pack,
  `inventory::upgrade_bit`); its wearer's sheet is dressed by
  `ItemFacts::fitted` (the item made of its properties and the fitted
  ones'; `rewear` sees a facts change, not only a template's).
  `Action::Fit { item, upgrade, replace }` (wire 26; `may_upgrade`: one
  the party carries, or a member wears that the seat dresses) fits one of
  the party's upgrades of a row the item takes: a kind holds two lightsaber
  crystals, one of anything else (`properties::room`), a full kind only in
  place of the fitted one named (it goes back to the party). A colour
  crystal makes a lightsaber the template of its colour for its hilt, its
  old crystal back to the party, what is fitted kept. `Action::Unfit {
  item, row }` (wire 27) takes one out. Refusals: `Notice::CannotUpgrade`.
  Approved test `item_uses` (`upgrades`).
- **TSL's upgrades**: an item of an `UpgradeLevel` has slots by its base
  item's `itemtype` (`ItemFacts::slot_kinds`, `upgradetypes.2da` rows:
  a lightsaber's two crystals, colour crystal, lens, emitter and cell; a
  ranged weapon's targeting, power pack and firing chamber; a melee
  weapon's grip, edge and cell; armour's overlay and underlay). An item
  in play carries the upgrade item in each (`Item::fitted`, KJKS 47, KJKV
  32; one made starts with its template's `UpgradeSlotN`,
  `ItemFacts::parts`), whose own properties join its host's (so a
  crystal's +1 to hit stands under the saber's +2). `Action::Fit` puts
  one of a slot's kind in the slot `replace` names or the first empty,
  refused above the host's upgrade level (a crystal fits whatever its
  level, `inventory::CRYSTAL`); `Action::Unfit`'s `row` is the slot. The
  bench lists TSL's slots by index (`UpgradeSlot::at`, an empty one's),
  upgrades above the host's level greyed (`UpgradeChoice::fits`).
  Approved test `item_properties`.
- **The bench's visit** (`Action::Assemble { item, changes }`, wire 41):
  the bench edits a copy and sends its changes (`action::Fitting`: each
  `Fit` or `Unfit`) only on Assemble; they are made together, or, one
  refused, none (`State::assemble`); Back sends nothing. TSL's item (one
  with slots) is off its wearer while on TSL's bench: worn before, it is
  back on after only if its wearer may still wear it as it now is (its
  feats, its limits: a Solari crystal on a dark wearer's saber), else it
  stays in the party's pack. The executable's warning before such a fit
  (42489, OK or Cancel) is a confirmation, not asked (the owner's call on
  the bench's boxes).
- **Routines:** `GetItemHasItemProperty` (an `itempropdef.2da` row in
  force, its upgrades' with it, `ItemFacts::rows`).

## Taking and wearing (`world::containers`, `script::items`)

- **Taking** from a container: what it holds is its inventory as the
  scripts have it (`State::loot`: carried, droppable), as KOTOR's
  containers keep their item list: its template's items stocked on the
  module's first visit, what scripts gave it since (K1's civilian, Sith
  and Rakatan footlockers fill themselves on their first heartbeat,
  `k_plc_tres*`; some on their `OnOpen`), what the party put in; an item
  a script takes away is gone from it. While a seat holds it open the
  snapshot's goods carry its items (`State::goods`), the loot window's
  rows. A take names an item (`Action::Take`'s script id) and gives the
  party, on the authority, its whole stack (`State::take_item`), before
  the container's scripts hear of it: what is looted is the party's
  items, to wear, sell or give. Credits (`baseitems.2da` row 57, a stack
  of them the amount) are the party's gold. The first seat to take an
  item in a step gets it; the other is told it is gone.
  The snapshot's per-seat `carried` list is gone (KJKS 26).
- **Destroyed** (`DestroyObject(oDestroy, fDelay, bNoFade,
  fDelayUntilFade)`, as K1's executable takes it: the call sets the
  object's no-fade flag and its delay until the fade, then its destroy
  event comes after `fDelay`; the server removes the object then, and the
  client fades the model it still holds). An item goes at once. Anything
  else leaves the world after `fDelay`: its entity is gone from the sim,
  so nothing meets it again (scripts, minds, strikes, collision, uses,
  containers, locks, perception, the party, the view's targets). A
  creature or placeable is then still drawn, whole for
  `fDelayUntilFade`, then fading out over the engine's fade
  (`remains::FADE_MS`, 1 s [verify]), unless `bNoFade`. The snapshot
  carries it as it went while it departs (`Snapshot::departing`,
  `sim::Departing`: its state then, how long it is drawn yet and how long
  it fades; KJKS 50), so a view joining or a world loaded mid-fade draws
  it too (one still in its delay carries how it will depart,
  `EntityState::going`, so a save made then keeps it); the view draws it
  from there (`cast`, `made`, `play_view`) and
  `going_view` blends it, and hides a placeable the module placed once
  the snapshot no longer draws it. Of two destroys of one object, the
  sooner is kept (in one step too); one already departing is out of the
  world, so a destroy cannot find it, and a dead body fading out (below)
  keeps its fade to its end. Probes: `kjk sim probes destroys`
  (approved test `destroy_fades`).
- **Remains** (`world::remains`; KOTOR's body bags, read from the K1
  and TSL executables). A creature dead outside the party is taken away
  after its appearance's `destroyobjectdelay` (3 s where blank) unless a
  script keeps it (`SetIsDestroyable(FALSE)`, read when it is due, so a
  death script's counts), its body fading out over `fadedelayondeath`
  (the engine's own where blank: 1 s [verify]) while still in the world,
  as KOTOR's client fades a body from the fade delay its death set while
  the server holds it, then gone (`Going::Fades`; the views blend it,
  `going_view`). Taking the body out of the world when the fade begins
  instead (2026-10-09, briefly) moved the K1 chain's timings from the
  Taris Sith base on and stalled Dantooine's ruins and Manaan's sea
  floor. If it held a droppable item (worn or carried, as
  `State::items` has them: an item of its template's lists only when the
  list marks it `Dropable`, a missing mark not, as K1's executable and
  reone load a creature; what scripts gave it counts, as K1's spawn
  treasure `k_inc_drop` and TSL's `PlaceCritterTreasure` give, unless it
  joined an unmarked stack: K1's merge does not compare the mark, and the
  stack keeps its own), gold (a
  creature's own, `State::purses`, `GiveGoldToCreature`) or its bag row is
  a corpse, its remains stand where it lay, facing its way, once the fade
  is over (half a second at the least; KOTOR adds them then and shows them
  once the fade ends, so scripts here meet them a little later): a
  placeable made in play (`remains::bag_template`, its facts the
  `bodybag.2da` row's: no tag, named by the row, 38151 "Remains"; drawn as
  the row's `placeables.2da` appearance, a backpack where it has none)
  holding the droppable items and the gold as one `g_i_credits001` stack;
  what does not drop goes with the body. The row: the template's
  `BodyBag`, else, where that row has no appearance (row 0, every
  template's in both games), the appearance's `body_bag`
  (`module::Body::bag`). What they hold rides with what play made
  (`Spawned::holds`, KJKS 48), so every seat's loot window lists it and a
  save keeps it (KJKV 32: `State::corpses`, those of a module left in its
  `Left`); a placeable play made stands where it was made in a loaded
  world too. They open, are taken from and closed as any container;
  closed holding nothing they go, a corpse's (the rancor's, the krayt
  dragon's) stays. Neither engine reads `Lootable`, `NoPermDeath` or
  `DecayTime`; TSL also strips the dying body's gear from its model, not
  done here (K1 leaves it on). A body with no life left (dead, or downed)
  moves through bodies, meeting only the level and what play made (JA's
  `PmoveSingle` takes `CONTENTS_BODY` out of its mask), so the living
  walking through it do not shove it along and its remains stand where it
  fell (a Sith thug on Korriban, run through by the player, was pushed 7 m
  before it was taken). Approved test `corpse_remains`.
- **Wearing** is a player's action on a party member. `Action::Equip
  { item, slot }` and `Unequip(slot)` act on the seat's own body: its
  avatar, or the companion a guest drives. `EquipOn { who, item, slot }`
  and `UnequipFrom { who, slot }` name the member.
  - A seat dresses only its body. The host also dresses a companion of
    the party table standing here that no seat drives (`items::may_dress`).
  - Any other ask is refused with `NotYours`, which names the seat
    driving the member (I30 P14). So what a driven character wears comes
    off only by its player's hand.
  - A store buys what the party carries, never what anyone wears.
  - An item worn is one the party carried that fits the
    `INVENTORY_SLOT_*` (its base item's `equipableslots`). What was there
    goes back to the party, and the character is dressed anew in the same
    step; its sheet rides in the snapshot. One that does not fit is
    refused (`CannotEquip`).
  - Scripts' item routines are the story's and are not checked.
- **The second weapon set** (TSL's, read from its executable; in KOTOR
  too, where the original has one set: a remaster liberty, with TSL's
  rules and screens). Set 1 is the hands, slots 4 and 5; set 2 is slots
  18 and 19 (`inventory::slots::SETS`; an `Equip_ItemList` struct id is
  `1 << slot`, `0x40000` and `0x80000`, so a creature's template may
  bring one, though no shipped one does).
  - A set-2 hand takes what its set-1 hand takes (`slots::rules`: TSL's
    `CanEquipItem` reads the masks as `0x40010` and `0x80020`), with the
    same feats and use limits; the equip screen lists for it what the
    first hand takes.
  - Set 2 is held ready, not worn (`slots::worn`): TSL's `EquipItem`
    applies no item properties for those slots, so its weapons are not in
    hand and its guards, gear and stealth unit give nothing
    (`inventory::rewear` leaves them out).
  - A swap (`script::items::State::switch_weapons`, TSL's
    `AIActionSwitchWeapons`) exchanges the sets at once: set 2's right and
    left hands to set 1's, set 1's to set 2's, an empty hand as it is, so
    an empty set 2 leaves the hands empty and keeps set 1 in set 2. Each
    piece met its set's rules on the way in and the sets' rules are one,
    so none is refused crossing over. The sheet is dressed anew that step.
  - A player swaps with the swap key (B) or the HUD's swap button
    (`Action::SwitchWeapons`, wire 51: the seat's acting body), or the
    equip screen's (`SwitchWeaponsOn { who }`, wire 52: by the wearing
    rule above). Not while the creature takes part in a conversation
    (nwscript.nss: "does not work in dialogs"; quietly). In a fight it
    takes the round's second (`combat::SWAP_MS`, `COMBAT.md`); out of one
    it is instant. As TSL's client, a second passes between two swaps
    sent (`weapon_set_view`).
  - Scripts swap with `ActionSwitchWeapons` (`SCRIPTS.md`).
  - Both sets are the creature's items in their slots, kept by the save
    (KJKV 46, which also keeps a swap still queued) and seen in the
    snapshot's goods. Probes: `kjk sim probes --only "weapon sets"` (8).

## Stores (`sim::script::stores`)

- **A store** is a module object from the GIT's `StoreList` (its tag the
  UTM's), its terms read from the UTM (`inventory::Terms`): `MarkUp` and
  `MarkDown` (%), `BuySellFlag` (bit 0: it sells, bit 1: it buys; K1's
  pazaak traders only sell). Its stock is its `ItemList`, stocked on the
  first visit as every object's items are; an entry marked `Infinite`
  is an item of its own that never runs out (`Item::infinite`).
- **Opening** (`OpenStore(store, player, bonus markup, bonus markdown)`):
  the games' merchants' conversations run a script that finds the store
  by tag (K1's `k_ptar_larrimsto`, TSL's `a_m_chano`) and opens it for the
  PC speaker. The player then trades with it (`State::trades`, KJKV 13)
  on its terms with the bonuses added, its `OnOpenStore` runs, and its
  screen opens for that player (`Screen::Store`). A trade ends when the
  player leaves the store or the module (`State::end_trade`, as Leave). In
  co-op the store opens for who asked (`Conversation::asker`): the body
  whose use started the conversation (a guest's companion in that
  guest's own conversation, `DIALOGUE.md`), then whoever's pick took its
  last reply (a player who joined it too), on that seat's screen alone,
  though the host's character is the PC speaker. A store reply is
  anyone's (its script only opens the store, `dialogue::story`), so each
  player can open it for themselves.
- **Co-op**: trades are each
  player's, so two players shop at once, in two stores or one, with the
  party's credits. Their buys and sells are applied in seat order, so the
  second to buy the last of something is told `NotForSale` (the store no
  longer holds it) and the second to spend the last credits
  `CannotAfford`. While more than one seat plays a body, a shopper hurt
  (the `Damaged` raise) has its trade ended, and so does a party
  conversation gathering it (`Step::close_stores`; not one standing in a
  player's own conversation, which is not gathered); one player shops on
  as before. A body its seat lets go (dropped, dead, out of the party)
  has its trade ended always. The merchant opens the store again. An
  exit's ready check ends a ready seat's trade (`PARTY.md`), and the host
  cannot bench a companion a guest drives while it trades.
- **Buying and selling** are a player's actions (`Action::Buy`, `Sell`,
  `Leave`, by script object id), carried out by the authority before the
  step's scripts, one of an item at a time. A price is the item's value
  (its UTI's `Cost`: the toolset's total of the base item and `AddCost`;
  K1's stun grenade: base 10 × 2 + 80 = 100) by the markup to buy and the
  markdown to sell, rounded down (`inventory::price`): Larrim's blaster
  pistol (100) sells at 110% for 110 and is bought back at 40% for 40.
  Gold is the party's. A store sells what it holds (plot items too) and
  buys what the party carries, not what it wears nor anything marked
  plot. The last of a stack
  changes hands itself; otherwise one is made where it goes (joining a
  stack there) and the stack loses one, unless it never runs out. What
  could not be done is told to the player (`NoTrade`, `NotForSale`,
  `CannotAfford`).
- **Not KOTOR's:** no reference engine carries stores out (reone's
  `OpenStore` is unimplemented); the price rule is read off the games'
  data, not measured against the originals. Persuade plays no part (the
  scripts give bonuses where the games want them).

## Item creation (`sim::inventory::workshop`, `sim::script::workbench`)

TSL's workbench and lab station, as TSL's executable applies them (read
off its item creation screens, `docs/UI.md`; reone has none):

- **Components and chemicals** are counts, never items
  (`State::pool_carried`, TSL's `CItemRepository::AddItem`): an item of a
  base item whose `baseitems.2da` `itemtype` is 51 (50) a creature
  receives is gone into a count, whatever brought it (made on it,
  `GiveItem`/`ActionGiveItem`/`ActionTakeItem`, bought, taken from a
  container, among a creature's starting items): the party's for a party
  member (`State::components`, `chemicals`), told to every player as the
  game's "Acquired 5 Components." (128117; chemicals 128118,
  `Notice::Count`); else the creature's own (`State::hoards`, KJKV 47),
  never in its remains, gone with it. `CreateItemOnObject` then answers
  `OBJECT_INVALID`. A creature joining the party adds its own to the
  party's (`CSWPartyTable::TransferInventory`) and keeps them, so it adds
  them again at each join (QUIRKS Q-0232). A placeable or store keeps them
  as items. Counts are held to 999,999,999. K1 has no pools
  (`Workshop::pools` empty).
- **Worth and price:** an item's worth is its cost, at least 1 (nothing
  for a plot item), as stores read it (`CSWSItem::GetCost` multiplies by a
  base item factor TSL's loader leaves at 1; `itemmultiplier` is never
  read); its price is a
  twentieth of that, at least 1. Making it costs its price, whatever the
  skill.
- **The tables** (`session::workshop`): `itemcreate.2da` (the workbench's),
  `chemicalcreate.2da` (the lab station's, its rank column `skilllevel`),
  and those TSL's scripts name in their place (`tutitemcreate.2da`, the
  prologue's garage workbench; `itemcreatemira.2da`; `tutchemcreate.2da`,
  not shipped). A row is the template made (`label`), the rank it asks
  (`skill`) in which skill (`base_skill`), its tab (`group`: armour,
  ranged, melee, lightsaber, other), the least level and the alignment
  (`level`, `align`: 1 neutral 41 to 59, 2 light above, 3 dark below; 0
  any).
- **Listed** when the rank asked is at most 8 above the maker's total in
  the skill (ranks, feats, key ability, what it wears and what runs on
  it: TSL's `GetSkillRank(skill, NULL, 0)`), its level and alignment fit;
  **made** when the total reaches it (else the game's line 116292), with
  enough components (116297; at the lab station too few chemicals,
  116298): the price taken, one of the template to the party
  (`Action::Create`), the game's `gui_invadd` played for the maker's
  player.
- **Breaking down** (`Action::Breakdown`) one of an item the party carries:
  not a plot item nor one worth nothing, not one with upgrades fitted
  (124119); at the workbench not one the lab station's table lists (a
  table a script names keeps none of its own); at the lab station only
  what its table lists outside its last group (`workshop::UNMADE`: HK-50
  parts, keys). It yields its price times the maker's Repair over 20 (all
  of it from 20), at least 1; Treat Injury at the lab station
  (`workshop::yield_skill`); `gui_invdrop` played.
- **The scripts:** TSL's `ShowUpgradeScreen(item, character,
  disableCreation, disableUpgrade, table)` with creation not disabled opens
  the workbench (`notice::Screen::Workbench`: the character's skills, else
  the player's; upgrading too unless disabled; the table named, else the
  bench's); `ShowChemicalUpgradeScreen(character)` the lab station. After
  each breakdown or item made the host tells the bench again, with the
  party's stock.
- The executable's confirmations (a price above 50 times the maker's
  level, a yield above that or 250) are not asked: the owner's call
  (2026-10-19).
- **Not yet:** `GetItemComponent`, `GetChemicals`, their piece values and
  `ChangeItemCost` are unbound: no TSL script calls them, and the
  executable binds the first four to copies of its gold routines that pop
  an argument their prototypes lack. The workbench's maker is the
  script's character (the executable's list reads the controlled one: the
  same with every shipped script). Swoop upgrades are cut content (no
  item, no caller).

## Locks and Security (`sim::locks`, `sim::script::locks`)

- **A lock** is a door's or placeable's: locked as placed (`Locked`,
  `Opens::Locked`) or as scripts, keys and Security left it
  (`ObjectState::lock`, KJKS 28);
  its UTD's or UTP's `OpenLockDC`, `CloseLockDC`, `KeyName` (an item's
  tag), `KeyRequired`, `AutoRemoveKey`, `Lockable` (`module::Lock`). A
  locked door, container or exit door used fails to open
  (`OnFailToOpen`), once while use is held (`motion_flags::USE_SPENT`), as
  K1's feedback for a locked object has it: "This object is locked." to
  its player, and every player hears its own locked sound
  (`placeableobjsnds.2da` `locked`) and sees "Locked" (1439) over it (a
  bark; K1 floats the word over the object for 5 s). A container is told
  at once; a door only after its `OnFailToOpen` has run and its changes
  stand (`locks::refused`), and only if it is still locked then, neither
  opening nor with its own opening queued, as K1's exe reads the lock
  again after the script: the Endar Spire's doors that unlock and open
  themselves for Trask say nothing. Unless the party
  carries its key: "You used a key.", unlocked (`OnUnlock`), a door
  opened, a container opened for that player; the key used up when the
  lock says.
- **Security** is a player's action (`Action::Unlock { object, spike }`,
  wire code 11) on a locked object within 2.5 m, on the authority, done
  as a skill's use (`world::traps::skill_uses`, KJKV 34): the companion
  leading acts for the first seat. Refused at once, nothing played
  for a lock only its key opens ("This object cannot be opened through
  conventional means.") and for a character with no ranks of its own
  ("You cannot use this skill."; `skills.2da`: "Characters with no points
  invested cannot attempt to use this skill"); a user mid-swing begins
  nothing (the swing would break it off). The total is the ranks as
  effects leave them, the key ability's modifier (`skills.2da`
  `keyability`: K1 Wisdom, TSL Intelligence), Gear Head (+1, +2, +3) and
  Skill Focus (+2), and a spike's bonus (its UTI's `ThievesTools`
  property: `g_i_secspike01` +5, `02` +10). Out of
  combat it takes 20, in combat it rolls a d20 (the authority's stream),
  against `OpenLockDC`. Begun, the user turns to the lock and plays K1's
  own unlock animation (its open-lock action's: on a door `unlockdr`, on a
  placeable `unlockcntr`; `sim::locks::animations`) for `UNLOCK_MS`
  (1.5 s, the action's timer and `unlockdr`'s length); moving off,
  attacking or falling breaks it off with no check. Then the check, told
  in the game's own line (1406: "Security. success! Take 20 (roll 20 + 3)
  vs. DC 1", "failure", "success not possible"); a spike is kept on a
  success and used up on a failure (owner, 2026-10-09: as K1). A door
  unlocked opens; a container stays shut until used. KOTOR's AI asks `GetIsDoorActionPossible` and
  `DoDoorAction` to unlock: only a lock the creature's own Security
  reaches taking 20 (`ActionUnlockObject` is the scripts' and unchecked).
- **The user says** its sound set's lines, heard by every player at it
  (`SOUND.md`, "Sound-set lines"): begun (slot 22: Carth's "On it.", the
  male player's "No problem."), then failed (23: "Damn!", "Didn't work.")
  or succeeded (24: "It's open.", "Got it."). No sound plays for the lock
  itself: the unlock clips carry no sound events; an opened door sounds
  its `opened`.
- **Its weapons are hidden** while it works the lock: `animations.2da`
  marks `unlockdr` and `unlockcntr` (and the mines' `disablemine`,
  `setmine`, and `inject`) `hideequippeditems`; the views do not draw the
  weapons in hand while one plays (`game::acting::HIDES_ITEMS`), so
  in a fight they are put away for the 1.5 s and drawn again after (out
  of a fight they are not drawn anyway).
- **Not KOTOR's yet:** locking with Security (`CloseLockDC` is read,
  unused); the mines' own sound-set lines (18, 19). No XP was found in K1's open-lock
  action. No reference engine checks Security (reone and
  xoreos unlock outright); the rule is the games' own data and feedback
  lines, not a differential probe.

```bash
kjk script security k1                   # room 3's door (DC 1), end_door05 key only
kjk script security k1 end_m01ab         # end_door05: DC 28, take 20 not enough, a spike
kjk script security k1 danm16            # a key (dan16_door06), a locked footlocker
kjk script security tsl 101per           # TSL: Intelligence, a locked container
kjk play --stage end_m01aa --arrive end_door02 --skill security=2 --headless \
  --cmds 'wait world; wait 1s; ui security; wait 2s; screenshot unlocked; dump; quit'
```

## Traps and Demolitions (`sim::traps`, `world::traps`, `script::traps`)

- **A trap** is a trigger's (a placed mine, `g_t_trap…`), door's or
  placeable's: its `traps.2da` row, detect and disarm DCs, faction,
  one-shot and its key (`module::ObjectData::trap`); armed, disabled and
  seen by the party in `ObjectState::trap` (`snapshot::trap`, KJKS 37);
  who detected it, a laid mine's creator and DCs, and the Demolitions its
  setter had as it set it (TSL's `OwnerDemolitionsSkill`: the total rank
  then, KJKV 49) in the scripts' state (`TrapState`, KJKV 25).
- **Springing:** a creature coming into a trap's area, or opening or using
  a trapped door or placeable, springs it unless it is of the trap's
  faction or not hostile to it, or carries its key. It runs the trap's
  `OnTrapTriggered`, else its row's `trapscript` (the game's
  `k_trp_generic`: frag and plasma damage halved by a Reflex save, stun,
  poison gas a Fortitude save's `poison.2da` row), as the trap; the party
  is told "You triggered a Mine!" (1461). A row's script is in every
  module's library (`session::library` seeds it as it seeds the spells').
- **Detecting:** every creature, each 100 ms, finds the armed traps within
  20 m of their 1.5 m circle by Awareness + 1d10 + 10 (5 less running)
  against the detect DC; a party member's find is the party's: "This mine
  appears to be a: …" (1517).
- **Disarming and recovering** (the use menu: `Action::Disarm`,
  `Recover`, wire 25, 26), offered only to a character with Demolitions
  ranks, within 3 m (the walk there stops 0.6 m outside the mine's
  circle), take 4.5 s standing still, playing `disablemine`; take 20 out
  of combat (a d20 in it) against the disarm DC (recover: 10 more); a DC
  above 35 fails; short by more than 10 (recover: 5) springs it on the
  user. A recovered trap's kit goes to the party. Lines 1529 and 1531
  (1406's form), and K1's float over the user (`locks::floats`):
  `*success*`, `*failure*` in combat, `*Skill Too Low*` out of it
  (Security likewise, `*Impossible*` from DC 60, nothing on a success).
  A failure may be tried again.
- **Laying** (O, `buttons::LAY_MINE`): the deadliest kit carried
  (`traps::best_kit`) set at the feet in 2 s, against the row's `setdc`
  (1530), a failure keeping the kit; the mine the layer's and its faction's, its DCs from the check,
  seen by the party, at most 15 of the party's in an area; kept through
  saves.
- **Mines on doors** (TSL only; its exe's set-mine action and door
  events): the target block's Set Mine on a shut door (`Choice::Mine`,
  `Action::Mine`, wire 38) where the party carries a kit sets one there
  in 2 s, taking 20 even in combat; the kit is the weakest whose blast
  opens the lock, else the strongest (`traps::door_kit`; TSL's menu lets
  the player pick). Set, the mine stands at the door (a laid mine, the
  setter's) and 3 s later blasts the lock: 20 + half the Demolitions the
  setter had as it set the mine (whatever it has since) + the kit's strength (`traps::blast_strength`, the exe's
  table: average 3, deadly and strong 9, devastating 12, minor, stun and
  gas 0) against its open DC, told as line 128535; the mine goes. A
  blasted door, plot or not (`NotBlastable` is never read), is unlocked
  and opens 1 s later. The prologue's engine room door (DC 1) with the
  Proton Missile's minor frag mine (`playthrough` goal `mine TAG`).
- **Stealth XP:** an area's (`StealthXPEnabled`, `…Max`, `…Current`,
  `…Loss`), the routines `AwardStealthXP`, `Get/SetStealthXP…`; lost 6 s
  after a creature newly sees the party, at once when it attacks.
- **Routines:** `GetIsTrapped`, `GetTrap*`, `SetTrapDetectedBy`,
  `GetTrapDetectedBy`, `SetTrapDisabled`, `GetLastDisarmed`,
  `GetNearestTrapToObject`, `VersusTrapEffect`, `GetReflexAdjustedDamage`,
  `EffectPoison`; TSL's `GetOwnerDemolitionsSkill` (a door's mine's, a
  placeable's or a trigger's), `SpawnMine` (a mine of a row at a place, as
  a laid one is made: its DCs the row's modifiers plus those given, the
  creator's faction and Demolitions, or hostile with none; detected by no
  one: Nar Shaddaa's and Dantooine's `a_mine1`, `a_spawn_mines`) and
  `DetonateMine` (taken away; `k_sp1_generic` runs the mine's script on it
  first).

```bash
kjk script traps k1              # tar_m09aa's gas mines: 9 checks with k_trp_generic
kjk play --stage tar_m09aa --arrive tar09_wpalarm --skill awareness=10 \
  --skill demolitions=6 --item g_i_trapkit004=2 --moves '1s yaw 53; 1.6s w yaw 53; 0.5s; 0.3s mine; 4s' \
  --headless --cmds 'wait world; wait 8s; screenshot mines; quit'
```

## The party table (`sim::script::party`)

- KOTOR's companions by `NPC_*` number: each available one's template and
  its creature in this module; who is in the party (KOTOR's three: the
  leader and two followers), the leader, AI styles, solo mode, the
  player's restrict mode, selectability. Kept in the scripts' state (a
  save carries it, KJKV 10). Dialogue's action scripts change it mostly
  (I18). TSL's executable keeps two of the screens' facts here, and so does
  ours (KJKV 50): the map jammed (`DisableMap`, `PT_DISABLEMAP`: the map
  page shows static, the HUD no minimap) and the tutorial windows shown
  (`ShowTutorialWindow`, `PT_TUT_WND_SHOWN`, a bit a row, cleared by a new
  game); the map page's return button (`SetReturnStrref`) KOTOR does not
  save (`k_sup_guiopen` sets it anew), ours does, the snapshot carrying
  it (`docs/SCRIPTS.md`, "Presentation and flow").
- A creature that joins (`AddPartyMember`) follows and fights for the
  first seat in the player's faction (`Change::Join`: its mind's leader);
  one that leaves fights for itself again. `GetPartyMemberByIndex` and
  `GetPartyMemberCount` count the leader first, then the PCs by seat and
  the companions in joining order; `GetIsPC` is the PCs' and the
  companion leading's.
- **The leader.** A companion in the party leads when a script says
  (`SetPartyLeader`) or the first seat's player picks it: KOTOR's portrait
  click or leader key (`Action::Lead`, by the member's body, the player's
  own character the player again; `game::leader_view`), or by number
  (`Action::SetLeader`, `u8::MAX` the player again; a route's `leader`).
  Another seat's pick changes nothing: the party table is the host's. It
  is party member 0 and
  `GetPartyLeader` (TSL). The first seat's commands move the leading
  companion's body, as KOTOR's player controls whom it switches to
  (`world::lent`); the player's own body stands meanwhile (it does not
  follow yet). The Endar Spire's doors ask for it (Trask leading opens
  the bunk room's), and so does Taris's Sith base (T3-M4 walked into its
  door's trigger).
- **The switch.** `SwitchPlayerCharacter(npc)` (K1's Leviathan escape,
  TSL's many) makes companion `npc` the first seat's character in place
  of its own (the table's `switched`, KJKV 21); `NPC_PLAYER` (−1) switches
  back. From that call on, scripts see it as the PC: `GetFirstPC`,
  `GetIsPC`, `GetPCSpeaker` (with no conversation), party member 0; the
  player's own is no PC then (`GetIsPlayerMadeCharacter` still names
  it). From the next step the seat moves it as a guest drives a
  companion (`world::switch`: its `Mind::driver` the first seat, which
  the snapshot carries, so `Snapshot::body` and a client's prediction
  follow it), its mind stood down, in the party and the player's faction
  meanwhile; the player's own stays where it is, benched (its mind's,
  out of the party, still the snapshot's `Kind::Avatar`). Switching
  back, the companion's mind takes over (in the party, or on its own if
  not a member). A companion with no creature here is made from its
  template a step to the PC's left, on its soles; one a guest drives is
  taken from it. False for a companion not available (or another negative
  number). Conversations held with the PC go on with the new one (the
  Leviathan's switches run in conversations' scripts): its seat answers
  them, and it is their `GetPCSpeaker`. A switch whose companion has no
  creature any more is forgotten. While switched no companion leads
  (`world::lent`), and the benched character is out of the party in the
  snapshot too (`Snapshot::in_party`). On a module change the player's
  own comes along as when unswitched [verify].
- **Routines:** `AddAvailableNPCByObject`, `AddAvailableNPCByTemplate`,
  `RemoveAvailableNPC`, `IsAvailableCreature`, `AddPartyMember`,
  `RemovePartyMember`, `IsNPCPartyMember`, `GetPartyMemberCount`,
  `SetPartyLeader`, `SwitchPlayerCharacter`,
  `Get/SetNPCAIStyle`, `Get/SetPartyAIStyle`, `Get/SetSoloMode`,
  `Get/SetPlayerRestrictMode`, `Get/SetNPCSelectability`.
- **A template kept.** `AddAvailableNPCByTemplate` keeps the template as
  the module naming it read it (`party::Npc::template_data`, `sim::pack`'s
  creature encoding, in saves): KOTOR's table keeps the creature, so a
  module that does not ship the template still makes the companion
  (`SpawnAvailableNPC`) and carries it in as itself, its tag, hooks and
  name (`Level::with_companions`, added at every install and load). The
  dark side's Bastila is the Unknown World summit's own `p_bastilla00N`;
  the Star Forge's arrival scene (`stunt_44`) spawns her for its lines.
- **The selection screen.** `ShowPartySelectionGUI(exit, force1, force2)`
  shows the screen and keeps its exit script and forced companions in
  the table (a save carries it, KJKV 15) until a player closes it
  (`Action::ChooseParty`, the screen's Done or Back): the forced first,
  then those picked that are available and selectable, make the party
  (`FOLLOWERS` at most). Each stands on its creature here, else is made
  beside the player and joins as it is made; members not chosen leave
  and go. The exit script runs for the player the next step, when those
  made stand (the Endar Spire's Trask: `k_pend_traskdl29` destroys him,
  `k_pend_reset` talks to him again). The notice that shows it carries
  the table (`notice::Roster`: each available companion's template by
  `NPC_*` number, what each the table has a creature of wears in its
  body slot (its creature's, else the one it was kept from), the
  unselectable, the members, the forced). A player
  opening the party screen in play asks for it (`Action::ShowParty`,
  wire 33): the host tells the same, its choice changing nothing (a
  script's screen still open: that one). A choice that would send away
  a companion a guest drives while it is in a conversation, trades or
  plays pazaak is refused: the
  chooser is told "<name> is busy." and the selection stays open, for
  the party screen to show again.
- **Influence (TSL, `script::influence`).** As TSL's executable keeps it
  (`CSWPartyTable`, `CSWSCreature::HandleAlignmentInfluence`, I35):
  - One per companion of 12 (`NPC_*` 0 to 11), -1 while unset, 0 for
    any other number. `SetInfluence` sets within 0 to 100. `ModifyInfluence`
    moves within 0 to 100, from 50 where unset. Neither checks
    availability.
  - **Started.** A companion `influence.2da` lists (by tag) starts at its
    `BaseInfluence` (50 each) the first time it is made available, made,
    joins or follows (below), with no notice. Kreia alone stays -1 until a
    script first changes hers. So the early companion conversations' bands
    (`c_influence_bet` 40 to 60, `gt 49`) read 50, as the original does.
  - **Told.** A change shows in the status summary as "Influence Gained:"
    or "Influence Lost:" with the companions' names (KOTOR 2's
    `statussummary_p` rows). A set counts by the way the value went, a move
    by its sign. The HUD has no icon for it, as KOTOR 2's has none.
  - **Alignment follows.** A companion's alignment is worked out, never
    shifted:
    - e = its influence + 3 × the player's Charisma modifier;
      f = (clamp(e, 0, 100) − 50) / 50 × m;
      p = (`G_PC_Align_Val` − 50) / 50 × f.
    - It is base + (100 − base) × p for a positive p, else base × (1 + p),
      within 0 to 100. The arithmetic is single floats in the executable's
      order, truncated.
    - m is 1.0. It is 1.2 to 2.0 by prestige level (1, 7, 11, 15, 19) when
      the player has Dark Side Corruption, is a Sith Lord and stands at 40
      or below. The same holds with Light Side Enlightenment for a Jedi
      Master at 60 or above.
    - The base is the alignment it was made with, until a shift moves it
      (`Party::bases`, KJKV 44). Kreia's alignment is her base, whatever
      the player does.
    - It is worked out when its influence changes, when it is made
      available, made or joins the party, and when the player's alignment
      shifts.
  - **`AdjustAlignment` (TSL)** shifts only the one named; the companions
    follow through the formula:
    - **The player's own character:** it moves. Unless the fourth argument
      is TRUE, `G_PC_Light_Total` or `G_PC_Dark_Total` counts the points
      (up to 127) and `G_PC_Align_Val` takes its new alignment, which every
      companion here then follows.
    - **A companion in the party:** its base moves, and it follows from
      there.
    - **Anyone else:** it simply moves.
    - Unless the fourth argument is TRUE, a shift on one in the party
      shows as light or dark side points, even where the scale's end held
      it. `SetGoodEvilValue` does not update `G_PC_Align_Val`, as in the
      original.
    - K1's `AdjustAlignment` moves only the one named and tells only a
      player's character's shift.
- **Sent to base and found again (TSL).** `RemovePartyMember` keeps the
  companion as it stands and the puppet with it leaves too. In TSL its
  effects end, as its executable's `RemoveMember` ends them.
  `RemoveNPCFromPartyToBase` does the same, then fades the companion's
  creature out and forgets it, the table still knowing the companion
  (`SpawnAvailableNPC` brings it back as kept). It answers whether one was
  sent away. `RebuildPartyTable` finds each available companion's creature
  in the module again by its tag (the Remote's too). `SaveNPCByObject`
  keeps the creature given as the companion's.
- **Puppets (TSL).** As its executable's party table keeps them:
  - `PUP_*` 0 to 2, each made available once (by template, or by a
    creature, which joins the puppets' faction, 23).
  - Given to a companion whose creature stands here (`AssignPUP`).
  - Made by `SpawnAvailablePUP`, which moves its creature here instead.
  - Taken into the party by `AddPartyPuppet`, two at most, each once,
    following and fighting as a companion does.
  - `ActionFollowOwner(range)`, queued by a puppet in the party only (no
    one else queues anything), keeps it near its owner until its queue is
    cleared, as the executable's follow action does: while the owner
    stands within the range (across the floor), or it has none, the follow
    holds the queue; farther, a run to where the owner stands, within the
    range, and the follow again go behind what waits, and it completes. A
    dead puppet's, or one no longer in the party, ends. Saved with the
    queue (KJKV 52).
  - A companion joining brings its puppet along, moved or made beside it,
    and leaving sends it away (the Remote comes and goes with Bao-Dur; no
    TSL script adds it itself).
  - `GetPUPOwner` names the companion in the party it was given to.
- **Forfeit conditions (TSL).** `SetForfeitConditions` sets a duel's rules
  (`party::forfeit`, KJKV 45):
  - A party member breaks them by equipping armour, a weapon at all, any
    weapon but Dxun's sword, a ranged weapon or a lightsaber (the first
    rule broken, into the body or either hand), by using an item (any item,
    else any but an energy shield: a base item whose `itemtype` is 20, the
    forearm bands, `ItemFacts::shield`), or, its player ordering it, by a
    Force power. A weapon set swapped into the hands counts as equipped, as
    the executable equips each weapon on a swap.
  - A power counts as ordered when the player presses the power button
    with it chosen, or a held power's own button, and knows it and can pay
    for it now (the executable tests its order so: `GetSpellUsesLeft`),
    whether or not it then goes out (a grip with no one to hold). A power
    the AI or a script casts breaks nothing (`world::powers`,
    `scripts::Ordered`).
  - The act goes on. The rule broken is kept for
    `GetLastForfeitViolation`, and the area's user-defined script runs
    with event 4001 (it ends the duel).
- **Made in play** (`snapshot::Spawned`): `SpawnAvailableNPC` and
  `CreateObject` (creatures, and placeables) make an object from its
  template. A placeable's are the module's own UTPs (a party's backpack)
  and the install's shared ones its scripts name (`CreateObject` takes a
  template its script holds); one stands where it is made, facing its way
  (`Spawned::yaw`, KJKS 29), and collides as a box: its walkmesh's
  (`<model>.pwk`, by `placeables.2da`) least and greatest corners, turned
  to its facing (`ObjectData::geometry` of its template; the level's own
  placeables are in its collision, built before play made any). One made
  where a character stands holds it not (it walks out). A creature's
  template: the library carries every UTC the install lists (K1: 242,
  read with the items in about 20 ms), their hook scripts in the library
  too (the companions' party AI, `k_hen_*`). The object's id is the step's
  next, so the script can use it at once (`GetIsObjectValid`, `GetTag`,
  `AddPartyMember`); it stands in the world from the end of the step,
  stocked with its items, its sheet in the snapshot, its spawn script run
  the next step. It is an instance of the area like a module's placement
  (`sim::instance`): its number follows the placements' and what play made
  before it, its template in the snapshot's `spawned` list (stashed with
  its module, KJKS 24).
- **Following:** an install carries the party's companions (creatures
  following a seat) into the next module as its spawned objects, with
  their ids, fights and sheets, on the free spots beside the players; the
  module left does not keep them. Every member of the party table comes
  along, following or not: one left behind in the module left or in
  another's stash arrives with the party, as KOTOR places the whole party
  at a module's arrival (the dark side's Bastila, left at her Battle
  Meditation on the Star Forge, says the ceremony after it).
- **Views:** they load a module's creatures when its scene loads, and a
  creature made in play off the frame, on the task pool, from the frame
  the presented snapshot first has it (`game::made`): its look, body,
  head, weapons and clips from the template the instance table says it
  was made from (`Instances::made_from`), read
  as a module's creature's are (`viewer::CreatureLoad`), its models and
  view added to the scene, so it is posed, armed and spoken with as a
  module's creature is (`cast`, `combat_fx`, `dialogue_view`) from the
  frame its load is done. A companion carried into the next module is
  drawn a few frames into the new scene; its template is read from the
  scene's module or, when that lacks it (a module's own creatures are in
  its archives only, as `end_trask` is in `end_m01aa`'s), from the latest
  module visited that has it. When the snapshot no longer has one
  (destroyed, or left in a module the party left), its models go with its
  weapons; one still loading is dropped. A module's own creature is drawn
  so too, from the scene's first frame: once the snapshot lacks it (the
  Endar Spire's placed Trask, destroyed as his template joins; a save of a
  module whose placed creatures were destroyed), its body is hidden and
  its weapons go. What loading read stays for the scene: another of the
  same template reads only its textures again.
  A made placeable is drawn so too: its template's model (`placeables.2da`
  by its appearance) where it was made, turned to its facing, gone with
  its object.
  Measured, in the frame it came, before 2026-10-02: the Endar Spire's two
  reinforcements 67 ms for the first (its module's resolver opened), 1.4 ms
  for the second; Trask carried into `end_m01ab`, read from `end_m01aa`,
  20 to 300 ms, half a second from a cold disk. Off the frame since: Trask
  read in 20 to 56 ms and drawn 29 to 80 ms after he came, the
  reinforcements read in 17 to 18 ms; spawning each takes 0.1 to 0.2 ms of
  a frame. Dumps list them (`made`: drawn, loading, loads, the last read
  and spawn times, what each module read).
- **By hand in `kjk play`:** `--run SCRIPT` queues a script of the first
  module for the player at the end of the first step (a story's moment, as
  `--global` sets its state), and `--follow TAG` puts the first module's
  creatures with that tag in the party (the `NPC_*` number `npc.2da`
  gives the tag, else the table's next free one, so the story's scripts
  that remove another number leave it; following the first seat, as
  `AddPartyMember` leaves them). `--party TEMPLATE` (`kjk play`, `kjk
  dialogue run`; two at most) joins a companion who does not stand in the
  module: its template made available as the number `npc.2da` gives its
  tag, then picked on the party selection screen in one step
  (`Action::ChooseParty`), which makes it beside the player
  (`by_hand::party`); `--party p_cand` brings Canderous.

## The journal (`sim::script::journal`)

- **Quests.** The install's `global.jrl` (K1: 101 quests, 643 entries) is
  read at plan time into the library (`Library::with_quests`, in its
  digest): each category by its lowercase tag, its name (a talk-table
  line), priority, planet (`planetary.2da` row) and `plot.2da` row, and
  its entries (number, whether it ends the quest, text, the share of the
  plot's XP reaching it gives).
- **Progress** is the scripts' (`State::journal`, a save carries it,
  KJKV 14): the entry each quest has reached, by tag, the latest updated
  last. One journal for the session, the story's (the host's save).
- **What moves it:** `AddJournalQuestEntry` (a quest does not go back to
  a lower entry unless `bAllowOverrideHigher` allows it), and a
  conversation's node as it plays: an entry as it is shown (barks and
  one-liners too), a reply as it is taken, its `Quest` reaching its
  `QuestEntry` (never back [verify]). A quest the journal does not list
  is kept too: scripts read their own flags back (`GetJournalEntry`).
  Each update is a line in the conversation report, so `kjk dialogue run`
  transcripts show it with the entry's text.
- **Routines:** `AddJournalQuestEntry`, `GetJournalEntry`,
  `RemoveJournalQuestEntry`.
- **Everyone sees it** in the snapshot (`Snapshot::progress`, KJKS 27,
  with the party's XP): the journal page, the HUD's journal icon and the
  feedback's "Journal Updated:" read it (`docs/UI.md`).

## Experience (`sim::script::xp`)

- **The party's pool** (KOTOR's; the owner's choice for co-op): what one
  member gains, every player and every available companion has, kept
  under `inventory::PARTY`. A creature outside the party keeps its own.
  In the scripts' state (`State::xp`, a save carries it, KJKV 14).
- **Tables** (`Library::with_experience`, in its digest): `plot.2da`
  (K1: 75 plots, label and XP by row) and `xptable.2da` (K1: 20 levels by
  challenge ratings `c0` to `c20`).
- **What gives it:**
  - scripts: `GiveXPToCreature` (a loss never below 0), `SetXP`,
    `GivePlotXP` (a percentage of a plot's XP, by label; the share
    worked out wide, so it cannot overflow);
  - the journal: a quest reaching an entry gives the entry's
    `XP_Percentage` of its quest's plot XP each time the quest reaches it
    (a quest taken back or removed and brought there again gives it
    again [verify]);
  - a conversation's node with a plot (`PlotIndex`) gives its
    `PlotXPPercentage` of that plot's XP as it plays [verify: each time];
  - a death: no game script gives kill XP (`k_ai_master`,
    `k_def_death01` never call it), so the engine does: a creature that
    was no party member and the first player's enemy gives the party
    `xptable.2da` at the first player's level and its template's
    `ChallengeRating` (`ObjectData::challenge`, carried by the imported
    templates, `pack::VERSION` 4) [verify: whoever killed it].
- Every gain or loss of the party's is a notice to every player
  (`Notice::Experience`; setting XP is not one).
- **Routines:** `GiveXPToCreature`, `SetXP`, `GetXP`, `GivePlotXP`,
  `GetJournalQuestExperience`.
- Stealth XP is the area's (Traps and Demolitions, above). The party's XP
  rides in the snapshot (`Snapshot::progress`) for the character page and
  the HUD; so do the kills' XP of the last 3 s, each with its dead and
  when (`Progress::kills`, KJKS 51), which the views float over the dead.

## The status summary (`snapshot::Status`)

KOTOR's engine tells the party what the scripts give and take in its
status summary (`CGuiInGame::UpdateStatus`); the views gather it
(`docs/UI.md`). The sim raises a status (`Out::statuses`, kept 3 s in
`Progress::statuses`, KJKS 52, so every player sees it) only where the
original's routines do, and only when the one given or taken from is a
player's character (`GetIsPC`'s):

| status | raised by | not by |
|---|---|---|
| journal entry added | a quest reaching an entry (`AddJournalQuestEntry`, a node's `Quest`) | |
| XP received | `GiveXPToCreature` to a player's character, `GivePlotXP`, a quest's or a node's plot XP (`Out::story_xp`) | a kill, a trap |
| credits received or lost | `GiveGoldToCreature`, `TakeGoldFromCreature` | a container's credits, a trade |
| light or dark side points | `AdjustAlignment` moving a player's character; TSL's: one in the party, unless its fourth argument is TRUE | |
| influence gained or lost (TSL) | `SetInfluence`, `ModifyInfluence` changing a companion's | its start at its base influence |
| item received (its template) | `CreateItemOnObject` (TSL's fourth argument hides it), `ActionGiveItem`, `GiveItem`, `SetItemStackSize` growing a stack | taking from a container, buying, unequipping; TSL's components and chemicals (counts, told by their own line) |
| items lost | `ActionTakeItem`, `DestroyObject` of an item, `SetItemStackSize` cutting a stack | using an item, selling |
| most Force points raised or lowered (TSL) | `AddBonusForcePoints` (its amount), `SetBonusForcePoints` changing the bonus (by the new value: QUIRKS.md Q-0247) | |

Stealth XP is not raised yet.

## Levels (`sim::levels`, `sim::script::levelling`)

- **Classes** (`Library::with_classes`, in its digest; read by
  `session::classes`): each `classes.2da` row's hit die, force die, skill
  points, primary ability; by class level its base attack bonus, saves
  (its `attackbonustable`, `savingthrowtable`) and class defence
  (`acbonus.2da`); feats picked a level (`featgain`'s `_reg` + `_bon`) and
  powers (`classpowergain`); feats granted at a level and feats it may
  pick, recommended first (`feat.2da`'s `<class>_granted`, `_list` 0 to 2,
  `_recom`; the list values read as NWN's [verify]); powers it may pick
  from a class level (`spells.2da`'s `guardian`, `consular`, `sentinel`,
  recommended by `light_recom`); class skills and their recommended order
  (`skills.2da`). `exptable.2da` gives the XP each level asks (K1: level
  2 at 1000, 20 at 190000). What feats need (`prereqfeat1/2`,
  `mincharlevel`, `minattackbonus`, the ability minimums `minstr`,
  `mindex`, `minint`, `minwis`: blank in every row of both games, honoured
  for data that fills them, against the scores the level leaves, its
  ability raised) and the power each power needs (`prerequisites`).
- **A level** of a class it has (`Classes::take`): most HP + hit die + CON
  modifier (at least 1) + Toughness's and Master Toughness's 1 each and
  Wookiee Toughness's 2 (TSL's II and III 3 and 4; `combat::passive`),
  most FP + force die (what the companions' templates hold: Carth 44 =
  4 × 11, Bastila's FP 18 = 3 × 6; the templates' `MaxHitPoints` hold
  Toughness already: Trask 36 = 30 + 3 + 3, Juhani, HK-47, Canderous, by
  `kjk combat feats k1`), a feat learnt there what learning it gives once
  (Toughness and Master Toughness 1 HP for each earlier level, Force
  Sensitive 40 FP: `passive::learn`; `GrantFeat`, TSL's, too), base attack
  bonus, saves and class defence to the class's next row, feats granted
  there (a class level past the class's tables keeps its last row), an
  ability every fourth level, skill points (half of base + the INT
  modifier, the raised one, at least 1, four times that for a first level:
  what the screens count; a
  class skill's rank costs 1, another's 2; at most level + 3, half for
  another class's), feats and powers to pick. Picks that are not the
  level's (more than offered, a feat or power it may not have, points
  overspent) refuse the level, and the player is told
  (`Notice::LevelRefused`). It works on the sheet the character wears, so
  items' bonuses count towards it (the CON its items give adds HP that
  stays) [verify]. Once taken the character's HP are whole at its new
  most and its FP rise by what its most FP rose (`Change::Levelled`: both
  executables' `CSWSCreatureStats::LevelUp` sets the current hit points to
  `GetMaxHitPoints` and adds the gained most to the current Force points;
  a player's level, a companion's automatic one and `AddMultiClass` alike);
  a raised CON adds nothing for the levels before [verify]. Feats and powers past the sheet's sets
  (`Rows`: 256 feats, 320 powers) are left out of the classes.
- **TSL's granted powers** (`levels::granted_powers`, its level-up's
  `AddGrantedPowers`; `Classes::granted_powers`, TSL's classes only): with
  the level taken, a Jedi base class's Form at total Jedi levels (the
  prestige classes' counted) 11, 12, 13 and 14 (Guardian Shii-Cho,
  Makashi, Soresu, Force Focus; Consular Shii-Cho, Force Focus, Makashi,
  Soresu; Sentinel Shii-Cho, Soresu, Force Focus, Makashi), and the
  prestige classes' powers at the levels its last class reaches (Jedi
  Master 167, 168, 169, 170, 171 at 1, 5, 9, 13, 17; Watchman and Assassin
  156, 157, 158 at 1, 7, 13; Marauder 164, 165, 166 at 1, 5, 9; Sith Lord
  144 to 148 at 1, 5, 9, 13, 17). [verify] that the Jedi levels counted
  are those after the level is taken (the executable reads its level-up
  stats).
- **The story's own changes** (TSL's routines, `script::force`): an
  ability's base score (`AdjustCreatureAttributes`, Constitution moving the
  hit points, Wisdom a Force user's FP), a skill's rank
  (`AdjustCreatureSkills`), the base saves (`Modify…SavingThrowBase`, the
  templates' `fortbonus`, `refbonus`, `willbonus`), bonus Force points
  (`BonusForcePoints`), a power granted (`GrantSpell`) and the Form in use
  (`CurrentForm`) are the sheet's (KJKS 62), kept through levels, saves
  and the party table.
- **A new class** (`AddMultiClass`, the player made a Jedi on Dantooine):
  taken up with its first level at once, the game's picks [verify: the
  game shows its level-up screen then]; later levels may take either.
- **The game's picks** (`Classes::auto`, KOTOR's automatic level up): the
  primary ability, skill ranks in the recommended order (each as high as
  it may go before the next) [verify], recommended feats and powers in
  order.
- **When:** the party's XP (its pool) decides, and a level waits until it
  is asked: a player's own, a guest's for the companion it drives (I30
  P15), the host's for a companion no seat drives, one a guest dropped
  included (`Action::LevelUpOn`, by `items::may_dress`). The level-up
  screen's picks as an action (`Action::LevelUp`, or `LevelUpOn`, into
  `State::levelling`), the feats and powers it leaves unpicked and the
  ability point it leaves unspent the game's (`Classes::complete`),
  checked. The harness asks every level at once, the game's picks. The sheet rides in the snapshot
  (`Change::Sheet`); the career (classes and levels taken) is kept in the
  scripts' state (`State::careers`, a save carries it, KJKV 14), and
  `GetLevelByClass`, `GetClassByPosition` read it; `GetHitDice` the sheet.
  The careers ride in the snapshot's progress too (`Progress::careers`),
  for the level-up screen: what its next level may pick
  (`Classes::picks`, the checks `take` makes) in its latest class.
- A player without a character made (`kjk play`'s dev sheet) has no class
  in the sim until it asks for a level: that level takes the class asked
  for as the one its levels so far were in [verify].
- The other passive feats (proficiency gates, two-weapon fighting,
  Dueling, Conditioning, skill feats, damage reduction) are `COMBAT.md`'s
  ("Passive feats").

```bash
kjk script items k1 [module]   # stock, party inventory and gold, equipping, destroying, a save
kjk script party k1 [module]   # available, joining (three at most), leaving, a save
kjk script journal k1 [module] # quests read; forward, not back unless allowed, removed; a save
kjk script xp k1 [module]      # given, set, apart outside the party, plot and quest XP, a kill, a save
kjk script levels k1 [module]  # classes read; k_pend_xpboost; Trask levels himself; the player's level asked
kjk dialogue run k1 end_m01aa  # Trask's talk moves end_attack to 1 (the transcript says so)
# Stores: terms and stock, the game's script opening one, buying and selling, a save
kjk script stores k1                                  # Larrim, Taris (tar_m02aa)
kjk script stores k1 danm13 --opens k_pdan_pazaak     # a store that only sells
kjk script stores tsl 203tel --opens a_m_chano        # every ware never runs out
# Taking and wearing: Trask's locker emptied, the clothes and sword worn, his gate's GetItemInSlot
kjk script equip k1
# Co-op (I30 P14, P15): who dresses whom, a contested take, a driven companion's levels and its drop
kjk sim probes
# Larrim's own conversation opens his store (docs/UI.md: the screen), 1500 credits by hand
kjk play --stage tar_m02aa --arrive wp_02_01 --gold 1500 --headless \
  --moves '300ms yaw 148; 250ms w yaw 148; 600ms use yaw 148 pitch 8; 2500ms; 500ms skip; 1s reply 1; 2500ms; 500ms skip; 1s reply 2; 2500ms; 500ms skip; 2500ms; 500ms skip; 1s reply 4; 2500ms; 500ms skip; 600s' \
  --cmds 'wait world; wait 21s; ui nav down; ui nav down; ui nav activate; ui press BTN_Accept; wait 20f; screenshot bought; quit'
# The Endar Spire's own reinforcements script makes two Sith (CreateObject)
kjk play --stage end_m01aa --run k_pend_reinfrc03 --headless \
  --cmds 'wait world; anim 1.2; camera 21.7 125 2.2 270 -12; wait 1s; screenshot made; quit'
# Trask follows into end_m01ab, a made creature there from its first frame
kjk play --stage end_m01aa --follow end_trask --headless \
  --cmds 'wait world; module end_m01ab end_explode03; screenshot carried; dump; quit'
```

## The area's map (`sim::fog`, `world::charting`)

What the map page and the minimap show of the world (`docs/UI.md`, "The
minimap and the map page"):

- **Notes.** A waypoint's note (`HasMapNote`, `MapNote`, `MapNoteEnabled`;
  `ObjectData::map_note`, its words as the game shows them) shows as
  placed until a script's `SetMapPinEnabled` shows or hides it. The pin
  rides in the waypoint's own state (`ObjectState::state`: `pin::AS_PLACED`,
  `pin::ON`, `pin::OFF`; `ObjectState::map_note` answers the note while it
  shows), so it is stashed with its module and saved as every object's
  state is: no field of its own.
- **Fog of war.** Each area's explored map is the party's, as KOTOR 1's
  saves keep it (the area's `AreaMap` in a save): `MapResX` cells across
  the part of the picture the map uses, as many down as fit square cells
  on the 440 by 256 picture (the Endar Spire's 15 by 8, as a K1 save has
  it; 32 across where the ARE gives none; [verify] KOTOR 2's), a bit a
  cell, row by row from the top left. The ARE's map reaches the sim as
  `fog::Chart` (`ModuleData::map`, in the module's digest only where there
  is one).
- **Exploring.** Each step on the authority every party member (a seat's
  player, a driven or following companion) explores the cells whose
  nearest point is within 10 m of it (`fog::REVEAL_M` [verify: the
  original's radius]). `ExploreAreaForPlayer` explores the whole area for
  the whole party (every seat shares one map); `RevealMap(point, radius)`
  the cells within that many cells of the point's, and the whole with
  none (the default, -1).
- **Kept.** The grids ride in the snapshot's tail by module
  (`Snapshot::explored`, KJKS 45), carried across module changes as the
  party's (the modules left keep theirs), so a save holds them all.
- **Jammed, and the way back.** TSL's `DisableMap` (the party table's)
  jams the map: static on the page, no minimap. Each area's
  `DisableTransit` (its ARE's, `ModuleData::disable_transit`, until a
  script's `SetDisableTransit`, kept by module in the scripts' state) and
  the return button `SetReturnStrref` set ride in the snapshot's tail
  (`Snapshot::screens`, KJKS 63); the page labels its button so, greyed:
  returning to the Ebon Hawk (`k_sup_gohawk` behind a confirm box with the
  query strref, refused with 38451 here, 38452 with the party apart,
  38462 with a foe in sight) is not built yet.

## Bodies scripts change (`combat::Look`)

TSL's `ChangeObjectAppearance` and `DuplicateHeadAppearance` (K1's too)
change a creature's body for good: they ride its fighter (KJKS 63), so a
save keeps them. The views draw it in that appearance's body, with that
head, as they draw a disguise's (`game::made`); a disguise is drawn over
it while it lasts, and refuses the change while it runs (TSL's
executable). The new body stands where it fits (a jump to its own place).
Its size, speed and personal space stay its template's [verify: the
executable updates them from the new row].

## Placeables' looks and the visuals on objects (`snapshot::look`, `sim::visuals`)

Read from KOTOR 1's Mac executable (2026-10-06): its placeable rows of
`animations.2da`, its placeable `PlayAnimation`, its use and inventory
actions and its damage.

- **A placeable's look** rides in its own state (`ObjectState::state`,
  KJKS 46): its `AnimationState` (instance, else template) as placed, 0
  default, 1 open, 2 closed, 3 dead, 4 on, 5 off (above 5: default).
- **Scripts:** `PlayAnimation` and `ActionPlayAnimation` on a placeable
  (`ANIMATION_PLACEABLE_*`) set it: 200 on, 201 off, 202 open, 203
  closed, 204 to 213 its loops (`look::ANIMLOOP` on). `GetIsOpen` on a
  placeable answers whether its look is open.
- **Containers:** the first seat to hold one open opens it; the last to
  let it go closes it (closed, not default: the executable's
  `CloseInventory`).
- **Broken by blows** (`world::objects`): a container opens, any other
  placeable dies.
- **What it shows** is the views' (`game::own_fx`, MODEL-FX.md): each
  look's clip looping, a change's way there once.
- **Visuals on doors and placeables** (`EffectVisualEffect`, `EffectBeam`
  applied to one): kept by the module (`Module::visuals`, the snapshot's
  `visuals`, KJKS 46) from the step they were applied until they end, a
  fleeting one for `visuals::SEEN_MS` (1 s); each its effect's id, the
  object, who applied it (a beam's source), its `fighter::Visual` and when
  it began. A new module begins with none; a save carries them with the
  snapshot.

## The player character (`sim::persona`, `sim::creation`)

- **Who a seat's player is** (`Persona`, in the snapshot's tail, so guests
  and saves have it): an `appearance.2da` row, a gender, a `portraits.2da`
  row and a name. The scripts read it of a player's own character
  (`GetGender`, `GetName`, `GetAppearanceType`), never of a companion it
  drives; the view draws the body from it (below), the screens show its
  portrait and name, and a line's `<FullName>`, `<FirstName>` and gendered
  pairs (`<man/woman>`, `<sir/madam>`: the first for a man) are its
  (`dialogue_view::Fill`, the conversation's player; a bark's the first
  seat's).
- **Character creation** (`Creation`, `SimWorld::create`, at a new game's
  start for the first seat): a player portrait gives the appearance by the
  class's build (`portraits.2da`: a soldier `appearance_l`, a scout
  `appearancenumber`, a scoundrel `appearance_s`, as the game's class
  selection and portrait picker take it); the scores are bought with 30
  points from 8 (a raise costs the score's modifier, 1 to 3, up to 18,
  checked); its first level is a level of its class taken from nothing
  (`Classes::take`: the hit die and CON modifier, the class's first rows,
  the feats it grants, the feats picked, the game's for those left
  unpicked, the skill ranks bought); it wears and holds nothing (the
  Endar Spire's footlocker has its clothes and sword), is human, neutral
  (50) and of the player's faction. A level without classes (the course)
  takes the persona only. Seats without a character made get a default
  each (male and female in turn, each sex's portraits in order, a
  soldier's build).
- **Its body** (`game::wardrobe`): the appearance's body as
  `appearance.2da` dresses a creature, by the body armour its character
  wears (its letter and texture variation; none: its underclothes),
  dressed again when that or the persona changes, the next module's scene
  composed with it. Out of a fight its body plays its own clips
  (`game::gait`, `docs/SKELETON.md`).

```bash
# A female scout made by quick creation (her class's scores and skills)
kjk play --stage end_m01aa --made scout:po_pfha2:Mira --headless \
  --cmds 'wait world; wait 8s; camera 18.6 20.3 0.0 183.75 -4 50; wait 10f; screenshot made; quit'
# K1's own gender conditional run as the made character
kjk play --stage end_m01aa --made scout:po_pfha2:Mira --run k_con_isfemale --headless --cmds 'wait world; quit'
```

## The galaxy map (`sim::script::state::Galaxy`)

- **Planets** by `planetary.2da` row: those scripts make available (shown)
  and selectable (travelled to) (`Set/GetPlanetAvailable`,
  `Set/GetPlanetSelectable`), kept in the scripts' state (KJKV 17).
- **Travel.** `ShowGalaxyMap(planet)` tells every player
  (`Screen::GalaxyMap`, with the planets available and selectable then);
  the host picks a planet available and selectable on the galaxy map
  screen (`docs/UI.md`; `Action::Travel`, a guest's is not taken: I30 P6):
  `GetSelectedPlanet` answers it and the travel script runs for the player
  (K1's `k_sup_galaxymap`: the take-off, the next module). `kjk
  playthrough`'s `travel N` picks from the map shown, as its Accept would
  (a planet not on offer blocks).
- **The game's end** (`EndGame`, `StartCreditSequence`): with its death
  screen (`TRUE`, also the default K1's argument-less calls take; nine of
  K1's calls, the turret's hull and the krayt dragon among them) the game
  is lost: the party's loss (`Snapshot::lost`, the fall's box, I30 P13).
  Without it (`FALSE`: K1's ending; Manaan's life sentence; Taris destroyed
  after a fifth swoop race too slow, after its movie) every player goes to
  the main menu (`Screen::End`), once credits started by
  `StartCreditSequence` (`Screen::Credits`) have rolled on its screen.
  `IsCreditSequenceInProgress` is false: the credits roll on the players'
  screens, so the area heartbeat that waits for them (K1's `k_endgame`)
  ends the game under them.
- **Acquiring.** Each item the party gains runs the module's
  `Mod_OnAcquirItem` (`GetModuleItemAcquired` answers it): Tatooine's
  chieftain's map sets the global its Eastern Dune Sea asks. The party's
  items as last seen are in the scripts' state (`State::watch`, a save
  carries them, KJKV 22); a module change sees them afresh.
