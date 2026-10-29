# Korriban drafts (TSL)

Drafted 2026-10-29 from the modules' own data (`kjk show`, `kjk dialogue
show`, `kjk script dis`, read-only) and DSimpson's order (the cave and the
secret tomb before the Academy), cross-checked with GameBanshee 44-47 and
Kerbow's "Korriban". Not run. Every tag is the GIT's or its template's, every
`say` a reply's own text; the guesses are each route's ASSUMES block, marked
`# UNSURE:`.

## Routes and checkpoints

Each moves to `data/routes/tsl/` when its milestone comes up; the headers'
start commands already name that place. Add the checkpoints to `saves.list`
then.

| Route | Starts | Saves | Goals |
|---|---|---|---|
| korriban_valley | `@onderon_first_done` (aboard, Dxun) | `korriban_cave` (710KOR, the cave's mouth) | 17 |
| korriban_shyrack_cave | `@korriban_cave` | `korriban_tomb` (711KOR, alone) | 23 |
| korriban_secret_tomb | `@korriban_tomb` | `korriban_tomb_done` (710KOR, by the sealed door) | 61 |
| korriban_academy | `@korriban_tomb_done` | `korriban_vash` (702KOR, the detention room) | 43 |
| korriban_sion | `@korriban_vash` | `korriban_done` (003EBO, after Kelborn's message) | 15 |

saves.list lines, when they move:

```
korriban_cave         Korriban: the Shyrack Cave's mouth
korriban_tomb         Korriban: the secret tomb, alone
korriban_tomb_done    Korriban: out of the secret tomb
korriban_vash         Korriban: Master Vash found
korriban_done         Ebon Hawk: leaving Korriban
```

Party: Atton (NPC 0) and Bao-Dur (NPC 1). Kreia stays aboard: 701KOR's
OnEnter (`k_701area_enter`) takes her selectability away; her lines on
Korriban are the invisible `kreia_invis` placeables' (kreia_kr, kreia_cv,
sec_tomb, kreia_tm, kreia_si).

## Modules, entries and exits

| Module | Area | Entry (module.ifo / waypoint) | Ways out |
|---|---|---|---|
| 003EBO | Ebon Hawk | landing talk `kor_int` (Atton, Visas, Kreia) | `tr_leave` (party screen) to 701KOR |
| 701KOR | Valley of Dark Lords | the Ebon Hawk's arrival waypoint (72.6, 228.4); `tr_intro` plays kreia_kr's walk-off | `To_710` trigger (51.9, 63.7) to 710KOR `From_701KOR`; `To_702` trigger (37.3, -10.3) to 702KOR `From_701KOR`; `to_ebonhawk_custom` (`tr_ehawk_custom`, StartNewModule 003EBO at its outside waypoint) |
| 710KOR | Shyrack Cave | `From_701KOR` (10.4, 150.8); `From_711KOR` (241.3, 187.1), OnEnter `k_from_711` shuts `SealedDoor` | `To_701` trigger (6.9, 153.9); `To_711` behind `SealedDoor` (locked, plot): only sec_tomb's `a_tomb` (StartNewModule 711KOR `From_710KOR`) |
| 711KOR | Secret Tomb | `From_710KOR` (-137.2, -64.5), `tr_first_enter` (entrance_cs) | `To_710_2` (194.5, 23.2) behind `crypt_door@3` (unlocked after the last vision) to 710KOR `From_711KOR`; `To_710` at the entrance (-151.7, -66.7), between the entry and `KorribanDoor09` (unused: the tomb's way in is a_tomb's) |
| 702KOR | Sith Academy | `From_701KOR` (121.6, 156.6), `tr_enter` (Sion, the doors slam) | `To_701` (119.0, 168.1) behind `door_enter` (locked until termtort's "Open Front Door."); Sion's flight scene moves the party itself (`a_to_701kor`) |

Scenes and where they start:

- 003EBO: `kor_int` on landing (who starts it not seen; a_kor_int plays the
  movie and fades). The Kelborn message on return: `k_003ebo_enter`'s
  `CUT_MSG_FROM_KELBORN` (502OND_End_First, 403DXN_Return_Trigger,
  000_Jedi_Found), dialog `t3kelbrn`.
- 701KOR: `tr_intro`, `corpse_warn` x7 (kreia_kr start 7), `tr_hist_*`
  (Kreia's tomb histories), `tr_cave_banter` (kreia_cv, then `tr_shyrak`
  spawns two shyracks), `tr_acad_banter` (kreia_ac), `tr_exit702_end`
  (closes `KorribanTempleDoor` after Sion).
- 710KOR: `tr_tukata_attack` x8, `tr_sith_ass` (a_sith_att), `tr_secret_tomb`
  (jumps the party to `wp_neartomb`/`wp_cnpc1`/`wp_cnpc2`, starts sec_tomb).
- 711KOR: `tr_first_enter`, `tr_1st_vision` (malak; locks 711_norm_door and
  711_1st_door), `711_2nd_door`'s OnOpen `tr_pc_god` (re_flash), the area's
  heartbeat `k_hb_check_mines` (re_flash start 2 once no mine is left near
  `check_mines`), `tr_evil_kreia` (kreia_vs; locks the first `711_last_door`),
  `tr_last_vision` (creates `g_darkpc`/`g_darkpc_male`, the Exile's double),
  `tr_revan_appears` (revan), `711kor_ud` (`k_inc_korriban`: unlocks each
  stage's doors, the crypt doors and starts rev_end), `sarc_door`'s OnOpen
  `tr_kreia_tm` (kreia_tm).
- 702KOR: `tr_enter` (sion start 0), the `tr_sith_ass_*` ambushes,
  `tr_gas_trap`, `tr_vash` (vash), `tr_sion_fight` (sion start 1), Sion's
  `k_sion_ud` (kreia_si), sion start 2 (the flight, `a_to_701kor`).

## Gates

- **The secret tomb:** sec_tomb lets the Exile in on "not c_align_bet 26 74
  and c_global_gt G_PC_LEVEL 9": alignment 25 or less, or 75 or more, and the
  global G_PC_LEVEL above 9. Short of it Kreia's scene knocks him down at the
  door (start 3) and the drafts block at `module 711kor`. If the chain's
  Exile is short, take the Academy first (korriban_academy's first two
  goals become `exit To_702` from the valley) and the cave after, or leave
  the tomb (it is optional: its Light Side points and loot only).
- **The cave's banter** (kreia_cv): "finish the Academy first" when strong
  enough, "not yet strong enough" otherwise, same gate; either way only
  words.
- **termquiz:** "Enter 'New Recruit.'" needs `datapad_recruits`
  (footlocker_comp); the "[Computer]" ID line and "Bypass login" need Computer
  Use (c_sc_com_gt), not taken. The test needs 4 of 5.
- **termtort:** "Enter \"Lonna Vash.\"" needs `datapad_vash` (vash_container).
- **Skill-gated replies left out:** kreia_kr's "[Awareness] You sound like you
  disapprove." (Ajunta Pall's tomb: then "No one is beneath redemption", LSP
  and +INF); re_flash's "[Demolitions] Let me go first this time." (the same
  end as "Not this time, I'll go up there alone."); termtrn's "[Computer]"
  ID.

## Optional content left out

- The valley's skeletal corpses (corpse_tr_hss*: two Hssiss each), the tomb
  histories off the way (tr_hist_sadow, _hord, _pall, _ragnos).
- The cave's side passages' jars, corpses and rubble.
- The tomb's side rooms (first vision north and south, the minefield's north
  and south) and the dead Jedi's endless Hssiss (one search only).
- The Academy's library (its assassins, the Thorium Charges: Security, never
  bashed), Uthar's room (door_thorium, the Sith Holocron), door_locked_dif,
  the south passage, the locked and trapped footlockers, the west hall's
  datapads. GameBanshee keeps the Thorium Charges for Dxun anyway.

