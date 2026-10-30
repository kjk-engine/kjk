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

- [x] `side` **peragus_t3:** the broken droid and the footlocker on T3's way through the fuel depot hallway: judged none to find. 103PER's GIT holds no broken droid or footlocker on the upper hallway T3 takes from the blast door to the fuel control station; DSimpson's "broken droid" there is the remains of the droids fought on the way, and the nearest container (`MilLowPlstcCylin@1`) lies on the sublevel below, past no door T3 opens.
- [ ] `choice` **Persuade:** the ways that need it, skipped throughout (the route header says "the way without Persuade").
- [ ] `choice` **Dark side and other replies:** HK-50's talk, the Harbinger's crew, Atton's interrogation.
- [x] `skill` **The dormitories' shift console:** [Repair]ed with a part, the east dormitory's ventilation shut with a spike [Computer], the lockdown ended, as DSimpson does (peragus_dorms); no longer destroyed.
- [x] `skill` **The Harbinger's navicomputer:** the Exile's [Repair] with a part (peragus_harbinger), the spike kept for the engine deck's maintenance control.
- [x] `choice` **Kreia's talk about her hand** aboard after Peragus took the pity reply by default (-8 influence); it now asks what if she had died, DSimpson's way that loses none (peragus_escape).
- [ ] `feature` **The turret:** the party's AI keeps running during the 107per turret. That's not reproduced as a bug, but a test should pin it down.

## Telos (PR 14)

- [ ] `blocker` **telos_base_shuttle:** the route leaves the bay control room by `TelosDoor2@7`/`@6`, before the tank droid's fight too (the chase after it made for the barrels' pocket once a body went round across). The nav mesh blocks a face only when most of it lies under a placeable, so it plans between two barrels in the south doorway.
- [x] `side` **Ramana**'s quest: Harra heard, Doton Het beaten at pazaak for her (his end script takes the 1000 and asks again on a loss), Ramana set free (LSP) (telos_cantina_games, a branch from `telos_luxa` with a `gold` stand-in: the main line keeps its credits for Falt). Paying Doton 2000 is the other way; the "[Persuade] 1500" reply has only a [Failure] entry in the data.
- [x] `side` **Mebla's pazaak record**: three wins for her Double card (telos_cantina_games).
- [ ] `side` **The swoop record** on Citadel Station: a race is run (telos_cantina_games), but the bot's clean run (some 49 s, now that it shifts as a gear tops out, looks ahead by time and jumps the walls) takes the third best time, not Ov Darum's record of 41.27 s (k_207tel_enter's 4127 hundredths). DSimpson notes the PC game's record all but unreachable ("at best you can get second place").
- [x] `side` **Suspicious Goods**: the bay thugs' blaster shown to Grenn, Samhan's smuggling plan heard out, Grenn's goods carried to him, the sting in the docks' bay, Samhan's surrender and Grenn's thanks (LSP) (telos_ithorians).
- [x] `side` **The Czerka escort**: the salvager found behind the base's security doors, led back to its entrance (his thanks a one-line talk, LSP) (telos_military_base).
- [x] `side` **The Handmaiden sisters' duels** at the academy: all five bouts (one sister, the higher forms, free of restriction, a pair, five at once; 750 XP), the gear the mat took worn again (telos_academy).
- [ ] `side` **The Handmaiden's own sparring** aboard: it waits for the Exile's level 10, 14 and 18.
- [x] `side` **Kreia's influence talks** aboard after Telos: those who follow us (DSimpson's light answers), her past ([Intelligence]) and the vision of it (1000 XP), the Handmaiden asked about (dantooine_khoonda).
- [x] `side` **Atton's pazaak** aboard, Republic Senate rules, for practice (dantooine_khoonda).
- [ ] `side` **Atton's past**: "I'd like to know about your past" is not offered yet; it waits on more of his influence (`c_influence_bet` 40 to 60 fails it).
- [x] `skill` **The Czerka console's [Computer] option**: judged none to take. No Czerka console on Telos has a [Computer] reply (`mnfrm_c` in 209TEL copies, erases or logs out; 233TEL's landing pad console has no conversation); the station's [Computer] lines are the TSF terminal's, Falt's desk's, Chodo's, the medical and vivarium terminals' and Slusk's, which the routes play where they need them.
- [ ] `skill` **Corrun Falt** is persuaded down to 500 credits ("If Lorso's dirty", Persuade over 4), but the first check ("Of course not", over 10) fails: the chain's level 5 Exile has Persuade 10, so he is still paid.
- [ ] `skill` **Atris's Persuade** fails at the chain's ranks (still the Light Path).
- [ ] `choice` **The bay doors:** only "Come with me" with Bao-Dur and Kreia was played; the other party choices weren't.
- [ ] `choice` **The Exchange's and Czerka's dark side or mercenary paths** on Citadel Station, and the Ithorians betrayed.
- [x] `feature` **`leader player`** after Bao-Dur's pad scene: the original gives the Exile back on entering the old military base (232TEL's `k_232_enter`: `SetPartyLeader(npc_player)` when the one entering is `GetFirstPC`, the leader). Two engine rules make it so, and the route's switch is gone (telos_czerka_site): the party leaves an area through the leading companion's walk into an exit, and the area's OnEnter runs for each companion the party brings in, as the executables raise the entered event for every creature added to an area (probes `leader_walks_out`, `area_entered_by_the_party`).

