# TSL chain: what the walkthrough routes skip

This lists everything the TSL chain routes leave out, so each item can get
its own test route later. The main chain plays one light-side path; every
side quest, skill-gated branch, dark-side or alternative choice and
workaround below still has to be shown to work.

Each milestone's PR adds its own items here. Tick an item when a route in
`data/routes/tsl/` plays it (name the route) or when it is judged not to
need one (say why).

The **kind** tags are:
- `side` is an optional quest or encounter;
- `skill` is a branch gated by a skill or attribute the chain's Exile lacks;
- `choice` is a reply or path not taken (dark side, the other companion, paid instead of persuaded);
- `feature` is engine support that is missing;
- `blocker` is a route workaround for an engine bug (those need the engine fix, then the route undone).

## Peragus (PR 11)

- [ ] `side` **peragus_t3:** the broken droid and the footlocker on T3's way through the fuel depot hallway, not found by tag.
- [ ] `choice` **Persuade:** the ways that need it, skipped throughout (the route header says "the way without Persuade").
- [ ] `choice` **Dark side and other replies:** HK-50's talk, the Harbinger's crew, Atton's interrogation.
- [x] `skill` **The dormitories' shift console:** [Repair]ed with a part, the east dormitory's ventilation shut with a spike [Computer], the lockdown ended, as DSimpson does (peragus_dorms); no longer destroyed.
- [x] `skill` **The Harbinger's navicomputer:** the Exile's [Repair] with a part (peragus_harbinger), the spike kept for the engine deck's maintenance control.
- [x] `choice` **Kreia's talk about her hand** aboard after Peragus took the pity reply by default (-8 influence); it now asks what if she had died, DSimpson's way that loses none (peragus_escape).
- [ ] `feature` **The turret:** the party's AI keeps running during the 107per turret. That's not reproduced as a bug, but a test should pin it down.

## Telos (PR 14)

- [ ] `blocker` **telos_base_shuttle:** the route leaves the bay control room by `TelosDoor2@7`/`@6`. The nav mesh blocks a face only when most of it lies under a placeable, so it plans between two barrels in the south doorway.
- [ ] `side` **Ramana**'s quest.
- [ ] `side` **The pazaak and swoop records** on Citadel Station.
- [ ] `side` **Suspicious Goods.**
- [ ] `side` **The Czerka escort.**
- [ ] `side` **The Handmaiden duels and sparring** at the academy.
- [ ] `side` **Kreia's and Atton's influence talks.**
- [ ] `skill` **The Czerka console's [Computer] option** never appeared.
- [ ] `skill` **Corrun Falt** is persuaded down to 500 credits ("If Lorso's dirty", Persuade over 4), but the first check ("Of course not", over 10) fails: the chain's level 5 Exile has Persuade 10, so he is still paid. kjk's check is TSL's: `c_sc_per_gt` asks `GetSkillRank` over its parameter, and the exe's `CSWSCreatureStats::GetSkillRank` totals the ranks, the ability, items, effects and the Empathy line's best tier (+1, +2, +3). Persuade is the first of exile.build's order at its most every level, and its reasons buy Charisma 14 and raise it from the twelfth level only, so the build keeps it: faithful to this Exile.
- [ ] `skill` **Atris's Persuade** fails at the chain's ranks (still the Light Path).
- [ ] `choice` **The bay doors:** only "Come with me" with Bao-Dur and Kreia was played; the other party choices weren't.
- [ ] `choice` **The Exchange's and Czerka's dark side or mercenary paths** on Citadel Station, and the Ithorians betrayed.
- [ ] `feature` **`leader player`** after Bao-Dur's pad scene: `tr_baodurpad` makes him leader and nothing in the route's path switches back. Check how the original returns control.

## Dantooine (PR 15)