## Engine features the routes likely need

- **The party taken away and given back.** sec_tomb's `a_tomb` jumps the
  companions to `wp_cnpc1`/`wp_cnpc2`, slows and walks the Exile into the
  tomb, `StartNewModule("711KOR", "From_710KOR")`, then loops NPCs 0-11 with
  `IsNPCPartyMember`/`RemoveNPCFromPartyToBase` (a script that runs on after
  StartNewModule: check it runs before the module goes, or the companions
  arrive in 711KOR). `tr_ehawk_custom` does the same on the way aboard.
- **The party selection away from the Hawk, opened by the player.** Out of
  the tomb the Exile is alone in 710KOR and nothing shows the party screen
  (`k_from_711` only shuts SealedDoor): DSimpson's player opens it from the
  in-game menu. The harness's `party` only queues picks for a screen a
  script shows (`Goal::Party` in playthrough.rs). Needs a goal that opens
  it (the menu's Party button, ShowPartySelectionGUI), with Kreia's
  selectability (`SetNPCSelectability`) honoured. Without it the Academy is
  played alone (nothing in it needs a companion).
- **Vision fights.** `SetMinOneHP` on the Exile through the tomb, Malak's,
  the Mandalorians' and the Jedi's fade (`k_fab_fade_sp`/`_ud`,
  `k_fab_malak_ud`, `k_mand_fade_ud`), the companions' copies (`a_evil_kreia`
  spawning from `wp_atton`, `wp_baodur`, ...: `GetNPCSelectability` counts
  `711KOR_Party_Count`), `KreiaEvil` with Min1HP and NoPermDeath, and
  `711KOR_Vision_Track` in `711kor_ud` moving each stage on. `pc_death`
  plays when the Exile "falls" in a vision (Min1HP reached): check --god.
- **The Exile's double.** `tr_last_vision` creates `g_darkpc` (or
  `_male`), `DuplicateHeadAppearance` from the PC, raises its attack and
  hit points to the PC's: the "Revan" fought is that double.
- **Mines sprung by walking.** The minefield's six `G_T_TRAP005` triggers
  (Average Frag Mines, two "{KillsOnEnter}" with `a_mine_killnpc` for the
  soldiers) must all go before `k_hb_check_mines` lets the captain charge
  (`GetFirstObjectInShape` over traps round `check_mines`). The drafts walk
  onto each; there is no goal to disarm or recover one with the Exile's
  Demolitions, and the walker may plan round a mine it knows.
- **Locked doors bashed.** `711_lock_door@1` and `@2` ("Locked, Bashable",
  DC 28, 120 HP, hardness 5): `bash` on a door, alone.
- **A fight with an unkillable foe.** Darth Sion keeps 1 HP; `fight` has to
  end by Kreia's call and the flight's module change, not by "none hostile".
- **The containers' OnClosed.** SevrdArm (`a_jedi_tomb_jrnl`) and jedi_cont
  (`a_jedi_jrnl_fin`, the quest's end and the Hssiss): `use` must close them.
- **Conversation owners that are placeables:** `kreia_invis` on every
  Korriban module (Kreia's voice), `SionEnter`, `i_mandcs`, `rev_end`.
- Several scripts misname a waypoint (`a_tomb` jumps to `sp_leave_pc`, the
  GIT has `wp_leave_pc`): the games' own slip, harmless if a missing
  target is a no-op.