## Dantooine (PR 15)

- [x] `blocker` **Khoonda's militia desk corner (-6.9, -15.3):** the office joins the hall by a diagonal channel 1.05 m across between the desk's sloped nonwalk foot and the wall, which JA's square box (0.76 m, but 1.08 m measured across a 45° channel) could not pass; KOTOR tests a creature's move by its personal space across its way whichever way it goes (TSL's `CSWSCreature::WalkUpdateLocationTestDistance` hands `CSWSArea::NoNonWalkPolysDetailed` the appearance's `perspace`, and `CSWCollisionMesh::NoNonWalkPolys` sweeps the path widened by it). A body now moves against the level's solid as its box round across (`collision::Shape::Upright`); the walls standing in for KOTOR's point on the walkmesh (outline and footprint walls) it meets as the box (collision probes `upright face`, `upright diagonal`, `upright outline`; sim probes `body_channel_shapes`, `body_runs_a_diagonal_channel`).
- [x] `side` **Zherron's kinrath job** and its 2000-credit reward (dantooine_khoonda, dantooine_cave).
- [x] `side` **The cave Matriarch**, who only spawns with Zherron's job: fought, her remains searched (dantooine_cave).
- [ ] `choice` **The will given to Zherron**: the chain gives it to Daraala.
- [x] `skill` **The battered droid's diagnostic and repair** (Computer, Repair), and the mechanic persuaded the Exile is no Jedi (dantooine_khoonda).
- [x] `skill` **The sensor surveillance branch**: Bao-Dur, asked by the Exile's [Awareness], finds the surveillance gear (+INF); Saedhe caught out on it, the sensors left him for 3000 credits (LSP) (dantooine_cave).
- [x] `skill` **Ralon's holocron**: inspected, called the fake it is ([Awareness]), his family's need heard and 500 credits given (LSP) (dantooine_enclave). The [Awareness] reply asks a local the line's own script sets as the fake is shown: both executables run an entry's scripts before they offer its replies (`SendDialogEntry`: `RunScript` twice, then `SendDialogReplies`, where each reply's condition is checked), which kjk now does too (coop talk probe `line_script_before_replies`); before, the reply was never offered and the route's default bought the fake for 1000.
- [x] `skill` **The storage room's overload** in the Enclave sublevel: its console overloaded (a spike), the conductors blown, the room's containers searched (dantooine_enclave). The spike is bought back from Akkere on the landing pad (dantooine_khoonda), as Dxun's routes count on two.
- [ ] `skill` **The protocol droids and the conduits' main terminal**: the six droids ask some seven repair parts each, the terminal four more and two spikes; the party carries a dozen parts, which Dxun's droid carrier counts on.
- [ ] `skill` **Jorran's Persuade:** "I saved your life" asks Persuade over 11, the chain's Exile has 11 there; he is paid.
- [ ] `feature` **The landing pad's "Plasteel Cylinder{Med, Blast}":** DSimpson mines it open, and kjk can set mines on doors only.
- [ ] `side` **The pazaak Ithorian.**
- [x] `side` **Nallek**: talked down ([Persuade], "I'm a warrior, like yourself") (dantooine_enclave).
- [x] `side` **Dopak's message**: Berun's doubts laid before Zherron ([Persuade]), his message ("Go ahead") given Dopak, and Zherron told (dantooine_khoonda, dantooine_enclave, dantooine_vrook).
- [x] `side` **Khoonda's defence tasks** before the battle (dantooine_battle): Zherron's list heard; Suulru, Akkere, Dillan and Jorran recruited; the three assault droids reactivated (Akkere's four parts bought); the four wounded militia treated by hand ([Treat Injury], LSP each); the side security door sealed ([Repair]).
- [ ] `skill` **The medic droid** (four parts) is left for the hand treatment: Dxun's droid carrier counts on the parts the party carries (eleven left after Dantooine).
- [ ] `skill` **The turret and droid bay consoles**: slicing and reprogramming ask about a dozen computer spikes; Akkere's stock is two (bought for the storage room and Dxun).
- [ ] `feature` **Khoonda's mines**: no harness goal recovers or lays a mine at a place.
- [x] `side` **Akkere's hydrospanner**: found in the third assault droid, his story heard and believed, 500 credits given (LSP), and he joins the militia (dantooine_battle).
- [ ] `side` **Akkere's thorium charges** (1000 credits): the light way recruits him by the hydrospanner instead.
- [ ] `side` **The lightsaber build:** the lens fixture comes only from a forged will.
- [ ] `feature` **The party swap before the battle** (Handmaiden in, Kreia out): judged not playable away from the Hawk (the finale's PR): TSL's executable opens the party selection only for a script's `ShowPartySelectionGUI` (`CGuiInGame::ShowPartySelection`, its one caller `ExecuteCommandShowPartySelection`, is the only code that adds the panel), so a player swaps at the Hawk or where a script offers the screen, as the routes do.
- [x] `choice` **Berun's conversation**: DSimpson's light way, his doubts of Zherron asked after and a look into him taken on (dantooine_khoonda). His message to Dopak through Zherron is delivered (above).
- [ ] `choice` **The mercenaries' side**, Azkul joined, the dark side Vrook rescue.

## Nar Shaddaa (PR 16)

- [x] `side` **Vogga's room and the dance**: the Handmaiden brought ashore, his domo's dancer, Vogga asleep, then woken: fuel for Telos once Goto is dealt with (nar_shaddaa_vogga, a branch from `nar_shaddaa_refugees`).
- [ ] `side` **Vogga's hoard and the kath hounds' juma juice**: the flophouse's thieves are overheard only stealthed (`k_chkvoggathv` asks IsStealthed), which the chain's Exile cannot be; with them go the hoard's lens fixture and Vogga's fuel word after Goto.
- [ ] `side` **The swoop droid** and the swoop races.
- [x] `side` **The Exchange deal**: the boss's back room by the landing pad walked in on, Squik killed by him, his men fought (the Exchange +1) (nar_shaddaa_exchange).
- [x] `side` **IT-31**: Tienn and TT-32 heard, IT-31 bought from Kodin for 150 ([Persuade]) and sent home ([Repair]), TT-32's droid shield for T3 (nar_shaddaa_landing).
- [ ] `side` **Pazaak.**
- [x] `side` **Geriel**: his illness laid before Saquesh ([Persuade]: his hold on the camp relaxed, the Exchange +1), then treated ([Treat Injury], LSP) (nar_shaddaa_landing).
- [x] `side` **Nadaa**: Adana's debt settled with Saquesh ([Persuade] for 600 credits, or 700; LSP, the Exchange +1), Nadaa's thanks (nar_shaddaa_landing).
- [x] `side` **Saquesh**: talked to over Geriel and Adana, not fought (nar_shaddaa_landing).
- [ ] `side` **The airspeeder.**
- [ ] `side` **Lasavvou** (helping him gives no notice).
- [x] `skill` **The Serroco persuaded**: Hussef heard first (the Serroco and the Exchange), the door guard and their leader talked round ([Persuade]; the refugees left alone, the Exchange +1), not fought (nar_shaddaa_landing). Hussef's thanks follow; his reward was not seen (start E25, "We're indebted to you").
- [x] `choice` **Kaalah-Nah:** the main line no longer pays her: the Refugee Sector's work (Saquesh over Geriel, Adana) and the Exchange deal bring the notice to the 8 Visquis's call needs (nar_shaddaa_landing, nar_shaddaa_exchange). Paying her 2000 for +3 is the other way.
- [ ] `choice` **The HK-50s** talked down, not fought.
- [ ] `choice` **Hanharr instead of Mira** (the dark-side companion).
- [ ] `feature` **Breath Control** is never actually cast; `--god` covers the poison gas in the Jekk'Jekk Tarr.
- [x] `feature` **Visas's ambush on the Ebon Hawk:** it plays at the start of nar_shaddaa_landing now (GetNPCSelectability's -1 for a companion not in the party table).
- [x] `blocker` **nar_shaddaa_zez:** Zez-Kai Ell teaches the Sentinel's next form (`a_give_form`, Ataru after Vrook's Shien) and his scene opens the safe house's door, so the route walks out through it (nar_shaddaa_zez, no `goto`).
- [x] `feature` **`G_PC_LEVEL`** follows the XP the party gains (TSL's `ReceiveExperience`), so `a_give_quest_ls` gives Visas's part as the game's treasure for the Exile's level (a short lightsaber, nar_shaddaa_landing).
- [ ] `feature` **Visas's joining conversation is cut short** after the Handmaiden's line, because the game's own `a_next_scene` destroys the owner (VisasCut), so the lightsaber-part notice and the second `a_next_scene` never play. Check in the exe whether a conversation talks on after its owner is destroyed.
- [ ] `side` **Goto's yacht:** the far rooms' plasteel containers and the sentry droids' scavenging (the container by the utility droid is searched, nar_shaddaa_goto).
- [x] `side` **Goto's yacht: T3-M4's combat upgrade** at the Security Console (nar_shaddaa_goto).
- [ ] `side` **Goto's yacht: the Reset program and the lab station** (the lab station's screen, `ShowChemicalUpgradeScreen`, has no harness goal).
- [ ] `choice` **The yacht hub scene's G0-T0 line** (Mira's plays instead).
- [x] `side` **HK-47's rebuilding**: his remains diagnosed aboard after Telos and the vocabulator fitted (dantooine_khoonda), the control cluster bought from Akkere and fitted (nar_shaddaa_landing), the processor bought from Kodin once the rebuild is under way (`a_stock_kodin` stocks the part the quest lacks; nar_shaddaa_landing), the chassis taken from the HK-50s' remains (nar_shaddaa_warehouse), the last two fitted aboard after Nar Shaddaa (nar_shaddaa_zez).
- [ ] `side` **The companion talks aboard after Nar Shaddaa** (Mira's, HK-47's first).
- [x] `side` **Fassa's passage** for Kahranna's family and Odis: both met in the Refugee Sector (nar_shaddaa_landing), Fassa asked after the yacht (the Exchange +1) (nar_shaddaa_zez).
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
- [ ] `feature` **The harness's chase takes up a gun** for a foe it cannot reach and keeps it: the routes put the Exile's lightsaber back in hand (`equip saber`).

