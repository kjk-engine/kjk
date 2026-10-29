# Onderon and Dxun, the first visit: notes for the drafts

Drafted from DSimpson's order (IGN's chapter 10 is his text), cross-read
with GameBanshee 25 to 31, and checked against the modules' own data
(`kjk show`, `kjk modules`, `kjk dialogue show`, `kjk script dis`). Every
`say` matches a reply in the DLG it is meant for; every tag is the GIT's
or the template's. Nothing here has been run.

## Routes and checkpoints

| Route | Starts from | Saves | saves.list line |
|---|---|---|---|
| dxun_landing | `@nar_shaddaa_done` | dxun_jungle | `dxun_jungle          Dxun: the jungle, from the landing` |
| dxun_camp | `@dxun_jungle` | dxun_tests | `dxun_tests           Dxun: Mandalore's camp, the tests taken` |
| dxun_tests | `@dxun_tests` | dxun_tests_done | `dxun_tests_done      Dxun: Mandalore's camp, four tests done` |
| dxun_departure | `@dxun_tests_done` | onderon_spaceport | `onderon_spaceport    Onderon: the Iziz spaceport` |
| onderon_spaceport | `@onderon_spaceport` | onderon_alibi | `onderon_alibi        Onderon: Riiken asked about Dhagon` |
| onderon_alibi | `@onderon_alibi` | onderon_dhagon | `onderon_dhagon       Onderon: Dhagon Ghent freed` |
| onderon_kavar | `@onderon_dhagon` | dxun_return | `dxun_return          Dxun: back from Iziz` |
| dxun_return | `@dxun_return` | onderon_first_done | `onderon_first_done   Ebon Hawk: leaving Dxun` |

Korriban's first route starts `@onderon_first_done`, aboard the Ebon Hawk
(003EBO).

## Modules, entries and exits

| Module | Area | Entry | Ways out used |
|---|---|---|---|
| 003EBO | Ebon Hawk | `wp_pc_walk_map` after the blockade (k_sup_galaxymap reloads it) | `tr_leave` to 401DXN (003EBO_RETURN_DEST 4) |
| 401DXN | Dxun - Jungle Landing | module entry by the Hawk (-117, -144); `From_402DXN` | `To_402` (4.9, 98.8); `to_ebonhawk` |
| 402DXN | Dxun - Jungle | `From_401DXN`, `From_403DXN`, `From_404DXN` | `To_403` (16.7, 56.4); `To_401`; `To_404` (cache, not used); the sentries' `a_to_mandalore` (403DXN at `wp_meetmandalore`) |
| 403DXN | Dxun - Mandalorian Ruins | `From_402DXN`, `wp_meetmandalore` | `To_402` (-46.3, 16.0); `a_to_iziz2` (party screen, then 501OND `From_403DXN`); `man_guide` (`a_guide2_camp`: 401DXN) |
| 404DXN | Mandalorian Cache (area 400dxn) | `From_402DXN` | not entered |
| 501OND | Onderon - Iziz Spaceport | `From_403DXN` by the G-wing | `To_502` (18.0, 100.3); `shuttle_iziz` (`a_to_dxun`: 403DXN) |
| 502OND | Onderon - Merchant Quarter | `From_501OND`, `From_512OND`, `From_Alibi1` | `To_512` (-7.0, 31.4); `To_501` (-0.4, -61.7); `tr_sky_ramp` (504OND: the finale, not used) |
| 512OND | Iziz - Western Square | `From_502OND`, `From_503OND`, `From_Alibi2` | `To_503` (-76.1, 2.4); `To_502` (3.2, 17.1) |
| 503OND | Onderon - Iziz Cantina | `From_502OND`, `From_502OND_Dhagon` | `To_502` (149.5, 112.8), which leads to **512OND** (`From_503OND`) |

Not in this visit: 421DXN (the Hawk's turret, "I'll man the turret"),
504OND (Sky Ramp), 505OND (Onderon Turret minigame), 506OND (Royal
Palace), 510OND (swoop track), 511OND (ruined Merchant Quarter), 410DXN
and 411DXN (the tombs).

## Left out

Skill-gated:
- 401DXN: the droid carrier's computer port (`comp_port`, br_term:
  Repair parts, then Computer spikes) and the three remote droids
  (`TDROID`, `TDROID2`, `TDROID3`: Repair parts per system).
- 403DXN: Zuka's navigational telemetry (`bwire` Repair, `tele_comp`
  Computer 8 spikes, then three Phase Pulse parts from cannoks'
  random remains: tsupport and tcannoks).
