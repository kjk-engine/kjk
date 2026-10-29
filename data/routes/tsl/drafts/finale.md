# TSL finale drafts: Onderon's civil war to the Ravager's shuttle

Drafted 2026-10-29 from the modules' own data (`kjk show`, `kjk dialogue
show`, `kjk script dis`, read-only) in DSimpson's order (IGN chapters 12 to
15: "Onderon Finale", "Dantooine Again", "Telos - Hidden Base Again",
"Telos - Citadel Station Again"), cross-checked with GameBanshee 48 to 56 and
Kerbow. Not run. Every tag is the GIT's or its template's (`@N` where it
repeats), every `say` a reply's own text; the guesses are each route's
ASSUMES block, marked `# UNSURE:`. They follow CHOICES.md: the Queen's side
on Onderon, light side replies, Kreia's influence kept up (+INF on Vaklu's
fate), a male Exile (the Handmaiden's scenes on Telos).

Dxun's tombs fall here: in DSimpson's order the first Onderon/Dxun visit
ends with Mandalore joining, Korriban follows, and Kelborn's message
(t3kelbrn, played at the end of korriban_secret_tomb) calls the Exile back to
Dxun for the split.

## Routes and checkpoints

| Route | Starts | Saves | Goals |
|---|---|---|---|
| finale_dxun | `@korriban_done` (003EBO, Kelborn's message heard) | `dxun_sith_tomb` (411DXN, the tomb's entrance, Visas's squad) | 37 |
| finale_sith_tomb | `@dxun_sith_tomb` | `onderon_civil_war` (511OND, the Basilisk landed) | 37 |
| finale_sky_ramp | `@onderon_civil_war` | `onderon_royal_palace` (506OND entrance) | 37 |
| finale_palace | `@onderon_royal_palace` | `onderon_won` (003EBO, after the Dxun reunion) | 62 |
| finale_enclave | `@onderon_won` | `telos_atris_again` (262TEL, from 006EBO) | 24 |
| finale_atris | `@telos_atris_again` | `telos_citadel_siege` (221TEL, the Ithorian compound) | 24 |
| finale_citadel | `@telos_citadel_siege` | `finale_done` (222TEL, fuel saboteurs dead, Mandalore waiting) | 30 |

251 goals. At `finale_done` 222mand's start 0 has played at
`tr_mandalorians` (Mandalore, Kelborn and Zuka spawned, Visas Marr taken by
a_set_party's forced party screen) and the TSF station is clear; the next
route (the Ravager) starts with `say =Let's go.`, `to tr_mandalore`, `talk`
(222mand start 1: MalMov03, a_mandalore to 851NIH). ravager_command_deck's
`if in 222tel` block expects start 0 still owed: its first `talk` at
tr_mandalorians finds the trigger spent (a local boolean), its first
"Let's go." then answers start 1.

saves.list lines, when they move:

```
dxun_sith_tomb        Dxun: Freedon Nadd's tomb (the strike squad)
onderon_civil_war     Onderon: the Basilisk in the Merchant Quarter
onderon_royal_palace  Onderon: the Royal Palace
onderon_won           Ebon Hawk: leaving Onderon
telos_atris_again     Telos: Atris's academy again
telos_citadel_siege   Telos: Citadel Station under the Sith
finale_done           Telos: Citadel Station, ready for the Ravager
```

Parties: ashore on Dxun Kreia and Visas Marr; the strike squad Visas Marr
(leader), Bao-Dur, Atton (Kelborn's split); Iziz the Exile, Kreia (forced)
and Mandalore; Dantooine Kreia and Visas Marr (sent away in the Enclave);
the Handmaiden alone on Telos; Citadel Station whom a_hand_pc_end leaves,
Visas Marr forced at the end (Mandalore joins on the Ravager).

## Modules, entries and exits

| Module | Area | Entry | Ways out |
|---|---|---|---|
| 003EBO | Ebon Hawk | galaxy map `Galaxymap` (50.7, 74.3) | `tr_leave` (64.5, 48.8), the party screen |
| 401DXN | Dxun - Jungle Landing | entry (-117.2, -143.8); `From_506OND` from the Palace | the guide `man_guide` (k_401enter spawns f_man_guide at wp_manspawn): a_guide2_camp to 403DXN; `to_ebonhawk` (-121.4, -148.5) to 003EBO |
| 403DXN | Dxun - Mandalorian Ruins | the guide's a_guide2_camp arrival (entry -33.9, 29.4, by From_402DXN); `From_411DXN2` (-2.7, 112.4) from the tomb | Kelborn's a_to_tomb (StartNewModule 410DXN From_403DXN); Zuka's a_to_iziz, party screen, a_to_iziz2 (OndMov07, 511OND From_403DXN) |
| 410DXN | Dxun - Jungle Tomb | `From_403DXN` (85.9, -10.2) | `To_411` trigger (-22.8, 129.2), linked From_410DXN (missing in 411DXN: the template from_410dxn is tagged From_231TEL_D) |
| 411DXN | Dxun - Sith Tomb | entry (-0.2, 25.8) | Xarga's a_leave_411 (403DXN From_411DXN2, the Exile the player again); `exit_door` (0, 22.7) back to 410DXN |
| 511OND | Onderon - Merchant Quarter (ruined) | `From_403DXN` (49.9, -11.5) | `To_504` trigger (5.7, 47.6), linked From_510OND (missing in 504OND) |
| 504OND | Onderon - Sky Ramp | entry (78.7, 16.2) / `From_502OND` | `To_506` trigger (-63.5, 160.8) to 506OND From_504OND; `To_502` back |
| 506OND | Onderon - Royal Palace | `From_504OND` (0.4, 21.8) | a_end_game (OndMov06, 401DXN From_506OND); `To_504` back |
| 601DAN | Dantooine - Khoonda Plains | the Hawk's landing (325.6, 200.7) | `To_605@1` (496.3, 138.5) to 605DAN From_601DAN2 |
| 605DAN | Dantooine - Enclave Courtyard | `From_601DAN2` | `To_650` (104.1, 95.6) to 650DAN FROM_605DAN |
| 650DAN | Dantooine - Rebuilt Jedi Enclave | `FROM_605DAN` (82.0, 117.7) | `Trans006EBO` (78.9, 126.4), heartbeat k_006ebo: StartNewModule 006EBO once 650DAN_006EBO_Trans |
| 006EBO | Ebon Hawk (the cutscene copy) | entry (48.5, 52.6) | visasend's a_load262 (TelMov15, 262TEL FROM_006EBO) |
| 262TEL | Telos - Secret Academy | `FROM_006EBO` (57.2, 94.0) | a_hand_pc_end after handend (MalMov02, MalMov01b, 221TEL) |
| 221TEL | Citadel Station - Suburban (Res 082 West, the Ithorian compound) | entry (-57.4, -23.0) | `to_220TEL` trigger (-29.9, -13.0) to 220TEL from_221TEL |
| 220TEL | Citadel Station - Suburban (Res 082 East) | `from_221TEL` (-26.6, -16.0) | door `d_urban` (36.3, -0.7) to 222TEL from_220TEL |
| 222TEL | Citadel Station - Entertainment Module 081 | `from_220TEL` (63.7, 6.4) | Mandalore's 222mand start 1 (MalMov03, a_mandalore to 851NIH); `TO_851NIH` door (-15.9, -63.4, sealed) |

Scenes and where they start:

- 403DXN: Kelborn (kelborn start 4 Tobin's / 5 Kavar's, "You received my
  message, then?") from k_403_enter; Zuka's Basilisk (zuka start 2) on the
  return from 411DXN.
- 410DXN: xarga start 0 on entry, start 1 at `tr_relay_conv`; the waves
  `tr_wave1`..`tr_wave4`, `tr_wave_left`/`tr_wave_right`; `tr_sith_camp`.
- 411DXN: xarga start 0 on entry; sithcapt at `tr_intro_dlg`; beast_ta at
  `tr_beast_tamer`; ritlead start 0 at `tr_ritual_sc_1`, start 1 at
  `tr_ritual_sc_2`; xarga start 1 at `tr_leave_411`.
- 511OND: ro_sold on entry (k_511_enter); cs_tal_3 at `tr_cs_tal_3`.
- 504OND: ro_trp1 `tr_trooper1`, ro_trp2 `tr_trooper2`, ro_trp3
  `tr_trooper3`, bostuco starts 0 to 4 (`tr_bost_2` and the officers'
  triggers), ro_brck in the barracks, term_acc in the tower.
- 506OND: bostuco on entry; tobin start 0 at `tr_tobin_door`, start 1 at the
  inner door; va_capt at `tr_capt_interr`; kiph, termeast, the caged Riiken
  (mp_riike); kavar start 1 at `tr_security_north`; ro_weed (Kadron),
  ro_corp; thronecs at `tr_royal_battle`, kaventer at `tr_kavar_enter`,
  vaklu starts 0 and 1, kavar start 2, lrn_form, kavar start 3, talia,
  506kreia.
- 401DXN: reunion (k_401enter, 506OND_End).
- 003EBO: kreia start 7 (the Enclave), aboard after Onderon.
- 650DAN: 650kreia at `KreiaArriveParty` (DanCourt), 650jedi start 6 (the
  Masters alive), krehand1, krealone (the dark side's), `Trans006EBO`.
- 006EBO: attonend, visasend (a_load006end: VisasMarr for a male Exile).
- 262TEL: kreatris, sisend, atrend1, atrend2, atrruncs, atrend3 (start 0 at
  `tr_atrend3`, start 1 after the fight), handend (a_hand_pc_end).
- 221TEL: grenn at `tr_221_enter`, 221mand at `tr_mandalore`, riiken
  (ond_lead), zherron (dan_lead) at `tr_dantooine`.
- 220TEL: tsflead (tsf_lead), `tr_220_enter`, `tr_tsf_attack`.
- 222TEL: 222mand start 0 at `tr_mandalorians` (once), start 1 at
  `tr_mandalore`; onddoor at `tr_tsfoffice`; a_tsfsecurity on
  `d_tsfsecurity`'s OnOpen; ondwound (wound_sold) on use.

## Skill-gated and optional content left out

- Awareness: Kelborn's "[Awareness] I have to lead the group heading to the
  royal palace" (c_sc_awa_gt), Kiph's "[Awareness] I don't believe a word",
  Kreia's "[Awareness] You are afraid." (650kreia), Xarga's
  "[Precognition]".
- Repair / Computer / Demolitions on Dxun: the perimeter sensor (stealth,
  Bao-Dur's codes, a detonation pack), the Turret and Droid Power
  Generators, the camp's Security Terminal ("Foothold", 10 spikes); the
  411DXN broken droid and medical droid repairs; the palace terminals'
  camera and schematics menus (not gated, just skipped).
- Puzzles: 411DXN's puzz_sw ((6*2)-8+9*1) and puzz_se (C), the palace
  storage room's term_puz (66, 45, 39), the gas room's terminal.
- Persuade / Treat Injury: Talia's "[Persuade] If you kill him, you make
  him a martyr" (the route takes the ungated "This act will set the tone"
  instead), the wounded Onderon soldier on Citadel Station (both checks;
  the failed Persuade leads to a DSP reply, so he is left alone).
- Battle Meditation in the throne room (thronecs R8, R4: the power).
- The Dark Side Energy taints in 411DXN (a Will save; a DSP on failing it).
- Minigames: the Sky Ramp's turret (term_acc "Control the turret.", 505OND).
- Loot: the palace museum (Talia gives its relics), the Sith camp, the
  barracks lockers, the workbenches, the 650DAN corpses and footlocker,
  Ent 081's merchant room.
- 221TEL's TSF soldiers, Chodo Habat and Moza.

## Engine features the routes likely need

- **Party splits.** Kelborn's split (403DXN: a_set_partyup, a_to_tomb
  clears the party, RemoveNPCFromPartyToBase, SetNPCSelectability) and its
  undoing (a_leave_411, a_end_reunion with AddPartyMember from the
  403DXN_PARTY2_NPC* globals); the Enclave sending the party off
  (a_650removeparty); a_set_party on Citadel Station (SetSoloMode, a forced
  ShowPartySelectionGUI).
- **The player switched to a companion.** k_410_enter's
  SwitchPlayerCharacter(403DXN_PARTY2_NPC1) for the whole of 410DXN and
  411DXN (the Exile hidden at the camp), back with SetPartyLeader(-1) and
  SwitchPlayerCharacter(-1) in a_leave_411. The engine has
  SwitchPlayerCharacter (PR 51), but not a whole two-module segment led by
  it: the harness's seat, the walker, `fight` and the companions' follow
  all need to read the switched creature, and a checkpoint (dxun_sith_tomb)
  is saved mid-switch. On Telos the Handmaiden fights her sisters and Atris
  as the party leader (a_sisend, a_atrend1 read GetPartyLeader); who leads
  there is not settled by the data read.
- **Forced party screens.** a_to_iziz (ShowPartySelectionGUI forcing NPC 2
  or 6, exit script a_to_iziz2), a_set_party (forcing 9); `party` must take
  a pick list that includes or omits the forced one.
- **Large battles with allies.** 410DXN's ramp (turrets, war droids, waves
  of troopers, Dark Jedi); 511OND/504OND/506OND with Royalist and Vaklu
  soldiers on both sides, Bostuco's men following (a_run_to_you); 221TEL's
  Zherron charge and TSF officers. Encounters are LS/DS-gated
  (SetEncounterActive on the ls_*/ds_* encounters in k_506_enter).
- **Scripted fight ends.** Fights stopped at a minimum of hit points that
  hand over to a conversation: Vaklu (vaklu start 1), Atris twice (the
  Handmaiden's loss in atrend2, the Exile's win in atrend3: SetMinOneHP),
  the Sith Master's ritual backlash (a_ritual_sc_1). The throne room's
  battle ends when Kavar enters (kaventer: "all the opponents will just
  instantly fall dead", DSimpson). `fight` must stop when the scene takes
  over.
- **Force fields and bashing.** 504OND's ffield_lower (OnOpen a_bash_ff)
  and ffield_upper (Bostuco: "bash your way through it with your
  lightsaber"); 000ffield's Bao-Dur line. The route bashes the upper one.
- **Teleporting guides.** Riiken's and the Royalist Corporal's a_guides
  (wp_tele_*), Kadron's a_dline_pctele (wp_bgone_pc): the walker must take
  the jump without counting it as a stall.
- **Movies between modules.** OndMov07, OndMov06, TelMov15, MalMov02,
  MalMov01b, MalMov03 (a StartNewModule's movie arguments and PlayMovie).
- **Linked waypoints missing.** 511OND To_504 to From_510OND and 410DXN
  To_411 to From_410DXN have no such tag in the destination: the arrival
  must fall back on the module's entry, as the games do.
- **Dialogue replies under nodes with no condition but a skill label**
  ("[Computer] ..." on the palace terminals, term_acc): the harness answers
  them like any other; they are not gated in the data.