- [ ] `blocker` **Khoonda's militia desk corner (-6.9, -15.3):** every walk back from Zherron's post sticks there, because the nav mesh's rounding is cut short and the body box catches the desk's sloped nonwalk foot. Zherron's three optional visits are skipped because of it.
- [ ] `side` **Zherron's kinrath job** and its 2000-credit reward (blocked by the desk corner above).
- [ ] `side` **The cave Matriarch**, who only spawns with Zherron's job.
- [ ] `choice` **The will given to Zherron**: the chain gives it to Daraala.
- [x] `skill` **The battered droid's diagnostic and repair** (Computer, Repair), and the mechanic persuaded the Exile is no Jedi (dantooine_khoonda).
- [ ] `skill` **The sensor surveillance branch** (Computer, Awareness).
- [x] `skill` **Ralon's holocron:** played in dantooine_enclave by DSimpson's light way (the fake seen, 500 credits given, LSP). His [Awareness] reply (R50) checks the local E37's own script sets (`a_ralo_dialogue`, Awareness over 4), which needed the engine to choose a line's replies after its scripts, at the line's end, as TSL's `CSWSDialog::SendDialogEntry` does.
- [ ] `skill` **The protocol droids, conduits and storage overload** in the Enclave sublevel.
- [ ] `skill` **Jorran's Persuade:** "I saved your life" asks Persuade over 11, the chain's Exile has 11 there; he is paid. As Corrun Falt's: the check and the total are the exe's, and exile.build's Persuade is at its most.
- [ ] `feature` **The landing pad's "Plasteel Cylinder{Med, Blast}":** DSimpson mines it open, and kjk can set mines on doors only.
- [ ] `side` **The pazaak Ithorian.**
- [ ] `side` **Nallek.**
- [ ] `side` **Dopak's message.**
- [ ] `side` **Khoonda's defence tasks** (turrets, mines, droids, militia) before the battle.
- [ ] `side` **Akkere's hydrospanner.**
- [ ] `side` **The lightsaber build:** the lens fixture comes only from a forged will.
- [ ] `feature` **The party swap before the battle** (Handmaiden in, Kreia out): the harness has no goal to open the party screen away from the Hawk.
- [ ] `choice` **Berun's conversation** ends on the harness's default replies.
- [ ] `choice` **The mercenaries' side**, Azkul joined, the dark side Vrook rescue.

## Nar Shaddaa (PR 16)

