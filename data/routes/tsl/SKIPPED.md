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

- [ ] `blocker` **telos_base_shuttle:** the route leaves the bay control room by `TelosDoor2@7`/`@6`. The nav mesh blocks a face only when most of it lies under a placeable, so it plans between two barrels in the south doorway.
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

- [ ] `blocker` **Khoonda's militia desk corner (-6.9, -15.3):** every walk back from Zherron's post sticks there. Mapped (2026-10-29): the Administrator's office joins the hall only through a diagonal channel between the desk's sloped nonwalk foot and the wall, one body wide; the navigation mesh's corner there is stepped off short (the full step would leave the mesh), the walk makes from it straight for the next corner across the desk's foot, and its sidesteps and backing off bring it back to the corner, which resets its stalls. Behind it: Zherron's kinrath job, Dopak's message, the defence tasks (the office's door to the turret room is the master card key's, reached through the channel), Akkere's hydrospanner (in the second assault droid). The fix belongs to the walk (a corner reached again without headway) or the mesh (a funnel leg checked for the box), measured on `kjk collision walk` across modules.
- [ ] `side` **Zherron's kinrath job** and its 2000-credit reward (blocked by the desk corner above).
- [ ] `side` **The cave Matriarch**, who only spawns with Zherron's job.
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
- [ ] `side` **Dopak's message.**
- [ ] `side` **Khoonda's defence tasks** (turrets, mines, droids, militia) before the battle: written once (Zherron's tasks, the turrets to hostiles, the medic droid, the assault droids, the recruits, Akkere's hydrospanner and thorium) and taken out again: the start of dantooine_battle stands in the office, behind the desk corner. Laying mines at a place has no harness goal.
- [ ] `side` **Akkere's hydrospanner.**
- [ ] `side` **The lightsaber build:** the lens fixture comes only from a forged will.
- [ ] `feature` **The party swap before the battle** (Handmaiden in, Kreia out): the harness has no goal to open the party screen away from the Hawk.
- [x] `choice` **Berun's conversation**: DSimpson's light way, his doubts of Zherron asked after and a look into him taken on (dantooine_khoonda). His message to Dopak through Zherron waits on the desk corner (the blocker above).
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
- [ ] `feature` **T3-M4's Security checks** pass DC 28 doors with a total of 15. Check the roll against the exe.

## Onderon and Dxun, the first visit (PR to come)

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
- [ ] `feature` **The harness's chase takes up a gun** for a foe it cannot reach and keeps it: the routes put the Exile's lightsaber back in hand (`equip g_w_lghtsbr01`).

## Later milestones

The drafts in `drafts/` list their skips in their notes files, and each
milestone's PR moves them here.

## Whole-game items

- [x] `feature` **The chain plays a created Jedi Exile** (`exile.build`, chain.flags): TSL's creation of a light-side Sentinel, levelled by the build's plan; the masters teach their forms (Vrook, Zez-Kai Ell).
- [ ] `feature` **Prestige classes** are not reached: Kreia offers one (`c_pc_prestige`, `a_pc_jedi`'s `AddMultiClass`) from level 15, and the chain's Exile is level 7 leaving Nar Shaddaa. The engine side is there (the class taken up unlevelled with the next level's XP, its powers from its own `spells.2da` column); a route for it waits for the later planets.
- [ ] `choice` **Kreia's influence at Azkul's offer:** "I cannot allow your plans to succeed" (dantooine_vrook) costs Kreia 8; the other light-side way through his job offer is not checked.
- [ ] `feature` **Unspent skill points:** TSL's level-up keeps them for later; kjk's spends them (the build's plan puts what is left in the class's order).
- [ ] `choice` **The dark side playthrough,** start to end.
- [ ] `choice` **A female Exile:** the Disciple joins, not the Handmaiden.
- [ ] `choice` **Influence-gated content:** each companion's high and low influence talks, and the Jedi trainings (Atton, Bao-Dur, Mira, Handmaiden/Disciple, Visas).
- [x] `side` **HK-47's parts** and his assembly on the Ebon Hawk: see Nar Shaddaa's rebuilding above.
- [ ] `side` **Workbench crafting, lightsaber building and the crystal's growth.** The engine's workbench is there (`script::workbench`: breakdown, items made, the skill and component checks); no harness goal opens a bench, so no route crafts.
- [x] `feature` **Buying from a store**: `buy TEMPLATE [COUNT]` buys from the store open to the player, one a step once the world has settled, until the party carries that many (HK-47's control cluster and a spike from Akkere, his processor from Kodin).
- [x] `feature` **Pazaak in the routes**: Mebla's record and Doton Het's wager (telos_cantina_games), Atton's practice hand aboard (dantooine_khoonda); the Nar Shaddaa den (Kaalah-Nah's password, Dahnis, S4-C8, Geredi, the champion) is not played yet.
