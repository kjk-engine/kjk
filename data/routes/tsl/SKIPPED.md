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
- [ ] `skill` **Corrun Falt** is persuaded down to 500 credits ("If Lorso's dirty", Persuade over 4), but the first check ("Of course not", over 10) fails: the chain's level 5 Exile has Persuade 10, so he is still paid.
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
- [ ] `skill` **Ralon's holocron:** both light-side ways need Awareness 4, which the Sentinel now has (10 by the Enclave); the route still leaves it.
- [ ] `skill` **The protocol droids, conduits and storage overload** in the Enclave sublevel.
- [ ] `skill` **Jorran's Persuade:** "I saved your life" asks Persuade over 11, the chain's Exile has 11 there; he is paid.
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
- [ ] `feature` **Visas's joining conversation is cut short** after the Handmaiden's line, because the game's own `a_next_scene` destroys the owner (VisasCut), so the lightsaber-part notice and the second `a_next_scene` never play. Check in the exe whether a conversation talks on after its owner is destroyed.
- [ ] `side` **Goto's yacht:** the plasteel containers and the sentry droids' scavenging.
- [ ] `side` **Goto's yacht:** T3-M4's combat upgrade, the Reset program and the lab station.
- [ ] `choice` **The yacht hub scene's G0-T0 line** (Mira's plays instead).
- [ ] `side` **The companion talks aboard after Nar Shaddaa,** and HK-47's rebuilding.
- [ ] `side` **Fassa's passage** for Kahranna's family and Odis.
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
- [ ] `side` **HK-47's parts** and his assembly on the Ebon Hawk.
- [ ] `side` **Workbench crafting, lightsaber building and the crystal's growth.**
