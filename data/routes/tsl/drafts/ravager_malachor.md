# TSL drafts: the Ravager and Malachor V to the end credits

Drafted from the modules' own data (`kjk show`, `kjk dialogue show`,
`kjk script dis`, `kjk modules`), DSimpson's v1.6 order, Kerbow and
GameBanshee to cross-check. Not run. The dumps used are in
`context/rm/` (gitignored): one folder per module with its DLG trees
(`*.dlg.txt`, raw GFF with conditions' parameters in `*.dlg.raw`), its
scripts' listings and its templates; `context/rm/templates.txt` is every
template's tag, conversation and scripts.

Each route's ASSUMES block names its guesses; `# UNSURE:` marks a goal the
data could not confirm. Says sit before the goal that starts their
conversation (the harness queues them, `playthrough.rs`).

## Routes, in order

| Route | Starts | Saves | Modules |
|---|---|---|---|
| ravager_command_deck | `@finale_done` | `ravager_bridge` | 222TEL (if still there), 851NIH, to 852NIH |
| ravager_bridge | `@ravager_bridge` | `ravager_escape` | 852NIH, to 851NIH |
| ravager_escape | `@ravager_escape` | `malachor_surface` | 851NIH, 205TEL, 907MAL, to 901MAL |
| malachor_surface | `@malachor_surface` | `malachor_depths` | 901MAL, to 902MAL |
| malachor_depths | `@malachor_depths` | `malachor_remote` | 902MAL (Exile, then Mira), to 901MAL |
| malachor_remote | `@malachor_remote` | `malachor_academy` | 901MAL, 902MAL (the remote), to 903MAL |
| malachor_academy | `@malachor_academy` | `malachor_core` | 903MAL, 906MAL, 903MAL, to 904MAL |
| malachor_core | `@malachor_core` | none: `credits` | 904MAL |

Checkpoints for saves.list (not added there):

```
ravager_bridge     The Ravager: the bridge
ravager_escape     The Ravager: Nihilus dead, the last core owed
malachor_surface   Malachor V: the surface, the Exile alone
malachor_depths    Malachor V: the depths
malachor_remote    Malachor V: Bao-Dur's remote
malachor_academy   Malachor V: the Trayus Academy
malachor_core      Malachor V: the Trayus Core
```

## Modules, entries and exits

- **222TEL** (Citadel Station, the finale's): `tr_mandalorians` spawns
  Mandalore (`Mand`) and starts 222mand (start 0); `tr_mandalore` starts
  it again (start 1, "Are your preparations complete?"); "Let's go." runs
  a_playmovie, a_to851: MalMov04, `StartNewModule 851NIH FROM_222TEL`.
- **851NIH** (Ravager, command deck): entry `FROM_222TEL` (-3.5,75.0); the
  OnEnter starts 851mand from `RavagerIntro`. Exits: `TO_852NIH` trigger
  (199.2,26.9) to 852NIH `FROM_851NIH`; the way off is `tr_222esc`
  (-14.6,91.1), which starts 222tel on the `222TEL` placeable: leaving needs
  852NIH_Nihilus_Dead 1 and 851NIH_Bomb_Ravager over 2 (a_escape_851:
  MalMov05, then 205TEL when 101PER_Revan_End is not 1, else four movies
  and 907MAL). The bomb sites are `tr_851dem@1..4` (each its own script,
  tr_851demone..four, starting 851dem with Zuka spawned at
  `wp_851DEM_ZUKA`); mapnotes mn_bomb1..4 match them in GIT order.
  The missile bay: `851com` (control) and `851mis` (the missile), by the
  `Workbench`. Visas's cell: `tr_visas_med` (851visdr), `tr_visas_med2`
  (851medit). Tobin: creature `tobin` (851tobin). `TO_205` (-19.1,88.4)
  is placed but has no script.
- **852NIH** (Ravager, bridge): entry `FROM_851NIH` (12.1,41.2). `852Visas`
  trigger (852visas), the `Bridge` door (open; a_sealbridge locks it shut
  behind, a_start_ending opens and locks it open), the `852nih` trigger
  (k_852nih: the party warped, 852Nih). Exit: trigger `TO_852NIH`
  (23.7,44.5, its tag as 851's) to 851NIH `FROM_852NIH`.
- **853NIH**: a cutscene module (Visas's door, Nihilus, Sion, Tobin, Kreia
  as invisible placeables): not reached on this path.
- **205TEL** (area 204tel): entry (-49.0,-5.2) inside `tr_carth`
  (a_carthcut: carthend). carthend's EndConversation a_901jump: TelMov02,
  HypMov01, MalMov07, MalMov08, 907MAL_CUTSCENE 2, `StartNewModule 907MAL`.
- **907MAL** (area 904mal, the Core as a stage): k_907_enter by
  907MAL_CUTSCENE (0 kreiaend, 1 sioncut1, 2 krecut1, by its jump table);
  krecut1's a_kreiacut: `StartNewModule 901MAL` (its entry).
- **901MAL** (surface): entry (26.9,-12.2) by the Hawk. k_901mal_enter: on
  the first entry the party is emptied; with MAL_TURNINTO_REMOTE the remote
  is spawned (SpawnAvailablePUP 0 at `wp_remote_start`, added as NPC 1,
  SwitchPlayerCharacter 1) and Bao-Dur's hologram starts (901bao). Exit:
  `TO_902MAL` (-51.2,-134.0) to 902MAL `FROM_901MAL`. `tr_mira_han`
  (59.3,-148.3): Mira's waking scene (901mira). Consoles: `Console@1`
  (124.6,15.6), `Console@2` (-32.3,-194.8).
- **902MAL** (depths): entry `FROM_901MAL` (99.7,-144.1). `SealDoors`
  (k902boss: 902boss, the Greater Storm Beast `StormBeastGBoss`; the
  portcullises `Portcullis1`/`Portcullis2` start open). `tr_london_bridge`
  (-3.8,-13.5): flank, a_flank_cs: Mira's fight in this module
  (SwitchPlayerCharacter 7 at `sp_mira`, 902mira) or, without her, the
  remote (901MAL). a_mira / k_hanharr_ud: MAL_TURNINTO_REMOTE, 901MAL.
  Consoles `Console@1` (93.2,-98.5, console) and `Console@2` (-54.7,-219.4,
  conmain, the last: three found first). a_sensor_end: MalMov09,
  `StartNewModule 903MAL From_902MAL`. Exits: `TO_901MAL`, `TO_903MAL`
  (-3.8,36.5).
- **903MAL** (Trayus Academy): entry `FROM_902MAL` (0.1,-3.7), inside
  `tr_sionenter` (switch back from the remote, 903kreia). Exits:
  `TO_906MALa` (61.7,12.2) / `TO_906MALb` (58.7,60.4) to the Proving
  Grounds, `TO_905MALa..d` to the Crescent, `TO_904MAL` (-1.9,87.6) to the
  Core (behind the `SionDoor`s), `TO_902MAL`. `tr_sion` (-4.8,85.5): Sion.
- **905MAL** (Trayus Crescent): left out; `TO_905MALa..d` back.
- **906MAL** (Proving Grounds): `FROM_903MALa` (65.3,15.9), `FROM_903MALb`
  (62.7,64.1); exits `TO_903MALa` (63.2,18.0), `TO_903MALb` (60.1,67.8).
  The workbench at (99.9,-40.9).
- **904MAL** (Trayus Core): entry `FROM_903MAL` (0.1,-56.0); Kreia
  (`Kreia`, n_darthtraya001, 904kreia) at the centre; `CompAtt`
  (-4.7,-23.0, k_compatt) starts her talk; `AttonEnd` (k_attonend) does
  nothing as shipped. No way out: the game ends here.
- **950COR** (Ebon Hawk interior, holorec): not on this path.

## Who plays what

- The Ravager: the Exile leads Mandalore (NPC 2) and Visas (NPC 9) (222mand's
  a_set_party). Visas leaves the party briefly to meditate (a_visas_cs1/2).
  No party split into teams in the shipped game: the "teams" are
  Mandalorian squads on the comlink (Zuka, `g_mandalor002`, spawned by each
  bomb trigger) and one at the detonation (`g_mandalor003` at
  `wp_deton_mand`).
- Malachor: the Exile alone. **Mira** (NPC 7) against Hanharr in 902MAL
  (a_flank_cs: SwitchPlayerCharacter 7). **Bao-Dur's remote** (PUP 0 in NPC
  1's slot) on the surface and in the depths, priming the mass shadow
  generator at four consoles; sensorfi: G0-T0, HK-47 and the HK-50s
  (alignment picks the branch). Then the Exile again (tr_sionenter).
- Cut content, absent as shipped (TSLRCM restores them, not used here):
  Atton against the Disciple (903atton) and the Handmaiden against Visas
  (903hand) in 903MAL, Atton against Sion (attsion), Atton's dying scene at
  the Core (904atton, 904MAL_Atton_End), the Atton/Handmaiden/Mira/Visas
  kneeling spots in 904MAL (`wp_kreia_*`). Nothing in the modules' scripts
  starts these conversations.

## Skill-gated and optional content left out

- Tobin's [Persuade] (c_sc_per_gt): spared and set to blow the cores. The
  chain fights him.
- Nihilus's [Intelligence]/[Wisdom] lines (c_ac_int_gt and kin) and the
  Visas sacrifice (c_visas_melee, "You are his weakness"): dark/optional.
- Sion's [Persuade], [Awareness], [Wisdom], [Intelligence] erosions.
- Kreia's questions before the fight (start 3, R94 and its tree) and the
  friends' futures (R127: Mira, Mandalore, Visas, the Handmaiden...), each
  gated by globals and influence.
- Loot: Locker_Nihlus lockers, the DC 55 security rooms (door_nih003
  locked, DC 28 in the data), the Malachor corpses (Low/Mid/HighCorpse),
  the metal boxes, the workbenches (851NIH, 906MAL).
- The Trayus Crescent (905MAL); the Proving Grounds' south loop.
- The storm beasts in the surface's far east loop.

## Engine features the routes will likely need

- **SwitchPlayerCharacter to a companion who plays a whole fight** (Mira vs
  Hanharr) and to a **PUP** (`SpawnAvailablePUP`, `AddAvailableNPCByObject`
  putting the remote in Bao-Dur's NPC slot): the harness must drive the
  switched seat (T3-M4 at Vogga's warehouse is the nearest precedent) and
  --god should cover the creature played. `RemoveNPCFromPartyToBase`,
  `SetNPCSelectability`. Switching back: tr_sionenter's
  SwitchPlayerCharacter on arrival in 903MAL, after a module change made
  while the remote was the player.
- **The mass shadow generator** is only console conversations and
  `901MAL_Mass_Shadow` counted by a_setmassshad / c_chkmassshad, shared
  across 901MAL and 902MAL; conmain's c_global_lt gate. `a_sensordlgfire`
  starts sensorfi on the "Sensor"/remote object by `GetPCSpeaker`.
- **Boss phases by user-defined events at the last HP**: k_nihilus_ud
  (852nih start 1 at half), k_sion_ud (four falls, Sion's local number 13
  picking the start; `a_localn_set`), k_kreia_userdef (904MAL_Kreia_Battle
  0..3), k_hanharr_ud, k_boss_userdef (the Greater Storm Beast, 902boss
  start 1). Stun and buff drops in Nihilus's scenes (a_nihilus); Kreia's
  three floating lightsabers (`LightsaberFloat`, k_def_death01_ls,
  904MAL_Lightsaber).
- **Doors a script holds**: 852NIH's Bridge locked open, 902MAL's
  portcullises closed and locked by script and reopened, 903MAL's SionDoors
  (KeyRequired, plot) locked by tr_sion and unlocked by a_sion; d_entrance
  closed by a_903kreia.
- **Movies**: MalMov04 (to the Ravager), MalMov05 (its end), TelMov02,
  HypMov01, MalMov07, MalMov08 (to Malachor; a_901jump or a_escape_851's
  queue), MalMov09 (the remote's end), MalMov11a, MalMov11b, MalMov12 (the
  light ending; MalMov10 and MalMov14 the dark one). `QueueMovie`,
  `PlayMovieQueue`, `PlayMovie` before `StartNewModule`.
- **The end credits**: TSL's ending is 904kreia E109 a_playgoodmovie, then
  R112 **a_end_game**: GBL_GAME_COMPLETE set or incremented by
  G_PC_Align_Val, UnlockAllSongs, `EndGame(FALSE)`. No script calls
  `StartCreditSequence`. kjk's `credits` goal (playthrough.rs) is met on
  `Screen::End` only after `Screen::Credits`; `EndGame(FALSE)` alone shows
  `Screen::End`, read as game over. TSL's Movies folder has `Credits.bik`:
  the engine likely plays it on EndGame (verify in the exe, as K1's ending
  was) and kjk should then tell `Screen::Credits`. This is the last goal's
  proof and the most likely to need engine work.
- **Global boolean MAL_TURNINTO_REMOTE** carried across modules; module
  OnEnter scripts branching on it and on `GetLoadFromSaveGame`.
- **Area iteration and corpses**: Nihilus's body dissolves (a_end,
  NihilusZapped); Sion's last fall ends with his death in a conversation
  (E121 a_sion).
- 851NIH's `k_trans_abort` placeables (the global k_transabort
  conversation) sit by the transitions; 222TEL's c_endgame.
- 904MAL's IR (`kjk modules tsl 904mal`) lists one trigger of the GIT's
  two (CompAtt missing from the listing): worth checking the sim sees it.

## Riskiest guesses

1. The credits: EndGame(FALSE) without StartCreditSequence (above).
2. `@finale_done`'s place: finale_citadel.route saves it in 222TEL after
   222mand's start 0, away from `tr_mandalore`; the first route opens
   start 1 there. If that trigger fires only once, `use Mand` instead.
3. The switched seats: Mira's fight, the remote's walk with storm beasts,
   and the switch back in 903MAL.
4. Sion's four falls and Kreia's two phases each ending in a conversation
   the `fight`/`while` goals must hand over to.
5. Tobin's conversation: who starts it (`use tobin` or his own approach).