- [ ] `blocker` **dxun_departure: Mandalore's walk at (-23.0, 111.9).** When his conversation ends on the beat of his heartbeat, `k_mandalore_ud` (403DXN_Camp_Visited 2, set by the conversation's end script) queues his patrol (wp_mandalore_move3, move2, a bark) ahead of the `ActionStartConversation` `a_mand_cs1` delays half a second, and his KOTOR walk to wp_mandalore_move3 jitters there for good, so mand_cs1 never starts; the route waits 2 s before talking to him. The walk's stall is the thing to fix (it stalls on the finale's binary too).

## Korriban (PR 20)

- [x] `side` **Kreia's histories** at the four tombs and the excavation, the corpses' warning and the skeletal corpses searched (their Hssiss fought); Ajunta Pall's [Awareness] reply, "No one is beneath redemption" (LSP, +INF) (korriban_valley).
- [x] `side` **The landing talk's questions** to Kreia, and the party's banter at the cave and the Academy (korriban_valley).
- [x] `side` **The Academy in full** (korriban_academy): the west hall's datapads, every footlocker [Security]ed, the library opened from the student terminal, its terminal restored by [Computer], the Thorium datapad, the military locker [Security]ed for the Thorium Charges, Uthar's room blown open with them (his holocron broken by the blast, as its bark says), the difficult door and the south passage [Security]ed, every assassin ambush fought.
- [x] `side` **Sion asked everything** (how he knows Kreia, what she wants with the Exile, what he wants with her) before the fight (korriban_sion).
- [x] `side` **The secret tomb in full** (korriban_secret_tomb): the side rooms of the first and second visions, the dead Jedi's datapads, Ludo Kressh's armband and sword, the crypt rooms.
- [ ] `skill` **The student ID by [Computer]** (termquiz's "[Computer] Bypass login", termtrn's "[Computer]" ID): the recruits' datapad's ID is taken instead (DSimpson's way, the same XP).
- [ ] `skill` **The minefield disarmed**: the Exile has no Demolitions (skills.2da: not used untrained), so the vision's mines are sprung by walking over them; "[Demolitions] Let me go first this time" is not offered.
- [ ] `choice` **The dark ways**: Vash's body scorned (DSP), Sion's "don't let me stand in your way", the visions' dark replies (Malak followed, the troops ordered to charge, Kreia attacked), Kreia's counsel spurned (-INF).
- [ ] `choice` **The third vision's other answers**: stand back and watch, or side with the companions; the Exile protects Kreia (LSP).
- [x] `feature` **The party screen away from the Hawk**: out of the tomb the Exile walks back aboard alone and takes Atton and Bao-Dur up at the ramp for the Academy (korriban_secret_tomb, korriban_academy). Judged the game's own way (the finale's PR): only a script's `ShowPartySelectionGUI` opens the screen in TSL's executable (see Dantooine), so the walk aboard stays.
- [x] `feature` **TSL's kill XP**: xptable.2da heads its ratings `0`, `1`, … where K1's are `c0`, `c1`, …, so no TSL death gave XP and the chain's Exile came to Korriban at level 9, short of the secret tomb's gate (G_PC_LEVEL over 9). `CSWSCreature::CalculateDeathExperience` reads the column after `level` by the rating; probe `xp_kills_tsl_heading`. The Exile now lands on Korriban at level 14, so the routes take DSimpson's order (the cave and the tomb before the Academy).
- [x] `side` **The earlier routes with the Exile's new levels**: the random lightsaber `a_give_quest_ls` makes for the Exile's level differs with the Exile's level when it is made (the finale's PR's TSL death XP raised it), so the Dxun, Onderon and Korriban routes `equip saber`, the strongest lightsaber the party has; the Enclave's stuck door and the Jekk'Jekk Tarr's sparring-room door are struck with a vibroblade taken up (`equip w_melee_05`), the hands holding none; Dxun's hall fight may now chase down the lower camp's assassins too, so its walk there is played only while still in camp (dxun_departure).
- [x] `feature` **The 107PER turret is played**: the playthrough's turret bot shoots the area's troopers when the course has no fighters (TSL's 107PER, whose bolts meet the area), as a player does; it had waited for all 25 to board, and one stuck by a crate never did.
- [x] `feature` **TSL's kill XP, the rest of `CalculateDeathExperience`** (the finale's PR): the row is the level the PC's XP reaches (the executable's search of the experience table), at most 30; the rating at most 30; an autobalanced creature (its `MultiplierSet`, now kept: a pack format change) rates as `PCLevelAtSpawn` (1 if none) plus its row's `crmod`, at least 0; the sum times `npc.2da`'s `GAME_XP_General` percent, and `PER_NPC_Bonus` (0 in TSL's data) a share by member, rounded up (`ApplyDeathExperience`). kjk reads `G_PC_LEVEL` as the creature dies, not as it spawned [verify: a level taken in between]. Probe `xp_death_tsl`.
- [x] `feature` **A placeable used from its action point**: TSL's `CSWSCreature::GetIsInUseRange` looks from the user to the placeable's nearest action point (`CSWSPlaceable::GetNearestActionPoint`, its walkmesh's use positions), 1.5 m up; kjk looked at its middle, so Lonna Vash's body could not be searched through the force cage round her. Probe `use_from_action_point` [verify in K1's exe].
- [x] `feature` **Local numbers are bytes, as both executables keep them** (the owner's ruling on PR 19): K1's var table has 8 numbers, TSL's 32 (`CSWVarTable::Get/SetLocalNumber`), each an unsigned byte. `ExecuteCommandSetLocalNumber` keeps only -128 to 127 (stored as its byte, so -1 reads back 255) and ignores anything else; a slot past the last keeps nothing and reads 0. kjk kept the whole int in any slot. The save is unchanged: a wider number from an older save reads as its byte. Probes `local_numbers_k1`, `local_numbers_tsl`. The booleans' slots are bounded too now (the finale's PR): K1's var table keeps 96 (an unsigned index to 0x5f), TSL's 160 (`CSWVarTable::Get/SetLocalBoolean`, to 0x9f); one past them is not kept and reads false. Probes `local_booleans_k1`, `local_booleans_tsl`. A true boolean reads as its bit lies in its word (`1 << (index % 32)`), not 1, in both games (`CSWVarTable::GetLocalBoolean` masks it and `ExecuteCommandGetLocalValue` pushes it as it is; the Ravager's PR), so a script's `== TRUE` on any other bit than a word's first is false, as in the games.
- [x] `feature` **A room mesh that blends additively is not met** (TXI `blending additive`): the secret tomb's cloud of light over the minefield (`korfx_clouds`) had stood the Exile off every mine. Probe `additive_texture_not_met`.

## The finale: Onderon's war, Dantooine's Enclave and Telos (PR 23)

- [x] `side` **Kreia's prestige classes** aboard, the Jedi Watchman taken: now the first time the Exile is aboard past level 15, leaving Nar Shaddaa (nar_shaddaa_zez).
- [x] `side` **Atris's first scene** at her academy, owed as the Hawk leaves for Dxun (finale_dxun).
- [x] `side` **Freedon Nadd's tomb**: Xarga's questions, the beast trainers, the east Ancient Terminal's lever, the ritual broken without temptation (3 LSP), the sarcophagus and the mummy (finale_sith_tomb).
- [x] `skill` **The Turret Control Tower's door** [Security]ed (DC 28), the lower shield taken down from its console (finale_sky_ramp).
- [x] `feature` **The Sky Ramp's damaged upper force shield** (Hardness 125: `CSWSDoor::PostProcess` gives it a permanent damage resistance of 125 against every damage type but base damage, and a saber's damage is typed by its base item's flags, energy, so no plain blow gets through): on the Queen's side its console opens only the lower one (`a_kill_barrier`), Bao-Dur (whose blow opens it, `a_ffield_baodur`) is unselectable after the strike on the tomb, and the walkthroughs bash it with the saber. A test's mighty blow now counts its roll ten times over before the guards, so the chain's saber breaks it (finale_sky_ramp).
- [x] `side` **The palace in full** (finale_palace): Kiph spared and his console used twice (the doors, Riiken's cage, then the override once Riiken has spoken: `c_other_set`), the gas room's metal box, Kadron's armory locker, the storage room's number puzzle (66, 45, 39) and its boxes, the locked metal box [Security]ed, Kadron given the override, Battle Meditation in the throne room, Vaklu spared by the Exile's [Persuade] to Talia, Kavar asked about the masters and Atris, his form, Talia's relics, Kreia healing Tobin.
- [x] `side` **The Council at the Enclave** asked through to Kreia's judgment (finale_enclave), Visas Marr's talk aboard.
- [x] `side` **Atris** asked everything at her end (where the Sith strike from, Kreia, the death of the Force: [Persuade]), spared, and told the Exile is sorry; the Handmaiden's name (finale_atris).
- [x] `skill` **The wounded Onderon soldier** on Citadel Station: [Treat Injury], then taken along (finale_citadel).
- [x] `side` **The turret minigame at the Sky Ramp's tower** (505OND, term_acc's "Control the turret.", 800 XP) (finale_sky_ramp). Its fighters' tracks key a dummy node above the hook (`path_dummy02`), not the hook: a track's path is now the chain posed down to the hook (`session::minigame::carried`, probe `track_hook_carried`); five of eight had stood at their rest places 1 km off.
- [ ] `side` **The Dxun jungle's side content** played as Visas Marr: the perimeter sensor (stealth, Bao-Dur's codes, a detonation pack), the Turret and Droid Power Generators [Repair], the Sith camp's Security Terminal ("Foothold": [Computer], 10 spikes), its footlocker and metal box, the minefield's recovery.
- [ ] `side` **Freedon Nadd's tomb's side rooms**: SW (grenadiers, the broken medical droid, puzz_sw: 13) and SE (war droids, the workbench, the broken droid, puzz_se: C), the west wing, the Dark Side Energy taints (a Will save, a DSP on failing).
- [ ] `side` **The palace museum's relics** looted (Talia gives them instead), the gas room's and the boobytrapped consoles, the secondary terminal's cameras; Riiken's last talk ("already talking" as Kavar's started).
- [x] `skill` **The [Awareness] replies**: Kreia's "I have to lead the group heading to the royal palace" at Kelborn's split (finale_dxun), Kiph's two (finale_palace), Kreia's "You are afraid" at the Enclave (finale_enclave). The Council's other replies stay the harness's defaults (no light or dark points there).
- [ ] `side` **Chodo Habat and Moza** on Citadel Station, the TSF soldiers of 221TEL, Ent 081's merchant room, Carth's cameo (`222_carth`).
- [x] `feature` **A switched-in companion's level-up**: the harness levels the body the first seat is switched to (`Party::switched_body`) in its own class with the game's picks, as TSL's level-up screen levels the character the player controls (Visas Marr in 410DXN and 411DXN).
- [x] `feature` **`<FirstName>`** in a reply (Xarga's "What happened with <FirstName>?" to Visas Marr): the player's tokens are the first seat's player's, whoever it drives (TSL's `CSWTlkTable::GetTokenValue` reads the party table's name), on screen and in the transcript.
- [x] `feature` **A companion sent to base is gone at once** (`RemoveNPCFromPartyToBase`: `CSWPartyTable::KillNPCObject` deletes its creature), so 403DXN's `k_403_enter` finds no Kreia by tag and makes her anew; kjk had faded her out as the step ended, after the script had moved her, and Kelborn's talk lost her lines. Probe `puppets` (token 32).
- [x] `feature` **DestroyObject passes over a creature `SetIsDestroyable(FALSE)` keeps** (`CSWSCreature::EventHandler`'s `EVENT_DESTROY_OBJECT` asks it be destroyable): 506OND's `a_end_game` destroys Tobin's body, then raises him for Kreia's scene. Probe `kept_stays` [verify in K1's exe].
- [x] `feature` **TSL's ChangeToStandardFaction leaves the player's own character as it is** (`ExecuteCommandChangeFaction` tests the stats' PC flag): Dxun's battle circle sets the PC to Friendly 1 after each match, which in kjk turned every Friendly 2 creature (Dantooine's salvagers at the Enclave) on the Exile. Probes `pc_faction_k1`, `pc_faction_tsl` [verify in K1's exe].
- [x] `feature` **Fights that end in scenes** (Vaklu downed, Atris at her minimum, `SetMinOneHP`) and **a switched-in companion followed** (Visas Marr leading the strike, `SwitchPlayerCharacter`) play as the scripts have them (finale_dxun, finale_sith_tomb, finale_palace, finale_atris).
- [x] `blocker` **The harness's `unlock`** is met when the lock was placed locked and is unlocked already in play (a blow broke it open in the fight round it: Korriban's academy footlockers).

## The Ravager and Malachor V, to the end credits (the Ravager's PR)

Played: the routes ravager_command_deck, ravager_bridge, ravager_escape,
malachor_surface, malachor_depths, malachor_remote, malachor_academy and
malachor_core, on the light side to the end credits.

- [x] `side` **Colonel Tobin** asked what became of him and spared by [Persuade] (DC 17) for Onderon's sake, to set off the cores (Visas's good-act reaction) (ravager_command_deck).
- [x] `side` **Visas's meditation** in her cell (851visdr, 851medit) (ravager_command_deck).
- [x] `skill` **Darth Nihilus's [Intelligence] line** ("Kreia has lied to you"): he feeds on the Exile and falls (ravager_bridge); his mask asked for and given (ravager_bridge).
- [x] `side` **Carth on Citadel Station** (205TEL's carthend): Revan, the Endar Spire, "Safe journey, exile", Bastila (ravager_escape).
- [x] `side` **Mira against Hanharr**, played as Mira, Hanharr spared and asked whether he serves Kreia (malachor_depths).
- [x] `side` **Bao-Dur's remote** priming the four engine cores, G0-T0 holding it (malachor_remote).
- [x] `skill` **Darth Sion** asked about Kreia, the Academy and Revan, his will eroded by [Persuade] (his first word, his third fall) and [Awareness] (his second), his letting go (malachor_academy).
- [x] `side` **Kreia in the Trayus Core** asked everything (Atris, why, how she used the Exile, why him, what happens now), the offer to save her, the mass shadow generator held over her; her last words: the Exile to follow Revan, the friends' futures (Mira, Mandalore and his people, Visas, the Handmaiden, Atton, Bao-Dur and the droids), Revan's, the Republic's and the planets' (malachor_core).
- [x] `feature` **The end credits**: TSL starts no credit sequence; its ending movies MalMov12 (light) and MalMov14 (dark) end in Credits.bik's roll, and a_end_game's `EndGame(FALSE)` goes to the main menu (`CClientExoAppInternal`'s end pending: the module unloaded, the main menu shown). `Screen::rolls_credits` counts those two movies as the credits; the harness's `credits` goal is met by them and the end (probe `end_after_ending_movie`).
- [ ] `blocker` **901MAL's rock pillar** at (148, -63): its drawn slope (material 0, no walkmesh face) overhangs the walkable floor north of it, and a walk hugging the nonwalk faces round its foot wedges a body against them (off the floor): Bao-Dur's remote stuck there, and the Exile at (84, -24) by another. The routes step wide of it; the nav class (a route's corner held off drawn solids as it is off walls) is left.
- [ ] `blocker` **A scripted move through a door its scene opens a moment later**: a_903kreia walks the Exile from the Trayus Academy's hallway to wp_903kreia_1 through d_entrance and opens the door 4 s later; kjk's move stalls at the shut door and ends there (`scripts.moves.stuck_stalls`), outside, behind the way back out. malachor_academy stands the Exile where the scene puts him (`goto`). How KOTOR's walker waits on a door that is about to open is to be read in the exe.
- [ ] `side` **The Ravager's lockers** (Locker_Nihlus), the DC 55 security rooms (door_nih003 and kin), the workbench in the missile bay.
- [ ] `side` **Malachor's corpses** (HighCorpse, MidCorpse, LowCorpse), the metal boxes, the Proving Grounds' workbench and its south loop, the Trayus Crescent (905MAL), the storm beasts of the surface's far east loop.
- [ ] `choice` **Visas's sacrifice** at Nihilus (852nih R63, "You are his weakness", c_influence_gt) and Nihilus's [Wisdom] line; Tobin killed instead (the psychotic reaction).
- [ ] `choice` **The dark ending** (MalMov10, MalMov14), Kreia's offer taken, the Exile's other replies at her death.
- [ ] `feature` **851visas** (Visas and Mandalore aboard the Ravager) is the conversation of the module's p_visas001 template, which no script spawns: the party's Visas keeps her own (visasmarr). Not reachable in the game as shipped.
- [ ] `feature` **sensorfi's HK-47 branch** (HK-47 and the HK-50s turning on G0-T0) is not in 902MAL's copy, whose only start is G0-T0 holding the remote; HK-47 is not in this chain's party anyway.
- [ ] `feature` **Cut content** (TSLRCM restores it, not used): Atton against the Disciple and the Handmaiden against Visas in 903MAL, Atton against Sion, Atton's dying scene at the Core (904atton), the companions kneeling round Kreia.

## Whole-game items

- [x] `feature` **The chain plays a created Jedi Exile** (`exile.build`, chain.flags): TSL's creation of a light-side Sentinel, levelled by the build's plan; the masters teach their forms (Vrook, Zez-Kai Ell).
- [x] `feature` **Prestige classes**: Kreia's offer aboard (start 8, `c_pc_prestige`: level 15, alignment past 75) is taken the first time the Exile is aboard past level 15, leaving Nar Shaddaa (nar_shaddaa_zez), the Jedi Watchman (`a_pc_jedi`: `AddMultiClass` 13); every level after is the Watchman's, the build's plan picking its feats (Toughness's, Jedi Defense's and Conditioning's tiers, the saber's specialization) and light-side powers first (Master Valor and Speed, Force Aura, Shield and Armor, Master Heal, Force Immunity and the rest), so the class's order never reaches a dark power.
- [ ] `choice` **Kreia's influence at Azkul's offer:** "I cannot allow your plans to succeed" (dantooine_vrook) costs Kreia 8; the other light-side way through his job offer is not checked.
- [ ] `feature` **Unspent skill points:** TSL's level-up keeps them for later; kjk's spends them (the build's plan puts what is left in the class's order).
- [ ] `choice` **The dark side playthrough,** start to end.
- [ ] `choice` **A female Exile:** the Disciple joins, not the Handmaiden.
- [ ] `choice` **Influence-gated content:** each companion's high and low influence talks, and the Jedi trainings (Atton, Bao-Dur, Mira, Handmaiden/Disciple, Visas).
- [x] `side` **HK-47's parts** and his assembly on the Ebon Hawk: see Nar Shaddaa's rebuilding above.
- [ ] `side` **Workbench crafting, lightsaber building and the crystal's growth.** The engine's workbench is there (`script::workbench`: breakdown, items made, the skill and component checks); no harness goal opens a bench, so no route crafts.
- [x] `feature` **Buying from a store**: `buy TEMPLATE [COUNT]` buys from the store open to the player, one a step once the world has settled, until the party carries that many (HK-47's control cluster and a spike from Akkere, his processor from Kodin).
- [x] `feature` **Pazaak in the routes**: Mebla's record and Doton Het's wager (telos_cantina_games), Atton's practice hand aboard (dantooine_khoonda); the Nar Shaddaa den (Kaalah-Nah's password, Dahnis, S4-C8, Geredi, the champion) is not played yet.