- 404DXN: the cache's droid foreman (Computer) and its mines.
- 402DXN: Kumus's reward ([Persuade] / [Force Persuade]); the sealed cache
  `DxunCache_imp` (Thorium charges from Dantooine's Akkere or Korriban).
- 502OND: Bostuco, Riiken's Force Persuade, the journalist's Awareness
  line; 503OND: the swoop boss's Persuade, Gelesi's.
- Iziz's checkpoint guard's [Persuade]/[Force Persuade] (Kreia or HK-47
  influence; neither is in the party).

Optional, left as costing fights or detours:
- The turret minigame on arrival (421DXN, 600 XP).
- Mandalore's battle circle (five one-on-one duels under rules: no
  weapons, no powers; Davrel's rematch outside camp follows it), the
  cache's interior (404DXN), Kex's store.
- The beast rider's boma at the spaceport (refusing his credits: LSP).
- The open starport visas: Kiph's slicing (500 credits) of the Rodians'
  visa, Bakkel's open visa, and their buyers (Gormo, Xaart, Terlyn: light
  side; Tolas: neutral; Sakarie: dark).
- Ponlar's riot (a second pass by the market; "I won't fight civilians"
  keeps out of it), Anda's captains (Vaklu's side only), Gegorran's,
  Margar's, 1B-8D's and Kex's stores, Nikko's pazaak, the swoop races
  (510OND), the news holograms, Mandalore's back room after the return
  (`locked_door`; MilHighMetalBox, MilMidMetalBox@1, @3, CivilBagStrap@3).

## Engine features the routes will likely need

- **The galaxy map's first trip to Onderon** (k_sup_galaxymap
  `PLANET_ONDERON`): 401DXN_STARTED 1, OndScn01 queued, 003EBO reloaded at
  `wp_pc_walk_map` (DoPlanetChange); then Atton's attond starts there, and
  `a_dxun_cs` plays ONDScn02 and the queue and starts ond_rece. The route
  travels to row 7 (Onderon), not row 1 (Dxun).
- **Party selection away from the Hawk** with a forced member:
  403DXN's `a_to_iziz` calls `AddAvailableNPCByTemplate(2, "p_mand")` and
  `ShowPartySelectionGUI("a_to_iziz2", ...)` with Mandalore forced; the
  exit script plays a movie and `StartNewModule("501OND", "From_403DXN")`.
- **Scripts' StartNewModule with waypoints mid-conversation**:
  `a_to_mandalore` (402 to 403), `a_alibi_end` (503 to 502 `From_Alibi1`),
  `a_alibi_end3` (502 to 512 `From_Alibi2`), `a_dhagon` (512 to 503
  `From_502OND_Dhagon`), `a_to_dxun`, `a_guide2_camp`.
- **Mandalore's test count**: c_pp_done reads seven journal entries
  (explosivesit, missingm, scouthunt, tsupport, tcannoks, battle_circle,
  zchallenge) by GetJournalEntry and needs more than three.
- **Remains with items**: the zakkeg's ear (`zakkeg_ear` in its
  inventory) and Bakkel's encrypted holodisks; both asked for by
  `c_hasitem`/`c_holo_check`. The 402DXN cannoks' random Phase Pulse
  parts are not needed.
- **Encounters spawned by scripts**: the sentries (TR_MEETMANDALORE,
  `sp_n_mandalorian*`), the detonator's beasts (`a_es_explode`,
  `wp_es_spawn_*`), Kelborn's walk spawning the second scouts
  (`a_kel_wpwalk`, `npc_2scout*`), the Sith assassins' stealth and
  unstealth in 403DXN (`a_sith_att`, `sith_ass_*`), Bakkel's spawn in
  503OND (k_503_enter on cantinarats).
- **Conversations across height**: Kumus on his ridge (24 m) above
  `tr_kumustalk` (18 m).
- **Force power granted by a conversation**: Beast Trick (`a_givepower`,
  `a_addforcepoint`) from `fake_kreia` when Kreia is not in the party.
- **Paying from a conversation**: 1B-8D's 25 credits (`c_chkcredits`,
  `a_alibi_s0d3` gives the droid's head).
- **The escape's state** (`a_endgame`, 502OND_End_First, escape_merchant):
  turret_iziz creatures and soldiers hostile in 502OND, Mandalore's
  end_mand in 501OND, the shuttle's start 0 opened by c_global_gt.
- **Companion lines gated on who is in the party** (`c_con_mirapm`,
  `c_con_baodurpm`, `c_mand_inparty`): Mira's at the Zhugs and the
  Rodians, Bao-Dur's at Kumus, Mandalore's throughout Iziz.
- Not needed by these routes, but on the way: the turret minigame
  (421DXN) and the swoop track (510OND) exist in the data; the `turrets`
  and `race` verbs could take them if the chain ever wants them.