- [ ] `side` **Vogga's room and the dance.** It needs the Handmaiden (or the Disciple) as the dancer, and with it go Vogga's help against Goto, the Telos fuel and his share of the Exchange's notice.
- [ ] `side` **The swoop droid** and the swoop races.
- [ ] `side` **The Exchange deal.**
- [ ] `side` **IT-31.**
- [ ] `side` **Pazaak.**
- [ ] `side` **Geriel.**
- [ ] `side` **Nadaa.**
- [ ] `side` **Saquesh.**
- [ ] `side` **The airspeeder.**
- [ ] `side` **Lasavvou** (helping him gives no notice).
- [ ] `skill` **The Serroco persuaded**: the option wasn't offered, so they are fought.
- [ ] `choice` **Kaalah-Nah:** the chain pays him 2000 credits for his good word (+3 notice). Test the other ways of reaching the 8 notice Visquis's call needs.
- [ ] `choice` **The HK-50s** talked down, not fought.
- [ ] `choice` **Hanharr instead of Mira** (the dark-side companion).
- [ ] `feature` **Breath Control** is never actually cast; `--god` covers the poison gas in the Jekk'Jekk Tarr.
- [x] `feature` **Visas's ambush on the Ebon Hawk:** it plays at the start of nar_shaddaa_landing now (GetNPCSelectability's -1 for a companion not in the party table).
- [x] `blocker` **nar_shaddaa_zez:** Zez-Kai Ell teaches the Sentinel's next form (`a_give_form`, Ataru after Vrook's Shien) and his scene opens the safe house's door, so the route walks out through it (nar_shaddaa_zez, no `goto`).
- [x] `feature` **`G_PC_LEVEL`** follows the XP the party gains (TSL's `ReceiveExperience`), so `a_give_quest_ls` gives Visas's part as the game's treasure for the Exile's level (a short lightsaber, nar_shaddaa_landing).
- [x] `feature` **Visas's joining conversation ends at the Handmaiden's line**, as in TSL: that line's `a_next_scene` (parameter 1) spawns Visas, resets the Ebon Hawk and destroys its owner VisasCut at once. TSL's exe keeps a conversation on its owner (`CSWSObject::StartDialog`, the main loop's `UpdateDialog` on the owner), and `DestroyObject`'s event deletes a destroyable creature that is not the player's or the party's whatever it is doing (`CSWSCreature::EventHandler`), its conversation with it. So the lightsaber-part notice (the part is given at "My lightsaber... you have destroyed it", `a_give_quest_ls`) and R176's second `a_next_scene` are never reached in the game either. Probe `owner_destroyed_cut_short` (`talk ends`).
- [ ] `side` **Goto's yacht:** the plasteel containers and the sentry droids' scavenging.
- [ ] `side` **Goto's yacht:** T3-M4's combat upgrade, the Reset program and the lab station.
- [ ] `choice` **The yacht hub scene's G0-T0 line** (Mira's plays instead).
- [ ] `side` **The companion talks aboard after Nar Shaddaa,** and HK-47's rebuilding.
- [ ] `side` **Fassa's passage** for Kahranna's family and Odis.
- [x] `feature` **T3-M4's Security checks** pass DC 28 doors with a total of 15: right by TSL's exe. `CSWSObject::AIActionUnlockObject` takes 20 out of combat (1 + rand % 20 in it), adds `GetSkillRank`'s total (ranks, ability, feats, effects) and the spike's bonus, and opens on that sum against `GetOpenLockDC`, as kjk does (`world::locks::unlock`): 20 + 15 = 35 against 28. The feedback's "total" is the skill's, before the 20. Nothing to change (Korriban's PR).

## Onderon and Dxun, the first visit (PR 19)

- [x] `side` **The Ebon Hawk's turret** at the blockade (421DXN, six fighters, 600 XP): the Exile mans it (dxun_landing).
- [x] `side` **The droid carrier's computer port and the remote droids** (Repair, Computer): the port repaired, its core [Computer]ed, the three droids reactivated, the first's weaponry, targeting and shields, the second's weaponry (dxun_landing).
- [ ] `side` **The remote droids' other systems, patrol mode and the reactor overloads**: the parts left go to Zuka's wires.
- [x] `side` **The skeletal corpses and wreckage** on the landing's paths, the scouts' side path and behind the zakkeg (dxun_landing, dxun_tests).
- [x] `side` **The Mandalorian cache** (404DXN): the droid foreman examined and bashed, its droids fought, its boxes searched, two unlocked [Security] (dxun_tests).
- [ ] `side` **The cache's two keyed boxes** (`g_tresmilhig006`): plot, a key required and no key named, so neither Security nor bashing opens them. Judged not openable in the game either [verify in the exe].
- [ ] `side` **The sealed cache by the dead Mandalorian** (`DxunCache_imp`): it needs Thorium charges, from Akkere's job on Dantooine (skipped there) or Korriban.
- [x] `side` **The battle circle**: Davrel, Kex with swords, Tagren, Kelborn and Bralor beaten under the forfeit rules, champion (dxun_circle).
- [x] `choice` **Davrel's challenge outside camp** refused twice, the light way (dxun_circle). Fighting him, and the zakkeg hunted with him (only before the zakkeg is slain), are not played.
- [ ] `side` **Zuka's navigational telemetry**: his relay started and the wires [Repair]ed (dxun_circle), but the telemetry computer's core asks 4 computer spikes at the Exile's Computer and the party carries 3, and no store on Dxun or Onderon sells them. So the three phase-pulse cannoks, Zuka's reward and his word to Mandalore are left.
- [ ] `skill` **Kumus's reward** ([Persuade] or [Force Persuade] for 1000 credits): not asked, as DSimpson's light way asks none.
- [ ] `choice` **Kumus's dark ways**: left to rot, blown up with the detonator, or killed once down (DSP, HK-47's and Hanharr's influence).
- [ ] `choice` **The sentries antagonised** into a fight (DSP).
- [ ] `side` **Kex's store.**
- [ ] `side` **Mandalore's tests' other companions' lines**: the Handmaiden's and the Disciple's at Kumus (the party is Mira and Bao-Dur), T3-M4's at Zuka's.
- [x] `side` **The beast rider's boma** at the spaceport: slain, his credits thanked for as not needed (LSP) (onderon_spaceport).
- [x] `side` **An open starport visa** (Bakkel's) given to Terlyn's family by the starport gate (LSP) (onderon_kavar).
- [ ] `side` **The Rodian bounty hunters' visa**: not looted, so not sliced by Kiph (500 credits, Bahima's word first) nor given to Gormo or Xaart (LSP).
- [ ] `choice` **The other visa buyers**: Tolas (credits, Persuade for more), Sakarie (DSP, a Qixoni crystal).
- [ ] `side` **The swoop races** at the Iziz cantina (510OND, four heats; Persuade for the fee and a bigger cut).
- [ ] `side` **Nikko's pazaak** (three wins for his cards).
- [ ] `side` **Ponlar's riot** on a second pass by the market ("I won't fight civilians" keeps out of it).
- [ ] `choice` **The port's checkpoint guard** [Persuade] or [Force Persuade] (Kreia's or HK-47's influence; neither is in the party).
- [ ] `choice` **Vaklu's side**: the journalist taken, Ponlar's crowd joined, Anda's captains (Gelesi, Bostuco, Riiken), Tobin's deal in the cantina.
- [ ] `side` **The other companions' Iziz lines**: Visas at the politics and in the cantina fight, the Handmaiden's and HK-47's in the cantina fight, G0-T0 rewiring 1B-8D.
- [ ] `side` **Margar's, Gegorran's and 1B-8D's stores, the news holograms.**
- [x] `side` **Mandalore's back room** on the return: its lock [Security]ed, its boxes and bag searched (dxun_return).
- [x] `side` **The camp's boxes and bags** (dxun_circle).
- [ ] `feature` **The harness's chase takes up a gun** for a foe it cannot reach and keeps it: the routes put the Exile's lightsaber back in hand (`equip g_w_lghtsbr03`).

## Korriban (PR to come)

- [x] `side` **Kreia's histories** at the four tombs and the excavation, the corpses' warning and the skeletal corpses searched (their Hssiss fought); Ajunta Pall's [Awareness] reply, "No one is beneath redemption" (LSP, +INF) (korriban_valley).
- [x] `side` **The landing talk's questions** to Kreia, and the party's banter at the cave and the Academy (korriban_valley).
- [x] `side` **The Academy in full** (korriban_academy): the west hall's datapads, every footlocker [Security]ed, the library opened from the student terminal, its terminal restored by [Computer], the Thorium datapad, the military locker [Security]ed for the Thorium Charges, Uthar's room blown open with them (his holocron broken by the blast, as its bark says), the difficult door and the south passage [Security]ed, every assassin ambush fought.
- [x] `side` **Sion asked everything** (how he knows Kreia, what she wants with the Exile, what he wants with her) before the fight (korriban_sion).
- [x] `side` **The secret tomb in full** (korriban_secret_tomb): the side rooms of the first and second visions, the dead Jedi's datapads, Ludo Kressh's armband and sword, the crypt rooms.
- [ ] `skill` **The student ID by [Computer]** (termquiz's "[Computer] Bypass login", termtrn's "[Computer]" ID): the recruits' datapad's ID is taken instead (DSimpson's way, the same XP).
- [ ] `skill` **The minefield disarmed**: the Exile has no Demolitions (skills.2da: not used untrained), so the vision's mines are sprung by walking over them; "[Demolitions] Let me go first this time" is not offered.
- [ ] `choice` **The dark ways**: Vash's body scorned (DSP), Sion's "don't let me stand in your way", the visions' dark replies (Malak followed, the troops ordered to charge, Kreia attacked), Kreia's counsel spurned (-INF).
- [ ] `choice` **The third vision's other answers**: stand back and watch, or side with the companions; the Exile protects Kreia (LSP).
- [ ] `feature` **The party screen away from the Hawk**: out of the tomb the Exile walks back aboard alone and takes Atton and Bao-Dur up at the ramp for the Academy (korriban_secret_tomb, korriban_academy); DSimpson opens the party screen in the cave. The harness has no goal to open it away from the Hawk (see Dantooine).
- [x] `feature` **TSL's kill XP**: xptable.2da heads its ratings `0`, `1`, … where K1's are `c0`, `c1`, …, so no TSL death gave XP and the chain's Exile came to Korriban at level 9, short of the secret tomb's gate (G_PC_LEVEL over 9). `CSWSCreature::CalculateDeathExperience` reads the column after `level` by the rating; probe `xp_kills_tsl_heading`. The Exile now lands on Korriban at level 14, so the routes take DSimpson's order (the cave and the tomb before the Academy).
- [x] `side` **The earlier routes with the Exile's new levels**: the random lightsaber `a_give_quest_ls` makes for the Exile's level is now `g_w_lghtsbr03` (the Dxun and Onderon routes equip it); the Enclave's stuck door and the Jekk'Jekk Tarr's sparring-room door are struck with a vibroblade taken up (`equip w_melee_05`), the hands holding none; Dxun's hall fight may now chase down the lower camp's assassins too, so its walk there is played only while still in camp (dxun_departure).
- [x] `feature` **The 107PER turret is played**: the playthrough's turret bot shoots the area's troopers when the course has no fighters (TSL's 107PER, whose bolts meet the area), as a player does; it had waited for all 25 to board, and one stuck by a crate never did.
- [ ] `feature` **TSL's kill XP, the rest of `CalculateDeathExperience`**: its row is the level the PC's XP reaches (kjk: the sheet's level), a creature with an autobalance row rates as its level plus `autobalance.2da`'s `crmod`, and the sum is scaled by `npc.2da`'s `GAME_XP_General` (100). Not done [verify in the exe].
- [x] `feature` **A placeable used from its action point**: TSL's `CSWSCreature::GetIsInUseRange` looks from the user to the placeable's nearest action point (`CSWSPlaceable::GetNearestActionPoint`, its walkmesh's use positions), 1.5 m up; kjk looked at its middle, so Lonna Vash's body could not be searched through the force cage round her. Probe `use_from_action_point` [verify in K1's exe].
- [x] `feature` **Local numbers are bytes, as both executables keep them** (the owner's ruling on PR 19): K1's var table has 8 numbers, TSL's 32 (`CSWVarTable::Get/SetLocalNumber`), each an unsigned byte. `ExecuteCommandSetLocalNumber` keeps only -128 to 127 (stored as its byte, so -1 reads back 255) and ignores anything else; a slot past the last keeps nothing and reads 0. kjk kept the whole int in any slot. The save is unchanged: a wider number from an older save reads as its byte. Probes `local_numbers_k1`, `local_numbers_tsl`. Not changed: the booleans' slots (K1 96, TSL 160 in the exes), which kjk does not bound.
- [x] `feature` **A room mesh that blends additively is not met** (TXI `blending additive`): the secret tomb's cloud of light over the minefield (`korfx_clouds`) had stood the Exile off every mine. Probe `additive_texture_not_met`.

## Later milestones

The drafts in `drafts/` list their skips in their notes files, and each
milestone's PR moves them here.

## Whole-game items

- [x] `feature` **The chain plays a created Jedi Exile** (`exile.build`, chain.flags): TSL's creation of a light-side Sentinel, levelled by the build's plan; the masters teach their forms (Vrook, Zez-Kai Ell).
- [ ] `feature` **Prestige classes** are not reached: Kreia offers one (`c_pc_prestige`, `a_pc_jedi`'s `AddMultiClass`) from level 15; with kill XP the chain's Exile reaches 15 on Korriban, and the offer is not taken yet. The engine side is there (the class taken up unlevelled with the next level's XP, its powers from its own `spells.2da` column); a route for it waits for the later planets.
- [x] `choice` **Kreia's influence at Azkul's offer:** "I cannot allow your plans to succeed" costs Kreia's influence (E41, `a_influence_dec`). dantooine_vrook now takes his job by "[Lie] Okay, I'll help you." (R57: no DSP, his men not hostile, Kreia's "Well maneuvered!" with `a_influence_inc`), DSimpson's way to decide later, and dantooine_battle defends Khoonda as before.
- [x] `feature` **Unspent skill points** are kept for the next level, which adds its own to them (TSL's `CSWCLevelUpStats::AddSkillPointsPerLevel`; K1 saves them as TSL does, the creature's `SkillPoints`): `State::unspent`, `Classes::take`. exile.build's `bank` keeps what its skill order cannot spend, so Repair's rank waits for the point a level before left, and Demolitions, last in its order, takes the rest (Korriban's `c_sc_dem_gt`, over 5), not Stealth. Probe `unspent skill points kept` (`player_creation`).
- [ ] `choice` **The dark side playthrough,** start to end.
- [ ] `choice` **A female Exile:** the Disciple joins, not the Handmaiden.
- [ ] `choice` **Influence-gated content:** each companion's high and low influence talks, and the Jedi trainings (Atton, Bao-Dur, Mira, Handmaiden/Disciple, Visas).
- [ ] `side` **HK-47's parts** and his assembly on the Ebon Hawk.
- [ ] `side` **Workbench crafting, lightsaber building and the crystal's growth.**
