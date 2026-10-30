# The game's screens (`ui`, `game::ui_view`)

KOTOR's own `.gui` layouts, drawn by our GUI runtime over the live world.
- **`fmt_odyssey::gui`** reads a layout; **`asset_odyssey::font`** a TXI
  bitmap font. Child extents are relative to the root; the frame is
  640 × 480 unless the root reaches past it (`mainmenu16x12`, `mipc28x6`).
- **`ui`** (no Bevy, no sim): `paint` turns a layout plus each control's
  `ControlState` into a `DrawList` of quads; `raster` draws one on the CPU
  (probes, `kjk gui render`). `screen::Stack` holds open screens: focus by
  `MOVETO` then direction, the pointer, lists, edit boxes, the veil;
  screens raise `Intent`s. `screens/` has every screen; `vm::ViewModel` is
  all they read. `assets::GuiAssets` resolves layouts, textures, fonts,
  2DAs and the talk table.
- **`game::ui_view`** steps the stack each frame and draws its layers
  (and KOTOR's HUD under them in play) on the GPU with its own material
  (`ui_material`, `ui.wgsl`), a camera of its own. A layout is drawn
  as KOTOR draws its screens (reone's centred scaling mode): at its own
  size, a layout pixel a window point (two screen pixels on a Retina
  display), centred on whole pixels, and only scaled down in a window
  smaller than it (`draw::fit`); its fonts, borders and list rows keep
  the sizes the layout gives them. The HUD's frame is the window, its
  controls kept to their corners. The conversation's and a bark's frames
  are the window's shape at the layout's height, stretched to fill it
  (`draw::fill`, `DrawList::fill`; their sizes the owner's calls, below).
  Crisp: a texture is read Catmull-Rom where enlarged and from its
  mipmaps where shrunk, and a shape (a font, one-colour line art:
  `raster::Pixels::shape`) keeps its edge one screen pixel wide. `raster`
  samples and places the same way, so headless pictures match. Blended as KOTOR's,
  in gamma space: the screens draw into an image of their own, their
  art's bytes unconverted, and `ui_composite` lays it over the world,
  which keeps encoded values as well (`scene::encoded_view`):
  `screens + (1 − alpha) · world`, so translucent panels are as dark as
  the games' and `raster`'s. 3D views (`gui_scene`) draw into images that
  keep their encoded values, read like any other texture. Plugins fill the view-model: `character` (sheet, map, chargen's
  2DAs), `loot`, `merchant`, `script_screens`, `hud`, `settings`,
  `save_view` (the saves on disk, what a save now would be called);
  `session_flow` starts sessions from the menu, and loads saves.

The skin (`ui::skin`): a KOTOR 1 campaign's screens are KOTOR 2's layouts
and art, recoloured to KOTOR 1's colours ("blue steel": brass to blue
metal, KOTOR 2's greens and teals to KOTOR 1's blue, its palette onto
KOTOR 1's). KOTOR 2 is required: without it the screens are not drawn,
and the log says why. KOTOR 2 is read as installed, `override/` included
(restored content such as TSLRCM).

- A screen names its KOTOR 1 layout; `skin::moved` gives KOTOR 2's in its
  place: the same name with `_p`, but for the three KOTOR 2 named apart
  (`skin::RENAMED`: the main menu, the party selection, the advanced
  graphics options). Screens are written for KOTOR 2's controls. A layout of the
  campaign's own world stays the campaign's, drawn with its art
  (`skin::own`: the galaxy map, whose buttons are its planets). A layout drawn under
  a screen's (the bar) is read from where the screen's came from.
- A layout's own textures, fonts and words come from KOTOR 2; what a
  screen puts in (portraits, icons, item names, its words) from the
  campaign's.
- KOTOR 2's full screens are opaque 800 × 600 frames, black round them in
  a larger window. The in-game bar shows the party member the pages show:
  portrait, vitality and force, name, class and level; the other two
  members' portraits beside it choose theirs (the pages only: the sim
  moves one character a player, so a companion's equipment and levels
  wait). As in KOTOR 2, its fifth tab is the party and the messages open
  from the journal. The HUD shows the companions' portraits and bars,
  and KOTOR's stealth toggle (`TB_STEALTH`) lit while the character is in
  stealth mode (X; O lays a mine).
- In a session of more than one seat each player's portrait on the HUD
  carries a busy icon at its top right, half the portrait's width, for
  what that player is in (`Snapshot::busy`): asking the host in (`lbl_iquest`, with a "!" drawn as text on
  the host's screen), pazaak (`pcards_pos_x`), a store (`lbl_icredits`),
  a conversation (`gui_mp_talkd`), the first that holds in that order;
  none while the player is free (in none of these), watches or is down
  (`ui::screens::hud::busy_marks`; the dump's `hud.busy`). A guest's HUD
  lists only its own body and its followers, so the other players' icons
  show on the host's. A click on a companion's pazaak icon (while the
  pointer is free) watches its player's match (`hud::watch_at`,
  `Action::Watch`; R9a), where a click on the portrait beside it would
  make it lead; both name the member by its body (`MemberView::id`).
- The inventory's and the store's filters sort items by kind
  (`vm::ItemKind`, from the template). An item row shows its icon in a
  square beside its name, as in KOTOR 2.
- The main menu and loading screen show KOTOR 1's logos.
- 3D views (`game::gui_scene`): a screen names one per control
  (`ui::vm::SceneView`: the main menu's turning model, the character
  page's party member, class select's six figures, the upgrade bench's
  weapon); the game draws its models into an image the control's size
  on a layer of their own, and the control's quad shows it.
- The character page stands the member on the sheet's stage
  (`charrec_light`, as reone's character menu does: at the root, facing the
  stage's `camerahook`), its body and head but no weapons, both playing its
  side's clip (`good`, `neutral`, `evil`: light from 70, dark to 30). The
  camera is a perspective one at the hook (0.73 below the floor), through
  a window on the figure from z −0.04 to 1.85, measured off KOTOR's own
  sheet. The stage's lights, as the pose sets them, light the figure with
  no ambient (the key light's 50× strength kept to full, its hue kept: an
  assumption). A 3D view's image is drawn texel for texel (`ui.wgsl`).
  Behind it the stage's
  emitters (`game::emitter_view`, `asset_odyssey::emitter`) in the pose
  of its alignment: the model's clips `evil`, `align1`..`align19` and `good`,
  one each 5 points, set the emitters' birth rates and colours, from red
  fog and sparks through grey cloud to blue gas, light shafts and stars.
  `lighten` particles add by alpha; `normal` ones blend. The slider
  (`SLD_ALIGN`, tall) shows the same alignment, light at the top. Not yet:
  KOTOR 2's Mastery line (K1 has no mastery bonuses). `kjk play --alignment N` starts the
  player at N.
- Character creation shows the character being made (`SceneView::Made`,
  reone's character generation and portrait selection): the body its
  portrait gives its class, in plain clothes, turned to the camera, on
  `cgbody_light` seen from its `camerahook` 1.1 high each way (`maincg`'s
  `MODEL_LBL`), and its head on `cghead_light` from `camerahookm` or
  `camerahookf` 0.2 high, the body lowered by its own `camerahook`'s
  height (`portcust`'s `LBL_HEAD`); lit by the stage's lights.
- Feats and powers are picked at creation (`ftchrgen`) and at a level up
  (`ftchrgen`, `pwrlvlup`) from what the level may pick
  (`vm::PicksView`, the sim's `Classes::picks`). Feats are KOTOR's chart
  (`Classes::chart`): a row a chain, a feat needing no other
  (`prereqfeat1` blank), the feat needing only it and the one needing it
  and that one, joined by arrows, KOTOR 2's feat flow (`paint::flow`: 44
  pixels a row, `uibit_abi_back` 40 square tinted had / picked / open /
  red for not open, the icon 32 square over it, faint until had or
  picked, no arrow into one not open). The pointer chooses the feat it is
  over, the keys move along a row and between rows; the chosen one pulses
  and is named over its description (`LBL_NAME`). Add picks or drops the
  chosen feat (one the level offers), Recommended takes the first it may.
  Powers are a list, recommended first, a picked one framed (`lbl_hex_3`,
  the others `lbl_hex`), the rest faded once none are left. What is left
  unpicked is the game's (the host completes it). A level that offers none
  skips its step. Not yet: KOTOR's open feats counting this level's picks
  (Improved Flurry after Flurry picked at the same level), its separate
  bonus-feat pass, powers as a chart.
- A stack's count is drawn on its item's square as both games do
  (`fnt_d16x16`, the square's bottom right). The container lists alike
  stackable items as one stack (as KOTOR's containers merge them, up to
  `baseitems.2da`'s `stacking`), and taking it takes the whole stack.
- The party bar, the pages and the HUD show a player's name and portrait
  from its persona (the host's, for a guest).
- `kjk gui render k1 <layout> --skinned` draws what the game's screen
  draws. `kjk gui screen <game> <screen> --step "press TAG" --step "type
  TEXT"` opens a screen as play does on that game's own data (character
  creation's choices from its tables), takes the steps and draws its
  layers to one PNG (the 3D views stay blank): `kjk gui screen tsl
  chargen:solo --step "press BTN_SEL3" --step "press QUICK_CHAR_BTN"`.
- TSL's character creation (the same screen, its own layouts and tables):
  Jedi Consular, Sentinel and Guardian (`classes.2da` rows 4, 5, 3), the
  men's then the women's mirrored, taglines 48031 to 48033. A portrait's
  body by class, as TSL's picker takes it: a Guardian's (and K1's
  soldier's) `appearance_l`, a Consular's (K1's scoundrel's)
  `appearance_s`, any other's `appearancenumber`. Scores as K1's (30
  points from 8, 18 at most, a raise 1 to 3). Skill points are the
  class's and the INT modifier whole, not K1's half of them, at least 1,
  times 4 (TSL's tables halve `skillpointbase` instead:
  `Classes::whole_skill_points`). The first level's feat picks
  (`featgain.2da`) and granted feats as K1's; no powers: neither game's
  creation has a power step, and TSL saves its character knowing none
  (its Jedi first level would pick two). A new game sets `G_PC_LEVEL` to
  the level and `G_PC_Align_Val` to the alignment where the campaign
  declares them (TSL's globals, `SimWorld::create`). `kjk play --stage
  001ebo --made guardian:po_pmha01:NAME` (also `consular`, `sentinel`)
  starts TSL's prologue as such a character; `kjk sim probes --only
  creation` checks each class of both games against the tables. `kjk play`
  on a TSL module opens the screens on TSL's data (its tables, talk table
  and items: `runtime::open_screens`).
- The menu's New Game, both games installed, asks which story it begins
  (`screens::campaign`, `campaign:<then>`): the message box (`confirm`),
  "Which story does the new game begin?", its two buttons widened to the
  question, KOTOR and The Sith Lords [verify: the owner's look]. One game
  installed, it begins that one unasked. The game picked moves the
  screens to its data (`Intent::Campaign`, `runtime::set_campaign`,
  `GuiAssets::set_campaign`: tables, talk table, items, the campaign's art;
  the skin stays) and opens its character creation (`chargen:solo:tsl`,
  `chargen:host:<seats>:<port>:tsl`); the session intent carries it
  (`SessionIntent::Solo`, `Host` `campaign`; none, as a save's, is K1's),
  and the session starts in its first module (`new_game_module`: TSL's
  `001ebo`) as the character made, as `kjk play --made` does. Its creation
  backed out of puts the screens on K1's data again. Any session
  installed moves the screens to its module's game (`session_flow::
  install`): a TSL save loaded from the menu, a TSL host joined. The menu
  itself is K1's (TSL's when K1 is not installed).
- TSL's workbench (`workbench`, `screens::workbench`, the `component`
  layout; `docs/WORLD-STATE.md`, "Item creation"): opened by a script's
  `ShowUpgradeScreen` with item creation, directly when upgrading is
  disabled (the prologue's garage), else from the upgrade bench's Create
  Items. One list at a time: what the table lists for the maker by tab
  (Armor, Ranged, Melee, Lightsaber, Other; it opens on Other, as TSL's
  does), those its skill does not reach greyed; View Breakdown Items, what
  the party carries that the bench breaks down, all or by kind
  (`filter::BREAKDOWN`). The chosen item's description, the skill asked
  with the maker's total (with what it wears and what runs on it,
  `effects::sheet_now`), its price or what it yields, the party's
  components, how many it carries; Create Item makes or breaks down
  (`GameAction::Breakdown`, `Create`), the host's line telling a refusal,
  its sound (`gui_invadd`, `gui_invdrop`) the host's. Its close button
  reads Back where upgrading may be chosen, else Close. TSL's lab station
  (`chemical`, the same screen on the `chemical` layout,
  `ShowChemicalUpgradeScreen`): the tabs Health, Stimulants, Mines and
  Grenades (`chemicalcreate.2da` groups 0 to 3, as TSL's screen binds its
  buttons), opened on Health; its breakdown list unfiltered; chemicals for
  components. `kjk gui screen tsl workbench` (`chemical`) draws them for a
  stand-in maker; `kjk sim probes --only "tsl workbench"` opens the
  prologue's bench with its own script and makes the tutorial's repair
  kit; UI probe `bench_screens`. Item names leave out TSL's sort keys in
  braces (`{cr 02}Crystal, Rubat`, `fmt_odyssey::tlk::unbraced`, as
  conversations' lines do; UI probe `item_names_unbraced`). Not yet: the
  Accept button's text in breakdown mode, the executable's confirmations.

Screens: main menu (Solo, Host, Join, Options, Exit); new or load (a new
game or a save: its area, when, its module's loading image; alone, Delete);
save and load in game (from the options, as KOTOR's: a click points at a
save, a double click, Enter or Save acts on it; an old save's overwriting
is confirmed (`confirm`), then the name is typed in KOTOR's `savename` box,
the area's offered for a new save, the old one's over it; a save made says
"Game saved" at the right edge under the minimap, F5's too, and a refused
F5 says why there; loading during play, alone, loads in place behind the
loading screen as KOTOR does (the reload the party's fall makes,
`sim_host::ReloadRequest`); quitting to the main menu starts the program
again on the menu, the logos not played again: `--back-to-menu`); character creation (class, quick or custom,
portrait, 30-point abilities, skills, feats, name; the character made is
the first seat's player, `docs/WORLD-STATE.md`, "The player character");
loading (a session's start and
every module installed); message; options and
key mapping; in game the tabs (equip, inventory, character, abilities,
messages, journal, map, options), party, container, store, upgrade bench,
level-up, script message box, the fall, the galaxy map, the credits,
pazaak's wager, side deck and table. The upgrade bench lists what the
party carries and wears that takes upgrades (weapons by kind, body armour:
`ItemFacts::upgrades`, a lightsaber's colour crystal), each with its slots
(`UpgradeItem::slots`: a lightsaber's colour crystal, then one slot for
each kind it takes, two for its power crystals; the item's icon shows in
each). The bench works on a copy, as TSL's: a slot chosen lists an empty
row (taking out what is in it; not for a colour crystal, which stays
until another takes its place), what is in it, then what the party
carries that fits it and no other slot of the visit takes (`LB_ITEMS`,
from its top; one of an upgrade level above the item's greyed, refused
with the game's message box 116713); a row chosen changes the copy and
shows the slots again. Assemble sends the visit's changes together
(`GameAction::Assemble` → `Action::Assemble`, wire 41) and goes back to
the list; Back throws them away. UI probe `script_screens`. A script's
`ShowUpgradeScreen(item)` opens the bench on that item's slots. Both
screens hold their item by id (`upgrade:<id>`), as the list is rebuilt
from the goods each frame and another seat's trade may reorder it; an
item gone leaves the slots empty. The
recoloured saber is drawn by its own template (`weapons::sheet_models`:
the worn item's `ModelVariation`).

Party selection (`party`, KOTOR 2's `partyselect_p`, `game::script_screens`)
is KOTOR's over the party table the host tells (`vm::PartySelectionView`,
`docs/WORLD-STATE.md`, "The selection screen"): each available companion's
portrait on its button (`portraits.2da` by its template's `PortraitId`; K1's
nine slots, KOTOR 2's last three hidden), the others not available; it opens
with the party and the forced picked, framed in KOTOR's green. A button
chooses its companion (its name beneath the view); Add or Remove picks or
drops it (a forced one stays, two at most, greyed in play); the count is how
many may still be picked. A script's screen: Done sends the picks
(`ChooseParty`), Back or Escape the party unchanged. The screen opened in
play (P, the bar's party tab) asks the host for the table (`ShowParty`) and
only shows it. The chosen companion stands in the 3D view (`LBL_3D`) as KOTOR
shows the creature the table keeps (its `AVAILNPCn` state): its
`appearance.2da` body dressed by the item in its body slot, as the world
dresses a creature (`creature::resolve_appearance`) — what it wears now
(the table's `worn`), else what its template starts with. Not yet: its
level.

Every 3D creature view on the screens (both games' layouts have three:
the character sheet's `LBL_3DCHAR`, class selection's `3D_MODELn`, the
party selection's `LBL_3D`; no equipment, inventory, store or level-up
layout has one) dresses its body by that rule: the sheet shows the member's
body as the scene drew it, character creation the body in the clothes a
new character wears (`g_a_clothes01`).

The store (`store`, `game::merchant`) is KOTOR's: one list at a time,
the store's stock to buy or (Show Sell List) what the party carries to
sell, each at the store's price (`docs/WORLD-STATE.md`, Stores); the
chosen item's price, count (none for one that never runs out) and
description, and the party's credits. Accept, or a double click on a
row, buys or sells one (greyed when the store does not trade so); Close or
Back leaves the store. It opens and fills from the snapshot's goods
while this player trades with the store, and closes when the trade ends.
K1's executable's checks (its store screen's buy and sell handlers): an
item dearer than the credits is not bought, "You do not have enough
credits to purchase this item." (41950); one costing more than 50 credits
a level of the party's leader asks first, "This item costs more than
<CUSTOM0> credits. Do you still wish to buy?" (42020, `confirm`, OK and
Cancel); one worth more than that, at most 250, asks before it is sold
(41985). A purchase sounds `Inventory_Add`, a sale `Inventory_Drop`. UI
probes `store_double_click`, `script_screens`.

**Double clicks, one rule** (`Screen::primary`): a double click on a list's
row presses the screen's button for that row, the row chosen by its first
click, unless the button is greyed: the store's Buy/Sell, the inventory's
Use Item, the equip page's Equip (a worn slot's double click takes it
off), the saves' Save/Load and Start, the workbench's Create/Breakdown,
the upgrade bench's Upgrade Items. Lists whose click already acts (the
container's rows take, a conversation's or terminal's replies answer, the
key mapping's rows) and the feats and powers (`double` toggles) need none.
K1 itself acts on one click (its list rows are buttons, a click on one
buys, loads or takes); a click here points at a row and the double click
acts, the owner's rule (UI streamlining). The console's `ui double LIST
ROW` double-clicks a row (from 0) as the pointer does.

**Controls' sounds** (`ui::intent::GuiSound`, `game::ui_sounds`): K1's
controls are made with `guisounds.2da`'s `Clicked_Default` (`gui_click`)
for a click and `Entered_Default` (`gui_scroll`) for the focus coming to
them (a check box clicks `Checkbox_Check`); each list row is a button of
its own there. So here: a button's or a row's click, Enter, or the
console's press clicks; the focus moving to a control, or a list's cursor
to another row, enters (not a screen's first focus as it opens). The first
in-game screen over the world opened sounds `Fullscreen_Panel_Added`
(`gui_open`), the last closed `…_Removed` (`gui_close`); not the loading
screen (`Screen::quiet`). UI probe `control_sounds`.

The galaxy map (`galaxymap`, `game::script_screens`), which a script
shows (`ShowGalaxyMap`, `docs/WORLD-STATE.md`): KOTOR 1's own layout and
art, not KOTOR 2's, whose buttons are KOTOR 2's planets (`skin::OWN`).
The planets the scripts made available stand on their buttons
(`planetary.2da`'s `guitag`; the rest hidden), those not selectable
greyed. It opens on the planet the script named (where the party is):
chosen, its name and description (`planetary.2da`), and its model
turning in the 3D view (`planetary.2da`'s `model`, seen from its
`camerahook`, lit by its own aurora light). A selectable planet pressed is
chosen; Accept travels there (`GameAction::Travel` → `Action::Travel`);
Back closes the map. Every player sees it; only the host's Accept travels
(I30 P6: a guest's map has none, and closes when the module goes).

The credits (`credits`, KOTOR 2's `credits_p`, skinned), which a script
starts (`StartCreditSequence`: K1's ending): each `credits.2da` row's
block of names rises through the list on black (`ListView::roll`, 40
layout pixels a second [verify]), the music `credits` looping, then the
main menu; Back goes there at once. The game's end without its death
screen (`EndGame(FALSE)`) is the main menu at once, unless credits roll;
both wait for the movies before them. With its death screen the game is
lost: the fall's box (below).

Pazaak (`pazaak_wager`, `pazaak_setup`, `pazaak_game`;
`game::pazaak_view`, `docs/PAZAAK.md`) is KOTOR's three screens over
the match the snapshot carries (`sim::pazaak::Match`): the view opens the
one for the match's stage (`pazaakwager`, `pazaaksetup`, `pazaakgame`;
TSL's `_p` skinned) and closes them at its end; a script's `PlayPazaak`
notice opens nothing itself, as with the store. Each seat sees the match
it plays (`Snapshot::pazaak_of`, I30 P20), so two players can sit at two
tables at once; the others play on. A player watching another's match
(R9a, `Snapshot::watching`) sees its table read-only whatever its stage:
the player's hand as the player sees it, "Watching Carth" in place of the
player's name (shortened to fit, `ControlState::shorter`), a wager or a
side deck being chosen in words over the hand, a result in the third
person ("Carth won the set."), the play buttons faded and nothing sent,
Leave (its key named) on TSL's Forfeit button, and the Leave key ("Leave
a conversation", L) leaving too (`Action::Watch` with no player); no
tutorial pages. A paused match's screens and its tutorial pages close on
its player's screen and come back at its stage (`docs/PAZAAK.md`); a
watcher keeps the table, Leave live, and is told why in the same words'
place ("Carth is away.", "Carth is hurt.": R39). The cards are drawn in
the campaign's own art over the layout's (K1's `lbl_card*`: the main
deck's yellow, + blue, − red, ± either way as its sign is chosen), their
words on the label over each; a set won lights a score mark
(`lbl_winmark02`); the turn lights show whose turn. A set's and the
match's result (32334–32338) are the message box while the match holds
them; tutorial mode opens the tutorial's pages (38627–38649) as the side
deck and the table first show. The wager steps by one (Left, Right); the
side deck adds a card offered by a click (the count under it falls) and
takes a chosen one back; at the table a hand card plays with a click or
1–4, Flip (F1–F4) turns its sign first, End Turn E, Stand S; Back quits
before the match and does nothing at the table (K1 has no way out; TSL's
Forfeit does).

A cinematic conversation (`dialog`, `game::dialogue_view`) is
KOTOR's conversation screen, drawn under the screens and not one of them
(`docs/DIALOGUE.md`): the line in the bottom bar while it plays, in the
top bar with the replies below on the player's turn. A seat sees the
conversation its body stands in, else a party one; a watching seat the
one holding the member it follows (`dialogue_view::shown`). In a session
of more than one seat:

- **The dialog's Leave row** ("Leave (L)", naming the bound key; key
  mapping "Leave a conversation", L by default, the saber style key's
  default too: it leaves only in a conversation) stands under the replies
  while the player may leave (`Snapshot::may_leave`): in its own conversation or one it
  joined, and in a party one only as its listener (the host leading
  through a companion too: its character is that listener); faded while
  the conversation is locked (a game's cutscene too). A click on the row,
  or the key at any time in the conversation, sends `Pick::Leave`. One
  player alone has no row, as before, nor has a host whose guests only
  watch: the row shows while more than one seat plays a body, as the sim
  allows a Leave (`Snapshot::together`).
- **The story's replies** are drawn faded where the seat's player does
  not take them (`Snapshot::takes_story`: the host takes them, through
  whichever body it acts). A guest's click on one sends the reply where
  the sim asks the host in (`Snapshot::asks_host`: its own conversation,
  no player's character in it), else points at it.
- **The host's request card** (`ui::screens::dialog::Card`, its own
  layer over the conversation's screen and the HUD, under the screens):
  while a guest asks the host in (R5), each "Carth asks you to join:
  Garouk", then "Y: accept   N: end" in the highlight colour, the bound
  keys named (key mapping "Accept a join request" Y, "End a join
  request" N). It stands in the top bar's bottom right corner while the
  host walks, under the top bar while the host talks, never wider than
  half the replies, on a black backing. The keys answer the first
  guest asking (`Action::Request`) while no screen is open; while an
  exit's ready check is up, the host is down, the party fights, or the
  host stands in another locked conversation (the sim drops an answer
  then), they wait and the card says so ("Answer after the ready check",
  "Answer once you stand", "Answer after the fight", "Answer once your
  conversation ends"; `Snapshot::request_wait`): Y and N are also the
  ready and follow keys by default, so a ready check's Y readies and a
  watching guest's N follows; the host's Y and N answer a request only
  while its card is up and nothing keeps it waiting.

A computer's conversation (`computer`, `game::terminal`) is KOTOR's
computer screen over the world: the line in the upper screen, the replies
numbered below (each row as tall as its words, the pointer finding rows
so, the highlighted one kept in view), and what the scripts price with:
the first seat's Computer Use and Repair ranks with effects and the stack
of spikes and of parts they find. A row (pointer, Up and Down, Enter) or
the digit of an offered reply answers; with none offered, Enter ends the
line (`Intent::Answer`, into the command as the cinematic screen's; a
`--moves` run's `ui nav` answers too). In co-op the same Leave row stands
under the replies, and its key leaves without reaching play
(`Intent::Leave`; `swallow`). Each line starts its rows over. It
opens and closes with the conversation (the screens over it stay), steps
aside for a camera feed (a line with no text and a camera), and the
cinematic overlay keeps only the voice. A message longer than its box is
cut.

Real time: nothing pauses. The character idles under a screen and can be
hurt; death closes every screen.

Actions: a screen's `GameAction` becomes a `sim::action::Action` sent to
the host (reliable message 7) and applied in the step. What the host tells
one player is a `sim::notice::Notice`: words (message 10) or a screen a
script opened (`OpenStore`, `ShowLevelUpGUI`…, message 11).

Running and checking:

- `kjk` opens the main menu; `kjk play` starts in game. Esc, I, J, M, P, K
  open screens. `kjk play --gold N` starts the party with N credits,
  `--item TEMPLATE=N` with items, `--skill computer=8` sets a rank.
- Breaks and ready checks (`game::party_calls`, `PARTY.md`): Z calls
  or ends a break (during another's, it asks and is told it is not its
  own), Y is ready (the host's after 5 s: go now), the in-game
  menu's autopause button is Break/Resume, the banner (KOTOR's combat
  message) over everything; console `ui break|resume|ready|force`.
- The party's leader (`game::leader_view`, `WORLD-STATE.md`, "The
  leader"): Tab (KOTOR's `ChangeChar`; key mapping "Switch party leader")
  makes the next party member lead, and a click on a companion's portrait
  beside the HUD's own (while the pointer is free: in play the mouse looks)
  makes that one lead (`Action::Lead`, the host's only). The HUD, the bar
  and the pages list the party from the one leading (`cast::party_of`,
  turned as Tab turns it): its portrait in the HUD's big slot, the others
  beside it; the camera, the keys and the use key move it. Levels stay the
  player's own character's. Console `ui portrait 2|3` (a click on the
  second or third portrait); the dump's `lead`.
- Security (`game::lock_view`): U (key mapping "Security") tries the
  nearest locked door or placeable in reach in front of the player
  (`Action::Unlock`); the console's `ui security [spike]` does the same
  with a spike of that template the party carries. The target block
  offers Security too (below). The host's answer is the feedback log's,
  in the game's own lines. `kjk play --skill security=N` gives the
  players N ranks.
- Console verbs: `ui open <screen>`, `ui show <layout>`, `ui press <TAG>`,
  `ui nav up|down|left|right|activate`, `ui type <text>`, `ui back`,
  `ui close`, `ui security [spike]`, `ui use TAG open|close|talk|use` (a
  Use of the nearest object so tagged), `ui wear TEMPLATE` and `ui unwear
  TEMPLATE` (the player's character puts on one of the party's items of
  that template, in the first slot it fits, or takes it off; `ui wear
  TEMPLATE TAG` and `ui unwear TEMPLATE TAG` on the nearest character so
  tagged, a companion; `ui wear TEMPLATE left` into the player's own
  left weapon hand, a second pistol beside the first), `ui target TAG`
  (the target held on the nearest object so tagged or made from that
  template, e.g. `*bag2` for remains, as if the crosshair
  were on it; `ui target` lets it go; a walk there steers headless too), `ui act` (the
  use key on the target; `ui act N`: choice N first, from 0), `ui holster` (the
  holster key), `ui swap` (the weapon swap key), `ui form ROW` (TSL's Form of that `spells.2da` row in use,
  as the next-Form key picks one), `ui portrait 2|3` (a click on the HUD's second
  or third portrait), `ui watch 2|3` (a click on that portrait's pazaak
  icon), and `ui script upgrade|levelup|party|death|credits|end|box
  <strref>|movie <name>|galaxy <planet> [<available> <selectable>]` (`party` lists no
  companions: `ui open party` in play shows the host's table; rows by commas,
  `ui script galaxy 3 1,3,4,5,6,7,9 3,4,5,6,7`) (a store opens
  with a trade: `kjk play --run k_ptar_larrimsto` on `tar_m02aa`). In a
  conversation: `ui reply N` (the Nth reply offered, 1 to 30, as the
  screen picks it), `ui leave` (Leave; watching a pazaak match, its
  Leave), and on the host's screen `ui
  accept` and `ui end` (the request card's keys; they wait as the keys
  do), for scripted runs and `cargo xtask duo`.
- `kjk gui probes` runs the checks the approved test `gui_layouts` asserts.
  `kjk gui render k1 <layout>` draws one layout; `kjk gui sweep` paints
  every layout of each install; `kjk gui sheet` makes a contact sheet.
- Settings live in the user's config folder (`KJK_SETTINGS` overrides it).
- The options (`ui::screens::options`, a table per page): gameplay's
  inverted camera, the mouse's sensitivity and buttons, key mapping, the
  four volumes (`SOUND.md`), and the graphics (`game::display`):
  brightness (a gamma over the whole frame in `ui_composite`, a half to
  twice, 1 in the middle), the screen resolution (`optresolution`: windows
  of common sizes that fit the screen, largest first, borderless, and the
  screen's own fullscreen modes, except on macOS, where fullscreen is
  borderless; sizes in the screen's pixels; the window's present one
  chosen), and the advanced page's anti-aliasing (off, 2x, 4x, on every
  camera drawing to the window) and v-sync, with Cancel. Alt+Enter goes
  between a window and borderless. Applied at once and at the next start.
  The grass check box (`display.grass`, on) hides the area's grass without
  rebuilding it. Greyed: shadows, texture quality, anisotropy, frame buffer
  effects, soft shadows, the advanced sound page, the feedback list,
  difficulty, auto level-up, autosave and the minigames' axis. `dump`'s
  `display` shows the window.

The equip page: a slot chosen lists what the party carries that fits it;
Equip wears the chosen one on the member the bar's portraits bring up
(`GameAction::Equip { who, .. }` → `Action::EquipOn`) when the bar marks
it dressed (`MemberView::dresses`): the seat's own body, and for the host
the companions nobody plays. A guest's party starts with its body
(`cast::party_of`). TSL's second weapon set shows in both games (KOTOR's
own screen has one set: a remaster liberty, `WORLD-STATE.md`): its two
slots (`BTN_INV_WEAP_R2`, `…L2`; "Config 2" in the layout) list what the
first set's hands take and wear it in slots 18 and 19, and Switch Weapons
(`BTN_SWAPWEAPONS`) exchanges the sets of the member shown
(`GameAction::SwitchWeapons` → `Action::SwitchWeaponsOn`), greyed for one
this player does not dress. The HUD's swap button (`hud::swap_at`, a click
while the pointer is free) and the swap key (B; TSL's own is H, the
holster's here) swap the seat's body's sets (`Action::SwitchWeapons`,
`game::weapon_set_view`), a second apart at the least as TSL's client
keeps them. Level Up opens the level-up screen
for the member shown, in its class (a companion's from its template), and
Auto Level Up beside it takes that level with the game's picks at once
(`Classes::auto`, `CharacterView::auto_level`); both show for one this
player dresses that may take a level (`MemberView::may_level`). The
inventory page lists the party's items and credits (the snapshot's goods);
Use Item uses the chosen one on the member shown (`GameAction::UseItem` →
`Action::UseItem`, wire 25, its target the member itself), lit when that
member may use it on itself (`Owned::usable`: carried, worn in no slot, a
spell that is no hostile one, for its race: `sim::script::items::spell_for`,
the host's own rule) and this player dresses it; greyed otherwise (a
grenade, a droid's repair kit for a human). The host takes one use from the
party's goods and casts the item's spell at once (its impact script:
`k_sup_healing` heals the user; without one, the item's button's own
use), or tells why not ("That cannot be used now.").

An item's description, wherever a screen shows one (equip, inventory,
store, workbench, upgrade bench: `game::character::described`), is its
UTI's `DescIdentified`; in TSL, once Bao-Dur has been met (his influence
set: `Screens::bao_dur_met`, KJKS 65), one of the base items he cannot
equip (the Jedi robes, the armoured flight suit, the Zeison Sha armour,
and a sonic grenade, QUIRKS.md Q-0266) begins with "Cannot Equip:" and
"Bao-Dur" then a blank line, as TSL's executable writes it
(`sim::cannot_equip`). The executable's other lines there (what the item
asks of its wearer, its properties) are not written here.


The story's progress (I19, `docs/WORLD-STATE.md`) rides in the snapshot
(`sim::Progress`: the journal and the party's XP):

- the journal page lists the quests, the latest updated first, each by
  name with the text of the entry reached (`global.jrl`, from the level's
  library); a quest whose entry ends it counts 1;
- the character page shows the XP and what the next level asks, and Level
  Up while the character may take one: it opens the level-up screen, whose
  Finish sends its class (the one made in creation, else Soldier), ability
  and skill ranks, feats and powers to the host
  (`GameAction::LevelUp` → `sim::action::Action::LevelUpOn`); the host picks
  the feats and powers left unpicked, checks the level and takes it;
- the HUD shows the level-up mark (`LBL_LEVELUP<n>`) over each portrait
  whose member may take a level, and the menu bar's portraits show it too
  (`MemberView::may_level`, by the rule of `dresses`): as KOTOR 1 draws
  it, the campaign's yellow arrow (not the skin's
  recolouring) over, on the HUD, KOTOR's half-clear black backing
  (`LBL_LVLUPBG<n>`,
  taken from the campaign's own HUD layout where KOTOR 2's has none),
  its alpha pulsing 1 to
  0.4 and back, 0.75 s each way, as KOTOR's pulsing borders do
  (`hud::backed`, `ControlState::pulsing`, `Stack::pulsing`); the
  status icons (`LBL_JOURNAL`, `LBL_CASH`, `LBL_PLOTXP`, `LBL_ITEMRCVD`,
  `LBL_ITEMLOST`, `LBL_LIGHTSHIFT`, `LBL_DARKSHIFT`) for 4 s with the
  status summary that told them, and the feedback log the game's "Journal
  Updated:" with the quest's name and "Experience Points Gained: N"
  (`game::progress`, `loot::words`);
- a kill's XP floats over its dead as KOTOR's does: "XP 25" (the talk
  table's "XP" and the XP) in magenta (0.95, 0, 0.85), `fnt_d16x16`
  ringed in black, centred 32 pixels over its head's node and following
  it, fading out evenly over 3 s; a newer one over the same creature
  stands under the older. The sim keeps the party's kills of the last
  3 s in the snapshot (`Progress::kills`, every player sees them;
  `game::floaty_view`, `ui::screens::floaty`). A script's
  `DisplayFeedBackText` floats the same way for 5 s (`Progress::feedback`:
  its `feedbacktext.2da` line, "Resisted", "Immune", "Saved" over a Force
  power's target; "BAD STRREF" for a row without one), in white [verify
  KOTOR's colour].
- a script's message box of pages (`ui::screens::message::Pages`, the
  `confirm` layout, no cancel, OK turning the page): KOTOR's tutorial
  window (`ShowTutorialWindow`: the row's `Message_PC0` to `2`; its icon
  not drawn [verify]; off with the "Tutorial Popups" option,
  `feedback.tutorials`, on by default, kept in the settings file: the
  feedback page's list that holds it in KOTOR stays greyed here) and
  TSL's datapad (`DisplayDatapad`: the item's `DescIdentified`). KOTOR
  pauses under both; here the world goes on (a pause is a player's own).
  `ui script tutorial 51`, `ui script datapad qu_fakewill` open them.
- TSL's Forms on the HUD: a character who knows a Form has one more HUD
  action slot, after the power's and the items' (so it moves as they come
  and go; [verify] KOTOR 2 keeps its stance column fixed), the icon (`spells.2da`'s `iconresref`, `ifrm_*`) of the
  Form in use, else of the first it knows; the next-Form key (V, `Next
  Form` in the key map) puts the next Form it knows in use at once, no
  action and no cost, in a fight too (`Action::Form`, wire 47;
  `game::form_view`), told by the game's line for it ("{Shii-Cho}
  Determination Initiated" …, 124098 to 124108) in the feedback log
  (`sim::combat::forms`, `docs/COMBAT.md`). [verify] KOTOR 2 picks from
  the column's list with the mouse and shows that line 4 s on the HUD.
- KOTOR's status summary (`statussummary`, KOTOR 2's `statussummary_p`
  with the skin): what the scripts told the party (`docs/WORLD-STATE.md`'s
  table: journal entry added, XP and credits received or lost, light or
  dark side points, items received, named, and lost, KOTOR 2's influence
  and most Force points raised or lowered (`LBL_MAX_FP_GAINED`, `_LOST`:
  48610, 48611, each shown while what the statuses added to it is not 0);
  never what the party loots, trades or kills). As KOTOR's client, the statuses gather while a
  conversation, a fade or a screen is up, and the summary shows 0.25 s
  after none is: the layout's rows that apply, in its order, each as tall
  as its words (never less than its icon), the panel closing under the
  last. KOTOR pauses the game under it until OK; in real time it has no
  OK, stands at the window's right edge level with the minimap's bottom,
  slides in over 0.25 s, stays 6 s and fades over 0.6 s, a later one
  under it. Its words are in K1's own layout's blue, which the skin's
  recolouring would make yellow; K1's "Item(s) Received" has no place for
  the names, so they follow it as KOTOR 2's line has them
  (`game::progress`, `ui::screens::status_summary`).

## The minimap and the map page

KOTOR's minimap at the HUD's top left (`ui::screens::hud::minimap`,
`game::map_view`): the area's picture (`lbl_map<area>`, the campaign's
own) at its own size, moved so the player's place is at `LBL_MAPVIEW`'s
centre and clipped to it, under the skin's border (`LBL_MAPBORDER`, shown
only with a minimap); the skin's arrow (`LBL_ARROW`'s texture, at its
size) at the centre, turned the way the player looks. An area without a
map in its ARE, or whose picture the install lacks, has none, nor does a
map TSL's `DisableMap` jammed: the page shows the skin's `lbl_static`
then, nothing on it [verify] (`WORLD-STATE.md`, "The area's map").

The map page (K1's `map`, the skin's `map_p`; `ui::screens::map`,
filled while a page is open, `game::map_view::page`): in `LBL_Map` the
part of the picture the map uses, as large as fits in proportion, centred
on black (K1's 440 by 256 fills K1's own label; the skin's 588 by 294 has
black either side). On it:

- the rest of the party (other seats' players, companions) as
  `lbl_mapcircle`, the base colour;
- each note shown (below) as `whitetarget`, 16 px, the base colour; the
  chosen one half again as large, the highlight colour. `LBL_MapNote`
  holds its words; `BTN_UP` and `BTN_DOWN` choose the one before and after,
  round, greyed with fewer than two, and a click on a note's mark chooses
  it (`Screen::click`, a click on no control). Return to the Ebon Hawk
  stays greyed, labelled as `k_sup_guiopen` set it (`SetReturnStrref`, run
  on the host each time this player opens an in-game screen:
  `Action::ScreenOpened`); a click on the HUD's minimap opens the page
  (KOTOR's `BTN_MINIMAP`);
- the player's arrow where it stands (the skin's `mm_barrow_p`, else the
  campaign's `mm_barrow`; 32 px [verify]), turned as the minimap's.

Screens draw quads of their own over their layout (`Screen::quads`,
`Page::quads`): the page's picture and all that lies on it.

- **The map** (`asset_odyssey::map::AreaMap`, the ARE's `Map`): two world
  points and where they fall on the picture fix a scale for each axis;
  with north along world Y (`NorthAxis` 0, 1) world X runs across the
  picture, along X (2, 3) world Y does. KOTOR 1's pictures use the left
  440 of their 512 columns (its map page is 440 wide), KOTOR 2's all.
- **The arrow** turns clockwise by the north's yaw less the player's
  (`AreaMap::arrow`; norths +Y, −Y, +X, −X). A quad turns about its
  centre after its clip (`draw::Quad::spin`, on the GPU and in `raster`).
- **Notes** are waypoints' (`HasMapNote`, `MapNote`, `MapNoteEnabled`;
  `Instance::map_note`, `sim`'s `ObjectData::map_note`): shown as placed,
  or as a script's `SetMapPinEnabled` pinned them (`ObjectState::map_note`,
  `docs/WORLD-STATE.md`, "The area's map"), only where the party explored
  (K1's exe makes a note's button only on an explored point).
- **Fog of war**: what the party has not explored (the snapshot's grid for
  the area, `sim::fog`, the same for every seat) is dark over the
  picture's used part, on the minimap and the page alike, as K1's exe
  draws it (`CSWGuiMapHider`): over each unexplored cell the campaign's
  `blackdot` (a soft dark navy dot), four cells wide and centred on it; the
  dots overlap into dark and spill a cell or two over explored ground, so
  the edge fades rather than stepping. Nothing fades in time: a cell is
  explored or not (`ui::screens::map::fog`).
- The view is black under the picture, so beyond its edge nothing of the
  world shows (KOTOR's pictures are black round the plan; reone leaves it
  clear) [verify against the game].

```bash
kjk play --stage tar_m02aa --arrive wp_01_03 --headless \
  --moves '1s; 1s yaw 0; 1s yaw 90; 4s w yaw 270; 10s' \
  --cmds 'wait world; anim 1.8; wait 20f; screenshot; dump; quit'
# The page, the next note chosen, after walking some of the ring
kjk play --stage tar_m02aa --arrive wp_01_03 --headless --moves '1s; 4s w; 3s' \
  --cmds 'wait world; wait 6s; ui open map; ui press BTN_DOWN; wait 300ms; screenshot map; quit'
```

`dump` shows `minimap`: the picture, the player's place on it, the turn.

## Targeting and use

KOTOR's click on a door, container or creature, made for the JA camera:

- **The target** (`sim::target`, render side, `game::target_view`):
  what the crosshair (a dot at the screen's middle) rests on within 10 m
  of the player and in sight, else the nearest within 4° of it and in
  sight, else the nearest within 2.5 m in front and in sight of the
  player's eyes (what lies at the character's feet). It holds 0.2 s.
  Pick volumes are cylinders (a creature 0.4 m by 1.9, a placeable 0.5 by
  1.5, a shut door 1 by 2.5; an open door none, its brushes only). Targetable: doors but static ones, useable placeables,
  living creatures, and a downed party member (a seat's avatar, "Player
  N", or a companion, driven or not; lying, 0.8 m by 0.5). A living
  companion is a target whoever drives it, its AI or another player
  (owner, 2026-10-30: as KOTOR's party members are clicked and talked to),
  but only clear ahead of the player, more than its own radius in front
  (it follows at the heels and the party's bodies pass through each
  other: one there is not picked by a look past it); never the body the
  player itself plays; seats' avatars standing are not (no conversation).
  Its Dialog runs its dialogue hook as any creature's (K1's
  `k_hen_dialogue01` through `k_ai_master`, TSL's `k_hen_kreiadlg`, …),
  which begins its own conversation (`k_hcar_dialog`, `kreia`, …).
- **The block** (`ui::screens::hud`, K2's `mipc28x6_p`): fixed at the top
  centre, the target's name, a creature's vitality, up to three of what
  may be done to it with KOTOR's icons (`i_opendoor`, `i_openplace`,
  `i_useplace`, `i_dialog`, `isk_security`), the chosen lit and spelt out
  beneath (Open 365, Use 366, Dialog 371, Security 255). The mouse wheel
  chooses. A locked door or container: Open (its key, else its
  fail-to-open script and "Locked"), Security, Security with the best
  spike carried (owner, 2026-10-09: Open is the default; Security only
  for a body with ranks in it and not on a lock only its key opens, as
  K1); an open door: Use (its `OnClick`), and before it Close (1582, the
  open-door icon) where the player could open it again (not locked, not
  one only scripts open, not broken open, not leading out; nobody in its
  doorway, the host checks; its `OnClosed` runs). KOTOR offers nothing on
  an open door: Close is the owner's (QUIRKS.md Q-0330); a
  creature not hostile that speaks: Dialog; a hostile one shows its name
  and vitality, both red, and the items the player's body wears that it
  may use on a foe with a use left (a droid's stun ray or flamethrower:
  `Choice::Item`, the item's icon and name; E walks into the spell's
  reach and sends `Action::UseItem` with the foe); a downed party member:
  Revive (`isk_treatinj`, Treat Injury 257). `kjk play --class droid`
  plays the dev soldier as a droid.
- **E** does the chosen: at once within four fifths of its reach (doors
  2 m, Dialog 4 m, else 2.5 m; the host checks the whole reach), else the
  player walks there first. A door with action points (the two use
  positions of its closed walkmesh, `<model>0.dwk`: one before it, one
  behind) is done from the one on the player's side, a walkable one first,
  within four fifths of 0.75 m past a humanoid's personal space
  (`sim::target::Spot`; K1's and TSL's `CSWSCreature::GetUseRange`), so
  the player never walks into a wide door's face, and the host also takes
  a door's use from there. The walk (`sim::target::Walk`): a walker, or
  straight at it where no route leads, as before a door that leads out;
  steered in place of the keys, keeping its hand and power;
  its own move, jump or attack ends the walk, as do a screen, a
  conversation, the target going or no longer offering the choice (a door
  opened, a creature turned hostile). 10 s of the sim's time or three
  stalls: "out of reach". The `USE` bit is scripted `--moves … use`'s.
- **Revive** (`sim::target::Reviving`): there (within 2 m, else after the
  walk) the player asks for that body (`Action::Revive`, wire 23), turns
  to it and holds use (`USE_HELD`) with no key down until it stands
  (`docs/FALLING.md`). Its own move, jump or any
  action ends the hold (E again changes nothing, as during a walk), as do
  the body standing or going, and the host's revive breaking off (a hit,
  the body no longer ahead) or not beginning within 1 s (another player
  revives it). The block keeps the body's name and Revive lit (busy, as
  during a walk); its bar is the revive's progress, "Reviving" beneath.
  The downed player's own block names its reviver over the same bar.
- **The host** (`sim::world::uses`): a seat's `Action::Use { object,
  verb }` (wire 16) is checked (a verb the object takes, within reach, in
  sight) and does what the use key does; the games' scripts see their
  usual events (`OnOpen`, `OnUsed`, `ScriptDialogue`, `OnClick`, …).

Approved tests: `use_actions`, `use_scripts`, `use_walk`; the block's
checks are in `gui_layouts`.

### Area transitions

KOTOR names the area a door or exit leads to while the player faces it
(survey: `context/artifacts/2026-10-30-transition-label/SURVEY.md`):

- **Which** (`sim::transition::faced`, render side, `game::target_view`):
  each door and trigger that leads to a module (`ObjectData::exit`) is
  met by the line the player faces, across the floor, within its reach
  (`targeting.transition.reach`): a door by its pick volume, standing
  ahead; a trigger by its outline, where the line first crosses it. It
  must be in sight of the player's eyes; the nearest wins, asked again
  every frame (nothing holds). Only while the HUD is up and no screen.
- **What**: its `TransitionDestin`, cut as KOTOR's client cuts it: the
  part after the first dash and the character after it ("Taris - Upper
  City South" shows "Upper City South"); none without a name.
- **How** (`ui::screens::transition`): KOTOR's panel (K1 `areatransition`,
  TSL `areatrans_p`, the skin's): the strip, the name on it in the
  campaign layout's colour (K1's blue), the running-man icon beneath,
  centred under the target block and its words (KOTOR's stands at the
  same place across the top). The dumps carry it (`transition`).

## Barks

A one-liner or a script's `BarkString` or queued `ActionBarkString`
(`DIALOGUE.md`; neither barks while a conversation holds the world, as
both games' executables drop them), from the snapshot's barks
(`game::bark_view`, `ui::screens::bark`):

- **The bubble** is KOTOR's `barkbubble` (K2's `barkbubble_p`, skinned),
  over the HUD and under the screens, the latest bark alone, for as long as
  the sim keeps it; gone in a conversation, shown in a minigame. The panel
  grows to the words: the label as tall as its lines, the label's left
  again above and below (reone's reading). Drawn at 0.7 of a layout
  stretched to the window's height (`bark::SCALE`, the owner's call when
  the HUD was drawn so: at its size the bubble was too big), its
  words in the colour the campaign's own layout gives them (K1's blue),
  not the skin's (KOTOR 2's pale colour would be K1's yellow).
- **Where**: over the speaker while it is drawn and in front of the
  camera, the panel's bottom centre 0.3 m over its head's node (2 m over
  where it stands without one), kept inside the window; else where the
  layout puts it, as far from the middle as there (the Ebon Hawk's
  turret: `BarkString(OBJECT_INVALID, …)`; a minigame hides everyone).
  The real game's placement over a speaker is [verify]: reone draws the
  bubble at its layout's place only.
- **Its voice** (`SOUND.md`, "What plays when").

```bash
kjk play --stage m12ab --global K_KOTOR_MASTER=10 --global K_TURRET_SKYBOX=10 \
  --headless --moves 30s --cmds 'wait 1s; clip bark 6 12; quit'
kjk play --stage tar_m02aa --arrive wp_01_03 --aim Ithorian --talk Ithorian \
  --headless --moves 20s --cmds 'wait 300ms; screenshot; quit'
```

`dump` shows `bark`: its words, the point over its speaker, its voice.

Saving and loading: saves live in
`saves/` beside the settings (`KJK_SAVES` names another folder); F5 quick
saves, F9 quick loads (both rebindable); a save is refused in a
conversation, in combat (`Snapshot::in_combat`), during a break or a ready
check, while an area loads, during a movie and while down, saying why
(`saves::refusal`); another player's conversation or pazaak match is told
by its body's name ("Carth is in a conversation.", "Carth is playing
pazaak."; a conversation the first guest's body in seat order stands in),
the saver's own, a party conversation, the host's and two others' in
KOTOR's words. A
load opens no container's screen: a container a seat held open when the
save was made (a guest's, a quick save while looting) loads closed and
held by no one (`sim::containers::let_go`), as KOTOR's loot screen opens
only by looting. `kjk --load <id>` opens on a save; the console's `ui
save [name]`, `ui quicksave` and `ui load <id>` do what the screens do.
A save hosted holds the seats chosen (its guests take its companions,
I31; `kjk host --load <id>`). A guest keeps a copy of every save its host
makes (I30 P17), listed as
"<name> (from <host>)"; another build's copy is refused, with why.

The fall (I30 P11–P13, `docs/FALLING.md`): a revive shows in the target
block (`TargetView::progress`, above); the whole party down holds the
lost box (`lost`, on `confirm`) open on every seat, from the snapshot's
`lost` (`ui_view::fall`): the host's Load raises `Intent::ReloadLatest`
(the host's own latest save, never a copy, loaded in place for everyone;
`ui reload`; one that does not read is told over the box), a guest's box
has no button.

Waiting: screenshots in the save list.
