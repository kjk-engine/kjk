# The game's screens (`ui`, `game::ui_view`)

KOTOR's own `.gui` layouts, drawn by our GUI runtime over the live world.
[`UI-GUIDELINES.md`](UI-GUIDELINES.md) holds the principles and the numbered rules every screen is held to.
- **`fmt_odyssey::gui`** reads a layout; **`asset_odyssey::font`** a TXI
  bitmap font. Child extents are relative to the root; the frame is
  640 × 480 unless the root reaches past it (`mainmenu16x12`, `mipc28x6`).
- **`ui`** (no Bevy, no sim): `paint` turns a layout plus each control's
  `ControlState` into a `DrawList` of quads; `raster` draws one on the CPU
  (probes, `kjk gui render`). `screen::Stack` holds open screens: focus by
  `MOVETO` then direction, the pointer, lists, edit boxes, the veil;
  screens raise `Intent`s. A layout drawn under a screen (the in-game
  menu bar, `Screen::under`) has its controls renumbered past the
  screen's, so that the screen's `MOVETO` links never lead to the bar's
  tabs. `screens/` has every screen; `vm::ViewModel` is
  all they read. `assets::GuiAssets` resolves layouts, textures, fonts,
  2DAs and the talk table.
- **`game::ui_view`** steps the stack each frame and draws its layers
  (and KOTOR's HUD under them in play) on the GPU with its own material
  (`ui_material`, `ui.wgsl`), a camera of its own. A layout is drawn
  as KOTOR draws its screens (reone's centred scaling mode): at its own
  size, a layout pixel a window point (two screen pixels on a Retina
  display), centred on whole pixels, and only scaled down in a window
  smaller than it (`draw::fit`); its fonts, borders and list rows keep
  the sizes the layout gives them. Where the window draws a frame
  larger (`draw::scale`), the menus' and pages' words stay at their
  fonts' own size, a font pixel a window pixel (`Stack::step`,
  `Sizing::text`, `Font::scaled`), as KOTOR 2 draws them at
  1280 × 1024: the owner's call (2026-11-09, "a bit more elegant"). The
  conversation, barks and the HUD keep the frame's scale. The HUD's frame is the window, and
  its controls are placed as KOTOR 2 places them (`ui::screens::hud::place`). The layout is
  stretched to the window. Then Aspyr's rule narrows each control towards the side of the
  window it stands on, by the window's shape over 4:3, so that each piece keeps its shape.
  The combat queue and the action description narrow towards the middle instead. By default
  kjk keeps the target block at the top centre, with the area name under it, as suits its
  real-time play (the Interface option "Target block at the top centre"; off, the block
  floats over the target as KOTOR 2's does). With "Maintain image aspect ratio" on, a picture of
  the game's own HUD that the rule would widen keeps its shape too. The conversation's and a bark's frames
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
  (`skin::own`). The galaxy map is one, because its buttons are its
  planets. The loading screen is the other, because KOTOR 1's pictures have
  a slot for its bar. A layout drawn under
  a screen's (the bar) is read from where the screen's came from.
- A layout from an override folder (the game's own or a Workshop item's)
  is a mod's, unless it is one of the files Aspyr's update ships there
  (`vfs::resolver::STOCK_OVERRIDE`, by name and size). Aspyr's five
  layouts must match by their bytes too (`STOCK_LAYOUTS`), because a mod
  that only changes numbers keeps a layout's size. A mod's layout is
  stretched plainly, as KOTOR 2 stretches it, without kjk's keeping of
  pictures in shape: widescreen mods narrow their pictures for that
  stretch. The main menu adds Aspyr's build notice to a layout without
  one, so the build shows.
- A layout's own textures, fonts and words come from KOTOR 2; what a
  screen puts in (portraits, icons, item names, its words) from the
  campaign's. An item template only a module's own archives hold (a story
  item, Tatooine's star map datapad) is read from that module wherever
  the item is carried, as KOTOR's save keeps the item whole
  (`GuiAssets::item`; issue 165).
- KOTOR 2's full screens are opaque 800 × 600 frames, black round them in
  a larger window. The in-game bar shows the party member the pages show:
  portrait, vitality and force, name, and each class it has with its
  levels in that class (a Soldier who became a Jedi Guardian shows both
  lines, the second from the moment `AddMultiClass` gives the class); the
  other two members' portraits beside it choose theirs (the pages only: the sim
  moves one character a player, so a companion's equipment and levels
  wait). As in KOTOR 2, its fifth tab is the party and the messages open
  from the journal. The HUD shows the companions' portraits and bars,
  and KOTOR's stealth toggle (`TB_STEALTH`) lit while the character is in
  stealth mode (X). Mines are laid from the action bar or the power wheel.
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
  `Action::Watch`), where a click on the portrait beside it would
  make it lead; both name the member by its body (`MemberView::id`).
- The inventory's and the store's filters sort items by kind
  (`vm::ItemKind`, from the template). An item row shows its icon in a
  square beside its name, as in KOTOR 2.
- The main menu shows KOTOR 1's logo (KOTOR 2's own in a KOTOR 2
  campaign). The loading screen is the campaign's own layout, so it shows
  the campaign's own logo.
- The main menu is in the game played last (`game::menu_figure`): the
  newer of the newest save (one's own or a copy of a host's) and the note
  every machine keeps beside the saves of the module it last played and
  how its game was won (`played.json`, `saves::Played`), so a guest who
  never saves is kept up to date too. A KOTOR 1 game's menu is in KOTOR
  1's colours with Malak (`mainmenu`); a KOTOR 2 game's in its own with
  the story's Sith Lord, as KOTOR 2 picks him from its global
  `GBL_MAIN_SITH_LORD` (the newest save's: its story, else its world):
  Sion (`mainmenu01`; its prologue sets 0), Nihilus (`mainmenu03`;
  meeting Visas sets 2), Traya (`mainmenu02`) once on Malachor V, and
  after a game won the player's own character standing in the dark
  side's stance on the empty stage (`mainmenu05`) after a dark-side end,
  Kreia (`mainmenu04`) after a light-side one. One showing
  of Nihilus in eight has the Twi'lek dancers KOTOR 2 ships and never
  shows (`mainmenu06`). The model's smoke and mist play and its lights
  light it, its ambient-only lights as the view's ambient; seen from its
  `camerahook` through the 3D screens' lens (`STAGE_FOV` top to bottom)
  at the layout's scale, its view reaching the window's sides
  (`ControlState::wide`). `KJK_MENU_FIGURE=<model>`
  stands a model there whatever the saves say.
- 3D views (`game::gui_scene`): a screen names one per control
  (`ui::vm::SceneView`: the main menu's turning model, the character
  page's party member, class select's six figures, the upgrade bench's
  weapon); the game draws its models into an image the control's size
  on a layer of their own, and the control's quad shows it.
- The character page stands the member on the sheet's stage
  (`charrec_light`, as reone's character menu does: at the root, facing the
  stage's `camerahook`), its body and head but no weapons, both playing its
  side's clip (`good`, `neutral`, `evil`: light from 70, dark to 30). The
  camera is K1's executable's: level at the stage's hook, raised by the
  figure's own `camerahook` (a human's 1.63 m, so the view's middle is at
  0.9 m), seeing 22.726° top to bottom; T3-M4, HK-47 and Zaalbar have hooks
  of their own on the stage (`camerahookt`, `camerahookh`, `camerahookz`),
  picked by tag. The stage's lights, as the pose sets them, light the figure with
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
- A character's face, bare body and portrait darken as it falls to the
  dark side, in the steps both executables use
  (`game::alignment_look`, `rules::combat::sheet::DARK_LOOKS`). Its head
  takes the next `heads.2da` column (`headtexe` to `headtexvvve`) at an
  alignment of 40, 30, 20 and 10, and its portrait the next
  `portraits.2da` column (`baseresrefe` to `baseresrefvvve`). Without
  armour its body takes `appearance.2da`'s `texaevil` at 30 in KOTOR and
  at 40 in KOTOR 2. KOTOR 2 also reads `texbevil`, `texievil` and
  `texlevil` for armoured bodies of those letters, and `texmevil` for a
  body of letter `n`, a column its table does not have. KOTOR 2 draws a
  head with no dark texture in its `alttexture`. A character whose step
  changes is dressed again, as when its armour changes (`made::guise`,
  `wardrobe`). The HUD, the character sheet and the party selection show
  the portrait of its step.
- Character creation shows the character being made (`SceneView::Made`,
  reone's character generation and portrait selection): the body its
  portrait gives its class, in plain clothes, turned to the camera, on
  `cgbody_light` seen from its `camerahook` 1.1 high each way (`maincg`'s
  `MODEL_LBL`), and its head on `cghead_light` from `camerahookm` or
  `camerahookf` 0.2 high, the body lowered by its own `camerahook`'s
  height (`portcust`'s `LBL_HEAD`); lit by the stage's lights.
- Recommended works at a level up as at creation, for the player and for
  every companion, from the class of the level being taken. On the skills
  page it starts over from the character's own ranks and buys ranks in the
  class's recommended order (`skills.2da`'s `<table>_reco`), each as high
  as the points go before the next. On the abilities page it puts the
  level's point into the class's primary ability (`classes.2da`'s
  `primaryabil`). On the feats and powers pages it picks as at creation.
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
  Powers are KOTOR's chart drawn the same way (`pwrlvlup`,
  `Classes::power_chart`): a row a chain over the powers the class may
  ever pick and those known, a power needing none, the power needing only
  it and the one needing it and that one (`spells.2da` `prerequisites`,
  `23_27`: Push, Whirlwind, Wave); a power needing one with no row of its
  own (TSL's Crush Opposition IV) begins a row. The chosen one is named
  (`LBL_POWER`) over its description; Select picks or drops it. What is
  left unpicked is the game's (the host completes it). A level that offers
  none skips its step. Not yet: KOTOR's open feats counting this level's
  picks (Improved Flurry after Flurry picked at the same level), its
  separate bonus-feat pass.
- A stack's count is drawn on its item's square as both games do
  (`fnt_d16x16`, the square's bottom right). The container lists alike
  stackable items as one stack (as KOTOR's containers merge them, up to
  `baseitems.2da`'s `stacking`). Both games take or give a whole stack
  at once. kjk asks how many first, in its own number box
  (`screens::container::HowMany`, drawn on KOTOR 2's pazaak wager box):
  activating a stack of more than one, in the contents or in the give
  mode, opens it. Less and more (or Left and Right) change the number by
  one. When taking, the number starts at the whole stack, so OK takes what
  the original game takes. When giving, it starts at one. OK moves that
  many (`Action::Take` and `Action::Put` with a count, the stack split
  where only part of it moves), and Cancel or Back moves nothing.
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
  "Select game", its two buttons widened to the
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
  conversations' lines do; UI probe `item_names_unbraced`), and so do
  objects' names wherever a screen shows one (the target label, a
  speaker, a container, the message log; `ui::strings::name`): a
  designer's note such as TSL's "Lift Controls{to 002EBO}" stays in
  `GetName`, as TSL's Mac executable strips it only as its screens set
  the text (the target menu's name label among them). While breaking
  down, the title, Accept, the button to the other list and the price's
  label read "Breakdown Inventory Items", "Breakdown Item", "View
  Createable Items" and "Component Value" ("Chemical Value" at the lab
  station), as TSL's executable names them for its breakdown list. Not
  yet: the executable's confirmations.
- Labels whose words KOTOR 2's own code sets, in place of the layout's
  stand-in words, are given as KOTOR 2's lines (`tab::skin_line`), read
  from KOTOR 2's talk table whatever the campaign. The campaign's talk
  table may hold another line at the same number: K1's line 32132 is
  "Buy", KOTOR 2's "Buying Items". They are the skills page's row names,
  the store's and the workbench's labels for each list, the level-up
  title, the party screen's Add and Remove, the container's give mode
  and the equip page's slot name.

The co-op lobby (`screens::lobby`) is kjk's own screen, and a deliberate
deviation from KOTOR 2, which has no lobby. It opens after the host picks a
save on the Host screen, and when a guest joins. It is made from KOTOR 2's
party selection layout (`partyselect_p`), whose pieces are moved and copied:
the Players and Game panels and the description strip are copies of the
panel behind the companion's name, the player portraits are copies of the
companion portraits, and the saber rules, collision and "Show chosen
replies" rows are copies of the Add button. The companions keep the layout's own portraits, set in one
row with the main character. The row holds up to twelve companions and the
main character. Every piece is KOTOR 2's own art. A companion the story
took away shows the layout's "no companion" art in place of the portrait,
as KOTOR 2's party selection shows one who is not available. The player
slots are the screen's main part, with larger portraits than the
companions' row. The pointer over a control selects it, and
the description strip says what choosing it does. The keyboard and the pad
reach every control. Back leaves the lobby, as Leave does.

Screens: main menu (Solo, Host, Join, Options, Exit); new or load (a new
game or a save: its area, when, its module's loading image; alone, Delete);
save and load in game (from the options, as KOTOR's: a click points at a
save, a double click, Enter or Save acts on it; an old save's overwriting
is confirmed (`confirm`), then the name is typed in KOTOR's `savename` box,
the area's offered for a new save, the old one's over it; a save made says
"Game saved" at the right edge under the menu buttons, F5's too, and a refused
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
until another takes its place), what is in it, the part it held when the
bench opened if the visit took that part out (KOTOR 2's executable puts a
part taken out back in the party's inventory at once, so the list offers
it again), then what the party
carries that fits it and no other slot of the visit takes (`LB_ITEMS`,
from its top; one of an upgrade level above the item's greyed, refused
with the game's message box 116713); a row chosen changes the copy and
shows the slots again. Assemble sends the visit's changes together
(`GameAction::Assemble` → `Action::Assemble`, wire 41) and goes back to
the list; Back throws them away. UI probe `script_screens`.

What the bench tells follows KOTOR 2's executable. Its description box
shows the copy's description: the "Cannot Equip" lines, the item's stats
with the visit's changes, then its text ("No Description Set", strref
32172, when it has none). So a part put in shows its effect before
Assemble. KOTOR 1's parts bring the item's own stats of their
`upgrade.2da` row; TSL's parts add their own stats. While a slot's list
shows, the row the pointer or the keys are on describes its part instead,
and the empty row describes nothing. The slot the focus last came to is
named under the slots (`LBL_SLOTNAME`): by the part in it, else by the
slot's kind. Beneath it (`LBL_PROPERTY`) are that part's stats. An empty
slot of a weapon or armour shows there the item's stats that a part would
bring. An empty slot shows the picture of its kind. The slot names and
pictures are each executable's own table (`character::TSL_SLOTS`,
`K1_SLOTS`), with each game's strrefs. The item list's description tells
of the row the pointer or the keys are on. The bench's descriptions leave
out the rulebook's paragraphs, as KOTOR 2's show only the item's stats and
text. The bench's sounds are the controls' own. The back key clicks
before it closes a list or the screen, and neither bench screen is heard
opening or closing, as KOTOR 2's upgrade screens have no sound of their
own. A colour crystal chosen on a visit changes the saber's template, and
its description shows the change only after Assemble. UI probe
`upgrade_tells`. A script's
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
nine slots, KOTOR 2's twelve), the others not available. As KOTOR's
executable sets it up, a screen whose script forces companions in opens
with those alone picked, and any other opens with the party as it stands,
framed in KOTOR's green. A button chooses its companion (its name beneath
the view); Add or Remove picks or drops it, two at most. Add or Remove on
a companion that may not move opens a message box in the game's words: a
forced one must stay, and one the player may not pick can neither join
nor leave. The count is how many may still be picked. A script's screen:
Done sends the picks (`ChooseParty`). Back or Escape backs out with the
party unchanged when the script allowed it (TSL's `nAllowCancel`), and
otherwise a message box says the party must be accepted. The screen opened
in play (P, the bar's party tab) asks the host for the table (`ShowParty`).
In both games it changes the party (Done sends the picks, Back keeps the
party). Where the game refuses, Add or Remove shows the game's own line
for the reason. KOTOR 2 refuses in an unescapable area, in combat, and
while someone is unconscious. KOTOR 1 refuses in an unescapable area,
while a party member stands far from the leader, and while an enemy sees
one of the party, as its map page's Party Selection button does. For a
co-op guest the screen only shows the table and Add is greyed. The chosen companion stands in the 3D view (`LBL_3D`) as KOTOR
shows the creature the table keeps (its `AVAILNPCn` state): its
`appearance.2da` body dressed by the item in its body slot, as the world
dresses a creature (`creature::resolve_appearance`) — what it wears now
(the table's `worn`), else what its template starts with. Not yet: its
level. KOTOR 2's last two slots stand in for two others, as its executable
has them: Hanharr's (10) takes the place of Mira's (7), and the Disciple's
(11) the place of the Handmaiden's (4). Only one slot of each pair shows:
the stand-in's while that companion is available, the other one otherwise.

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
Back leaves the store. Its title, Accept, the button to the other list
and the price's label read "Buying Items", "Buy Item", "Show Sell List"
and "Item Cost" while buying, and "Selling Items", "Sell Item", "Show Buy
List" and "Item Value" while selling, as TSL's executable names them. It opens and fills from the snapshot's goods
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

**Check boxes and the options' descriptions** (`paint::check_mark`,
`screens::options::Help`): a check box's mark (`uibit_checkbxon`, a pill
on a square) is drawn at the row's left, half the row tall and three
quarters as wide, in the row's words' colour, as KOTOR 2 at 1280 × 1024
draws it (`context/artifacts/2026-11-09-k2-menu-refs/`); stretched over
the row it was a wide ring. A border with no edges or corners is its
fill alone, whatever its dimension (KOTOR 2's minus arrows declare 32 on
a 28 button). Each options page's `LB_DESC` tells the focused control:
KOTOR 2's own talk-table entry where the setting is KOTOR 2's, ours
(`strings::*_HELP`) where it is not (anti-aliasing's steps, the
controller's, autosave on each area).

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
greyed. Each button shows its row's `icon` in place of the layout's
picture, as both games do. A mod's table may name any control of a mod's
layout as a planet's button, not only those whose tags start with
`LBL_Planet_`. It opens on the planet the script named (where the party is):
chosen, its name and description (`planetary.2da`), and its model
turning in the 3D view (`planetary.2da`'s `model`, seen from its
`camerahook`, lit by its own aurora light). A selectable planet pressed is
chosen; Accept travels there (`GameAction::Travel` → `Action::Travel`);
Back closes the map. Every player sees it; only the host's Accept travels
(a guest's map has none, and closes when the module goes).

The credits (`credits`, KOTOR 2's `credits_p`, skinned), which a script
starts (`StartCreditSequence`: K1's ending): each `credits.2da` row's
block of names rises through the list on black (`ListView::roll`, 40
layout pixels a second [verify]), the music `credits` looping, then the
main menu; Back goes there at once. The game's end without its death
screen (`EndGame(FALSE)`) is the main menu at once, unless credits roll;
both wait for the movies before them. A game won is noted beside the
saves with each player's own character as it ended (`played.json`), for
the main menu. With its death screen the game is
lost: the fall's box (below).

Pazaak (`pazaak_wager`, `pazaak_setup`, `pazaak_game`;
`game::pazaak_view`, `docs/PAZAAK.md`) is KOTOR's three screens over
the match the snapshot carries (`sim::pazaak::Match`): the view opens the
one for the match's stage (`pazaakwager`, `pazaaksetup`, `pazaakgame`;
TSL's `_p` skinned) and closes them at its end; a script's `PlayPazaak`
notice opens nothing itself, as with the store. Each seat sees the match
it plays (`Snapshot::pazaak_of`), so two players can sit at two
tables at once; the others play on. A player watching another's match
(`Snapshot::watching`) sees its table read-only whatever its stage:
the player's hand as the player sees it, "Watching Carth" in place of the
player's name (shortened to fit, `ControlState::shorter`), a wager or a
side deck being chosen in words over the hand, a result in the third
person ("Carth won the set."), the play buttons faded and nothing sent,
Leave (its key named) on TSL's Forfeit button, and the Leave key ("Leave
a conversation", L) leaving too (`Action::Watch` with no player); no
tutorial pages. A paused match's screens and its tutorial pages close on
its player's screen and come back at its stage (`docs/PAZAAK.md`); a
watcher keeps the table, Leave live, and is told why in the same words'
place ("Carth is away.", "Carth is hurt."). The cards are drawn in
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
  while a guest asks the host in, each "Carth asks you to join:
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
aside for a camera feed (a line with a camera while it plays, one with
no text until the next line), and the
cinematic overlay keeps only the voice. A message longer than its box is
cut.

Real time: nothing pauses. The character idles under a screen and can be
hurt; death closes every screen. One exception keeps the scripts as the
games run them: both games pause while their upgrade bench is up, so a
script that shows the bench and then delays code (`DelayCommand`) means
that code to run after the bench. The code that script delays from then
on waits until the first seat closes the bench (`Action::ScreenClosed`),
and then runs as long after the close as it asked. A mod's workbench takes
back the crystals left over in this way, once the player has fitted one.

Actions: a screen's `GameAction` becomes a `sim::action::Action` sent to
the host (reliable message 7) and applied in the step. What the host tells
one player is a `sim::notice::Notice`: words (message 10) or a screen a
script opened (`OpenStore`, `ShowLevelUpGUI`…, message 11).

Running and checking:

- `kjk` opens the main menu; `kjk play` starts in game. Esc, I, J, M, P, K
  open screens. `kjk play --gold N` starts the party with N credits,
  `--item TEMPLATE=N` with items, `--skill computer=8` sets a rank.
- The key map's "Equipment", "Character" and "Messages" rows open those
  pages of the in-game menu, as I opens the inventory. They have no key
  and no chord until the player binds one. While the menu is open, a
  page's key turns the menu to that page, and pressing it on its own page
  closes the menu.
- "Hide interface" in the key map hides the HUD, or shows it again. It has
  no key and no chord until the player binds one. A hidden HUD takes away
  the action bar, the party portraits, the minimap, the crosshair and its
  hit marks, the target block, the words floating over creatures (names,
  health bars and combat text), the area transition panel, the arena's
  panels and barks. The power wheel still shows while its key is held.
  Conversations, the status summary and a break's banner keep their own
  rules, and the key does nothing during a conversation. Opening any
  screen, such as the menu that Escape opens, shows the HUD again, so a
  player who forgets the key gets the HUD back. Headless runs press it
  with `ui hotkey hide_interface`, and `dump` writes `hud_hidden` in the
  `ui` fact.
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
- The party's bases are K1's Ebon Hawk (`ebo_m12aa`, and `ebo_m40ad` on
  the Leviathan), K1's Taris apartment (`tar_m02af`) and TSL's Ebon Hawk
  (`003ebo`). Their scripts take every companion out of the party and
  stand them around the area. In a base the host's pages also list those
  companions after the party (`cast::pages_of`, the snapshot's
  `waiting`), so the player can change their equipment there, as in the
  original games. The bar's two small portraits then show the character
  before the one shown and the one after it, going round. The HUD still
  shows the party alone.
- Security (`game::lock_view`): U (key mapping "Security") tries the
  nearest locked door or placeable in reach in front of the player
  (`Action::Unlock`); the console's `ui security [spike]` does the same
  with a spike of that template the party carries. The target block
  offers Security too (below). The host's answer is the feedback log's,
  in the game's own lines. `kjk play --skill security=N` gives the
  players N ranks.
- Console verbs: `ui open <screen>`, `ui show <layout>`, `ui capture NAME`
  (the view model of that moment as a capture of play; "Captures of play"
  below), `ui press <TAG>`,
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
  holster key), `ui swap` (the weapon swap key), `ui solo on|off` (solo mode, as
  the solo key turns it), `ui form ROW` (TSL's Form of that `spells.2da` row in use,
  as the next-Form key picks one), `ui portrait 2|3` (a click on the HUD's second
  or third portrait), `ui watch 2|3` (a click on that portrait's pazaak
  icon), `ui set KEY VALUE` (a setting changed as the options screens
  change it: `ui set display.size 800x600`), `ui camera orbit YAW PITCH`
  / `ui camera orbit off` / `ui camera zoom STEPS` (the camera's orbit held
  and turned there, let go, a zoom; `MOVEMENT.md`), `ui photo on` / `ui
  photo on X Y Z PITCH YAW [FOV]` / `ui photo off` (photo mode turned on at
  the shot the player sees or at that place, metres in game space and
  degrees, and turned off), `ui photo speed stopped|tenth|quarter|normal`,
  `ui photo hide` (the player's own character hidden or shown), `ui photo
  step` (one tick of a stopped replay) and `ui photo fly FORWARD RIGHT UP
  [TURN_RIGHT TURN_DOWN]` (the photo camera's move held, each from -1 to 1,
  and its turn in degrees a second; `ui photo fly 0 0 0` lets go;
  `game::photo_mode`), and `ui script upgrade|levelup|party|death|credits|end|box
  <strref>|movie <name>|galaxy <planet> [<available> <selectable>]` (`party` lists no
  companions: `ui open party` in play shows the host's table; rows by commas,
  `ui script galaxy 3 1,3,4,5,6,7,9 3,4,5,6,7`) (a store opens
  with a trade: `kjk play --run k_ptar_larrimsto` on `tar_m02aa`). In a
  conversation: `ui reply N` (the Nth reply offered, 1 to 30, as the
  screen picks it), `ui leave` (Leave; watching a pazaak match, its
  Leave), and on the host's screen `ui
  accept` and `ui end` (the request card's keys; they wait as the keys
  do), for scripted runs and `cargo xtask duo`. `ui reply` also answers in
  a headless run with no keys, such as a save loaded with `--load`.
- `kjk gui probes` runs the checks the approved test `gui_layouts` asserts.
  `kjk gui render k1 <layout>` draws one layout; `kjk gui sweep` paints
  every layout of each install; `kjk gui sheet` makes a contact sheet.
- **Control trees.** `kjk gui screen` writes a control tree next to its
  PNG, a JSON file of the same name. `kjk gui sweep --out DIR` draws every
  layout at rest at its own size into `DIR/<game>/`, a PNG and a control
  tree each. A tree holds no pixels, so it can be committed where the
  picture cannot. The same state gives the same PNG and the same tree,
  byte for byte. Agents read the tree instead of guessing from the
  picture, and the UI lint (docs/UI-GUIDELINES.md) checks it. Format 1:
  - `shown`: the game, and the screen and its steps (or the layout, from
    the sweep). `window`: the picture's size in pixels.
  - `focus`: the top screen's focused control. `back`: what Back does
    there, the screens open after it (top last) and the intents it
    raised. It is null in the sweep.
  - `layers`: each open screen, bottom first, with its `screen`, its
    `layout`, its `scale` (window pixels to a layout pixel) and its
    `controls`, in paint order. A hidden control is not listed.
  - A control: `tag`, `id` (-1 for the layout's root), `kind`, `rect` (left,
    top, width, height in window pixels), `focused`, `disabled`, `strref`
    (the talk-table entry its words come from, if any), `focusable` and
    `moves`. `moves` gives the control that up, down, left and right lead
    to, as the stack moves the focus (`MOVETO`, else the nearest control
    that way). Only the top screen's controls that can take focus have
    them. A list box takes up and down itself row by row before its
    `moves` apply.
  - `words`: each block of words the control set (one for each row of a
    list), with its `text`, `font`, `rect`, `color` (RGBA as drawn, the
    focus's pulse in its alpha) and `background`. `background` is the
    darkest and the lightest colour behind the words, read from the
    picture drawn without any words. `body_height` is the font's body
    height in window pixels: the rows from the top of its tallest letter
    to the bottom of its lowest one (the digits and the letters A to Z,
    both cases), drawn by the renderer on a line that starts on a whole
    pixel, a pixel counting from half its alpha. This is the measure
    docs/UI-GUIDELINES.md uses.
- **UI catalogue** (`ui::catalogue`). Every module of `ui::screens` has a
  story file of its own name in `data/ui/stories`, and the approved test
  `ui_catalogue` fails when one is missing. A file gives the module's
  status in the audit (`not started`, `needs work` or `audited`), its
  notes, and its stories. A story is a named state of a screen: its
  `name`, the `screen` as `screens::open` names it, the `steps` that
  `kjk gui screen --step` takes, the `games` it is drawn on (both by
  default) and, for the message screen, an `error`. A story may also start
  from a capture of play instead of stand-ins (below). Every module has at
  least one story. A module that is only a part of other screens (the
  companion places, the filter, the tab and the menu bar) has stories that
  open the screens drawing it. Each story is drawn at the six sizes of docs/UI-GUIDELINES.md
  (`catalogue::SIZES`: `deck`, `1080p`, `4k-tv`, `4k-monitor` at a scale
  factor of 2, `4x3`, `ultrawide`) and with two inputs (`pad`, its hints
  shown, the Steam Deck's buttons on `deck`; `mouse`). The screens step in
  the logical window and are drawn at the physical size, as in play, so
  `4k-monitor` draws as `4k-tv` does: the screens keep their size at any
  desktop scaling. `kjk gui list [--json]` lists the modules, their
  statuses and stories, and the sizes and inputs. `kjk gui show MODULE`
  gives a module's notes and its stories' steps. `kjk gui story MODULE
  [STORY] [--at SIZE] [--input pad|mouse] [--game k1|tsl]` draws stories,
  a PNG and a control tree each, into
  `kjk-artifacts/gui/catalogue/<game>/<module>/<story>/<size>-<input>.png`.
  The steps a story takes and the stand-in view models live in
  `kjk_game::story_stage`, so that the in-game catalogue can stage
  stories as the shell does.
- **Captures of play** (`ui::catalogue::Capture`, `data/ui/captures`). A
  capture is the UI's view model at one moment of a real run, written as
  JSON with the window it was drawn in. It holds numbers, the names of
  textures and kjk's own words, but no game art and no text of the game's,
  so it can be committed (AGENT.md forbids committing the game's files and
  dumps of them). Each of the game's words, such as a creature's or a
  power's name, is written as its line in the install's talk table
  (`{"$talk": 1234}`), and read back from the install when a story is
  drawn (`ui::catalogue::TalkTable`). A line shown with its braces taken
  out says `"unbraced": true`. The text fields `area` and `name` of a
  save's summary say `"text": true`. A run of bytes is swapped only when
  it has three bytes or more and a letter among them, so pairs of numbers
  such as a character's health stay as they are. The settings are written
  as they are. A capture also holds the key bindings of the run. The list
  of saves on disk is left out, because it names the player's own saves.
  - **Taking one.** The console command `ui capture NAME` writes the view
    model of that moment to `data/ui/captures/NAME.json` in the checkout
    the game was built from. A name is letters, digits, `-` and `_`. It
    works in a `--cmds` script and in a headless run. Run such a script
    with `KJK_SETTINGS` naming a copy of the settings file, so that its
    `ui set` commands do not change the player's own settings. The game
    fills the action bar and the key hints for the input in use, so a
    story drawn with both inputs needs two captures of the same moment:
    one taken with the mouse and one taken after `ui pad xbox`. For
    example, the captures of the HUD's and the wheel's stories were taken
    with `kjk play --stage 301nar --class jedi --power 9 --power 21
    --power 26 --force-points 3 --item g_i_medeqpmnt01=3 --item
    g_w_fraggren01=2 --hostile exchangethug2 --size 1920x1080 --headless
    --cmds 'wait world; wait 40f; ui target exchangethug2; wait 30f; ui
    powerwheel hold; wait 30f; ui capture nar-shaddaa-low-force; quit'`,
    and again with `ui pad xbox` after `wait world` and the name
    `nar-shaddaa-low-force-pad`.
  - **A story from a capture.** A story is one situation, such as a
    hostile target held, and names a capture of it in each game it is
    drawn on: `capture = { k1 = "taris-lower-city-target", tsl =
    "nar-shaddaa-target" }` for the mouse, and `pad_capture` the same way
    for a pad. A story with captures is drawn on both games unless its
    `games` says otherwise, so the catalogue shows the situation in each
    game's own look. A capture records the game it was taken in. Its view model is
    then the capture's, and none of the stand-ins is added. A story with a
    `capture` and no `pad_capture` is drawn with the mouse alone. The
    `pad KIND` step renames the pad glyphs a capture holds
    (`kjkpad_<kind>_<chord>`) for that pad, so a capture taken with an Xbox
    pad shows the Steam Deck's buttons at the `deck` size. The story then
    takes its own steps on top, such as `set display.target_top false`.
  - **What is drawn over play.** The screen `play` (`catalogue::PLAY`)
    draws everything that stands over play besides the screens of the
    stack, over a plain background (`ui::over_play::OverPlay::still`). It
    draws them in the order a frame of play stacks them. First come
    KOTOR's HUD and its floating target block, the words floating over
    creatures, the area transition panel, the power wheel and the arena's
    line and panel. Then come a terminal's camera feed, a bark, the
    status summaries, the conversation, the host's request card and the
    banner. A conversation hides the HUD and what stands with it, barks
    and the summaries, as in play. The game draws the same layers each
    frame through the same `ui::over_play::OverPlay`, so a story shows
    what play shows. A minigame and a fade also hide the HUD in play, but
    a capture does not record them. Its stories start from a capture. The
    step `feed on` turns a terminal's camera feed on, as the game does
    while a computer conversation shows a camera. The wheel takes three
    steps of its own. `wheel at N` highlights entry N, counting from 0.
    `wheel pick NAME` highlights the first entry whose name holds NAME,
    such as `wheel pick Stun`. `wheel close` shuts the wheel. The pointer
    goes to the highlighted entry, and the arc of the entry highlighted
    before goes away, because only the game knows an entry's arc.
  - **The check.** The approved test `ui_catalogue` fails when a capture a
    story names is missing, does not read, reads otherwise once written
    again, was taken in another game than the story is drawn on, or was
    taken with the wrong input. A story with captures must name one for each
    game it is drawn on, with each input, and must open its screen on its
    capture. It needs no game data, so it
    reads each line of the talk table as stand-in words (`talk 1234`,
    `ui::catalogue::StandInTalk`).
- **The catalogue in the game** (`kjk_game::catalogue_view`,
  `ui::virtual_screen`). `kjk gui catalogue [--story MODULE/STORY] [--at
  SIZE] [--game k1|tsl]` opens the game on one story, live. The story is
  `MODULE/NAME`, where the name may be the story's slug, such as
  `options/gameplay-page`. Without `--story` it opens on the list of
  stories (below). `--at` names the size it opens at (`1080p` by default).
  `--headless --cmds '…'` work as for any run (docs/RUN.md).
  - **The virtual screen.** The story is staged with
    `kjk_game::story_stage::Staged` in a window of the chosen size and scale
    factor from `catalogue::SIZES`, whatever the real window is. A second
    camera draws its draw list into an image of the device's exact pixel
    size, through the same material the game uses (`ui_material`). The
    window shows that image. The screen takes the mouse, the keyboard and a
    pad. It runs in real time and plays its sounds. When its last screen
    closes, the catalogue goes back to the list of stories.
  - **The list of stories** (`ui::screens::catalogue_list`). The list is a
    screen of KOTOR's own skin, drawn on the save list's layout, so it
    follows the game that is shown (KOTOR 1 or KOTOR 2) and works with the
    mouse, the keyboard and a pad. It has a row for each screen module, with
    the module's status from its story file on the right, and under it a row
    for each of the module's stories, written after a dash. The panel on the
    right names the module and its status, and the games the pointed story is
    drawn on. Open, or the pad's A on a row, opens the story live with its
    steps taken. Open on a module's row opens the module's first story. A
    story that is not drawn on the game shown opens on its first game. The
    list has a story file of its own, `catalogue_list`, as the approved test
    `ui_catalogue` asks of every screen. The stories come from
    `ui::catalogue::read_all`, through the view model's `catalogue`.
  - **Back to the list.** `F8` or the pad chord LB with B (`LB+East`) goes
    back to the list from any story. The list opens with its cursor on the
    story that was open. The story that was open stays the one the next
    chord returns to.
  - **The three modes.** Fit scales the whole image to fit the window and
    leaves black bars. Pixel for pixel shows one pixel of the image as one
    pixel of the display. As seen scales the image so that a pixel of the
    device looks as big to the owner as it does on the device, seen from the
    device's usual distance. The scale is the device pixel's apparent size
    over the display pixel's, where a pixel's apparent size is its width
    over its distance (`virtual_screen::seen_scale`). The plan gives each
    size's screen and distance (`virtual_screen::viewing`): 7 inches at 35
    cm for `deck`, 24 inches at 60 cm for `1080p`, 55 inches at 2.5 m for
    `4k-tv`, 27 inches at 60 cm for `4k-monitor`, 20 inches at 60 cm for
    `4x3` and 34 inches at 70 cm for `ultrawide`. On a display of 254 pixels
    to the inch seen from 60 cm, a Steam Deck pixel covers about 2.02 pixels
    of the display and a pixel of the 4K TV about 0.76. The owner's distance
    is the setting `catalogue.distance_cm` (60 by default).
  - **The display's density** (`kjk_game::display_density`). The catalogue
    asks the operating system how big the monitor the window is on is, and
    divides the monitor's pixels by its width. On macOS it reads
    `CGDisplayScreenSize` and the width in pixels of the display's current
    mode. On Windows it reads the monitor's raw dpi (`GetDpiForMonitor` with
    `MDT_RAW_DPI`), which comes from the size the monitor reports. It reads
    again twice a second, so a window moved to a second monitor takes that
    monitor's density. The setting `catalogue.display_density` overrides
    the system's reading when it is set. When the system reports nothing, as
    in a headless run, or on a platform other than the two, the density is
    110. `ui catalogue density N` sets the override from a script, and `ui
    catalogue density system` removes it and reads the main display. A dump
    shows the density used, the system's reading and the setting.
  - **Opening a large image.** When the image is larger than the window in
    pixel for pixel or as seen, the window opens centred on the focused
    control, or on the middle of the image when nothing has focus. It does
    the same after a change of size or mode, and when a story opens. Panning
    starts from there.
  - **Panning.** When the image is larger than the window, which pixel for
    pixel and as seen make it on a large device, the right mouse button
    dragged or the pad's right stick moves the window over it.
  - **The pointer.** The window's pointer is mapped into the image in all
    three modes (`virtual_screen::Placement::to_image`) and then into the
    virtual window's logical pixels, so a click lands on the control it is
    over. A pointer in the black bars is not over the screen.
  - **Keys.** `F6` or the pad chord LB with Y (`LB+North`) changes to the
    next of the six sizes, `F7` or LB with X (`LB+West`) to the next mode,
    and `F8` or LB with B (`LB+East`) back to the list of stories. A size change keeps the screen as it is: the stack is stepped in
    the new window and lays itself out again. The keys are the table `KEYS`
    in `catalogue_view.rs`, one row for each, with a key and a chord.
  - **A pad alone.** Every step works with a pad and nothing else. The d-pad
    moves through the list, A opens a story, the screen takes the pad as in
    play, LB with Y changes size, LB with X changes mode, the right stick pans
    an image larger than the window, and LB with B goes back to the list. A
    headless run does this with `ui nav up|down|activate` for the d-pad and A,
    and `ui raw Pad:LB+North` (or `LB+West`, `LB+East`) for the chords.
  - **Console.** `ui catalogue size NAME|next`, `ui catalogue mode
    fit|pixel|seen|next`, `ui catalogue pan X Y`, `ui catalogue density N`
    and `ui catalogue distance CM` do the same from a script. The `ui`
    commands that move the pointer (`ui pointer X Y`, `ui click X Y`, `ui
    down`, `ui drag`, `ui up`) take points of the window, and the catalogue
    maps them as it maps the mouse. `ui press TAG`, `ui nav`, `ui raw`, `ui
    type` and `ui back` reach the screens. A dump has the fact `catalogue`,
    with the size, the mode, where the image stands in the window and the
    focused control.
- **Working with the catalogue** (`kjk_game::catalogue_view`, its module
  `collab`). Five things let an agent and the owner work on a story together.
  - **The agent bridge.** When the catalogue starts, it creates three files
    in a folder of its own and prints their paths. The folder is the run's
    folder, under `catalogue/`, or `kjk-artifacts/catalogue/PID/` when the run
    has none, with the catalogue's process id, so two catalogues never share
    a file. The state file `state.json` is kept up to date while the
    catalogue runs. The command file `commands.txt` is read by the catalogue
    about ten times a second. The log `log.jsonl` has one line for each
    change. Each catalogue also writes a pointer, the file
    `kjk-artifacts/catalogue-bridges/PID.json`, named with its process id. The
    pointer holds the paths `state`, `commands` and `log` and the `pid`. The
    catalogue removes its pointer when it exits. When it opens, it also
    removes the pointers of processes that are not running any more, which a
    crash leaves behind. An agent that finds one pointer drives that
    catalogue. An agent that finds several picks the one whose `pid` it
    started, or whose state file shows the story it wants, and never writes
    to the others. The agent never touches the window. It reads the state
    file and appends lines to the command file.
  - **The state file** is one JSON object. The state file is written whole
    and renamed into place, so a reader never sees half of it. It is written
    again only when something in it changed, and the words of the controls
    count as something: a credit count that changes alone writes the file
    again. Its `revision` counts the
    writes. Its fields are `story` (`MODULE/NAME`), `game`, `size`, `device`
    (the size's pixels), `factor`, `mode`, `focus` (the focused control's
    tag), `screens` (the open screens, bottom first), `outlines`,
    `backdrop`, `backdrop_still` (`making`, `ready` or `failed: WHY` while the
    backdrop is `play`, else null), `recording` (its `steps` so far and the `saved` stories),
    `commands_read` (how many command lines the catalogue has read),
    `last_command` (the last line, with `ok` and an `error` when it failed)
    `tree_current` and `tree`. The `tree` is the control tree of a recent
    frame in the format of "Control trees", with one difference: `back` is
    null, because asking what Back does would close a screen. It is built
    again when the frame's controls change, but at most every 0.4 seconds,
    because it draws the screens a second time to read the colours behind
    the words. `tree_current` is true when the tree is of the frame the
    other fields describe. An agent that needs the tree of a change reads the
    state file again until `tree_current` is true. The colours of the words
    can be a moment old while a control's focus pulses.
  - **The command file** takes one command on each line. The catalogue reads
    a line only when it ends with a newline, and ignores empty lines and
    lines that start with `#`. The commands are `open MODULE/STORY`, `size
    NAME`, `mode fit|pixel|seen`, `step STEP` (any step of `Staged::step`, such
    as `step press BTN_GAMEPLAY`), `outlines on|off`, `backdrop
    plain|stripes|play` and `save [NAME]`. A command that fails does not stop the
    others. The state file's `last_command` and a line of the log with the
    change `refused` say why.
  - **The log** has one JSON object on each line, with `n` (the line's
    number), `seconds` (since the catalogue started), `source`, `change` and
    `detail`. The sources are `keyboard`, `mouse`, `pad`, `console` (a `ui`
    command of a run's script), `agent` (the command file) and `disk` (a
    reload). The changes are `story`, `size`, `mode`, `backdrop`, `outlines`,
    `step` (a step taken, whether the owner's input made it or an agent
    asked for it), `save`, `reload` and `refused`.
  - **Saving a tryout as a story.** The input of the owner on the live
    screen is recorded as steps, from the moment the story opened. A key
    press becomes `key Down` and the other moves of `Staged::step`. Enter
    on the focused control becomes `press TAG`, with the tag of that
    control. A click becomes `press TAG`, with the tag of the focusable
    control under the pointer, and a click on nothing is not recorded. The
    wheel becomes `hover TAG` and `scroll N`, where the control is the one
    under the pointer. Typing becomes `type TEXT`. When the pointer moved the
    focus onto a control and a later key depends on it, a `hover TAG` step
    comes first. A step that an agent asked for is recorded too, so the
    replay matches the screen. A click on a row of a
    list becomes `row TAG N`, with the tag of the list and the number of
    the row from 0 that the click moved the list's cursor to, and the step
    clicks that row again. A raw key, mouse button or pad chord that no
    key move comes with, such as `MouseBack`, becomes `raw NAME`. A raw name
    that comes with a key move is not recorded, because the key's own step
    makes the same move. A click on the main button is never recorded as raw.
    `F11` or LB with the D-pad's down button, or the command `save`,
    appends the recording to the end of the story file of the open story's
    module, as a new story called `Tryout 2026-11-27 21:04` (UTC). The new story has the open story's screen,
    games, captures and error, and its steps are the open story's steps
    followed by the recorded ones. The owner can rename it in the file
    later. Before it writes the story, the catalogue stages it again from its
    steps and compares the controls with those on screen (their tags, places,
    focus and words). The line of the log for `save` says `replays`. When the replay differs,
    nothing is written, and the log's line says the first difference. The recording goes on after a save, and it ends when
    the story opens again. `hover TAG` is a step of `Staged::step` that
    moves the pointer to the middle of a control.
  - **Hot reload.** The catalogue looks every half second at the open
    story's file in `data/ui/stories`, the folder `data/ui/captures` and
    `data/ui/tokens.json`. When one changed, it reads the story again, stages
    it again at the same size and takes the steps of the tryout once more.
    The window shows the result. A story file that does not parse, or that
    no longer has the story, is reported in the log as `reload refused` and
    the old screen stays. Saving a tryout does not reload. The files
    watched are listed in `sources` in `collab/reload.rs`. The override
    layouts of https://github.com/kjk-engine/roadmap/issues/25 are added to
    that list, so their change reloads the story the same way. Today the
    tokens file changes nothing that is drawn, because the screens read their
    palettes from the code, so a change there only stages the story again.
  - **Outlines.** `F9` or LB with A (`LB+South`), or `outlines on|off`,
    draws a frame around every control of the current frame, each with its
    tag in a black box. The frames of each open screen have their own
    colour, and the focused control's frame is orange and twice as thick. The
    frames are drawn from the same marks as the control tree. Their lines
    and tags are sized in the window's pixels, so they read the same in
    every mode. The text beside a frame comes from `label` in
    `collab/outline.rs`, where the lint of
    https://github.com/kjk-engine/roadmap/issues/24 adds the numbers of the
    rules a control breaks.
  - **Backdrops.** `F10` or LB with the D-pad's up button, or `backdrop
    plain|stripes|play`, changes the backdrop behind the screens. Plain is
    black, as the catalogue has always been. Stripes are diagonal bands of two
    greys. Play is a frame of the game world, so that the see-through parts of
    a KOTOR panel, and a HUD, show what the player sees behind them.
  - **The still behind the play backdrop** is a picture of the world with
    the HUD hidden, made by kjk itself. The catalogue runs `kjk play --stage
    STAGE --headless --size WxH` with `wait world; ui hotkey hide_interface;
    wait 240f; screenshot still; quit`, where `WxH` is the device's pixels of
    the chosen size. A thread starts the run, so the catalogue never waits
    for it. The screens show the plain backdrop until the picture is ready,
    and the state file's `backdrop_still` says `making`, `ready` or `failed:
    WHY`. The picture is kept in `kjk-artifacts/catalogue-stills/STAGE-WxH.png`,
    so each is made once for each stage and size. It is never committed,
    because it holds the game's art. The stage comes from the start of the
    name of the story's capture for the game: `endar-spire` is `end_m01aa`,
    `taris-apartment` is `tar_m02af`, `taris` is `tar_m03ae`, `nar-shaddaa`
    is `301nar`, `telos` is `202tel` and `floors-stage` is `floors`. A story
    without a capture of that kind uses `tar_m03ae` on KOTOR 1 and `301nar` on
    KOTOR 2. The table is `stage_for` in `collab/backdrop.rs`. A still that
    failed is not tried again until the catalogue starts again. The folder
    of the failed run stays next to the picture's place, named
    `making-STAGE-PID`, so that its log can be read. A headless run turns to
    the play backdrop with the console's `ui raw F10`, pressed twice from
    plain, and takes its picture with `screenshot` once the state file's
    `backdrop_still` says `ready`.
- **Design tokens** (`data/ui/tokens.json`, the W3C Design Tokens format
  2025.10). They hold the screens' palettes, which are the source of the
  colours in the code (the probe `palette_tokens` fails when a palette
  differs), KOTOR's bitmap fonts by their use (`com.kjk.bitmapFont` holds
  the texture), and the smallest body heights of docs/UI-GUIDELINES.md.
  `kjk gui census` counts what the layouts of each install use into
  `data/ui/census.json`: the layouts, the controls of each kind, and the
  controls that use each font, text colour and border texture. It is the
  "before" the tokens are measured against.
- Settings live in the user's config folder (`KJK_SETTINGS` overrides it).
- The options (`ui::screens::options`, a table per page): gameplay's
  inverted camera and its Camera page (ours, on TSL's unused second
  reverse box, drawn as the Mouse Settings button is; `optsound`'s layout: distance, height, shoulder and field of
  view on its sliders, the preset on its Advanced button, and three copies
  of that button stacked under the sliders, `screens::stack_under`, for the
  guns' camera (its own page, the same layout, `options_camera_guns`, its
  preset button moved into the sliders' box),
  how closely the camera follows and the body's fade; `MOVEMENT.md`), the
  mouse's sensitivity and buttons, key mapping, the
  four volumes (`SOUND.md`), and the graphics (`game::display`):
  brightness (a gamma over the whole frame in `ui_composite`, a half to
  twice, 1 in the middle), the screen resolution (`optresolution`: windows
  of common sizes that fit the screen, largest first, borderless, and, on
  Linux only, the screen's own fullscreen modes; sizes in the screen's
  pixels; the window's present one chosen). On Windows and macOS the list
  has no fullscreen modes, and a fullscreen display kept in the settings
  opens borderless over the whole screen. On Windows, exclusive fullscreen
  changed the whole desktop's mode and made the game's window topmost. On
  an NVIDIA card with Vulkan the GPU device was lost a second after the
  switch, the game stopped answering, and the player could not get back to
  the desktop without turning the PC off. Borderless changes no mode and
  is not topmost. A change of display is put to the window once, after its
  screen is known. The graphics page also has the advanced page's
  anti-aliasing (off, 2x, 4x, on every
  camera drawing to the window; TAA, kjk's own upscaler at Quality,
  Balanced, Performance and Ultra Performance (`game::upscale`), and DLAA
  and DLSS's four qualities
  where the build has DLSS and the card offers it, `game::temporal`,
  README.md), Lighting and v-sync, with Cancel. A row of the
  resolution list is chosen by a click, or by Enter or the pad's A. The
  pointer passing over a row does not choose it. This is a deliberate
  difference from KOTOR 2's other lists, where the row under the pointer is
  chosen: the OK button sits under this list, so the pointer crossed the
  rows below the chosen one on its way there, and OK applied the last row
  it crossed. Alt+Enter goes between a window and borderless, and saves
  the choice at once. Applied at once and at the next start.
  The grass check box (`display.grass`, on) hides the area's grass without
  rebuilding it. The Shadows check box steps through Off, Low, High and
  Ultra (`display.shadows`, High). Ultra is offered only on a card that
  traces rays. KOTOR 2's Soft Shadows box is the V-Sync box
  on the advanced page.
  KOTOR 2's Frame Buffer Effects box on the advanced page is Lighting. It
  steps between Classic and Enhanced (`display.lighting`, Classic).
  Classic is the original game's lighting. Enhanced is kjk's own, which
  adds glow to lightsabers, blaster bolts and muzzle flashes
  (`game::rolloff`, LIGHTING-PLAN.md). This is a deliberate change from KOTOR 2's options
  screens.
  The layout's own V-Sync box, the last row of the advanced page, is
  Effects Quality. It steps between Low and High (`display.effects`,
  High). At High, smoke, mist and the other particles of KOTOR's emitters
  are drawn as before. At Low they are drawn at half the frame's width and
  half its height, and then laid over the frame (`game::effects_quality`).
  KOTOR 2 has no such option, so this is also a deliberate change.
  KOTOR 2's Anisotropy row on the advanced page, just above Lighting, is
  Ambient Occlusion.
  Its arrows step between Off and On (`display.ambient_occlusion`, Off).
  On darkens the room's light in corners and where characters and objects
  stand close to each other, to walls or to the floor
  (`game::ambient_occlusion`, LIGHTING-PLAN.md). It needs Enhanced lighting,
  and TAA, DLSS or kjk's own upscaler. Under Classic lighting or
  multisampling it has no effect. KOTOR 2 has no such option, so this is
  also a deliberate change.
  Greyed: texture quality,
  the advanced sound page, the feedback list,
  difficulty, auto level-up, autosave and the minigames' axis. `dump`'s
  `display` shows the window. The main page's unused auto-pause button is
  Debug: it starts the combat sandbox alone (`SessionIntent::Sandbox`,
  `COMBAT.md`); only the main menu opens that page.
- The Interface page (`options_interface`) is kjk's own. KOTOR 2 has no
  such page. The graphics page opens it from the unused force speed box,
  drawn as a button. The page is a list of option rows on the Feedback
  page's layout (`optfeedback`), under the title "Interface", with the
  Feedback page's description box, Default button and Close button. Any
  options page can be built as such a list (`screens::options::Line`). A
  row is a check box, a stepped choice or a button that opens another
  page. A check box row works as the Feedback page's rows do. A stepped
  row shows its name and its value ("Menu width: Up to 16:9") and has no
  check box. A click, Enter, the pad's A or the X key steps it to its next
  value, and the last value wraps to the first. Left and Right on the
  keyboard and the pad step it back and on while it is the list's cursor
  row. The Feedback page itself is KOTOR 2's and is unchanged. The
  Interface page's eight rows are all deliberate additions to KOTOR 2:
  - Menu width (`display.menu_width`): Fill the screen, Up to 16:9 or 4:3.
  - Maintain image aspect ratio (`display.image_aspect`).
  - Text size (`display.small_fonts`): KOTOR 2's "Use Small Fonts", which
    KOTOR 2 sets in its ini file alone.
  - Text scale (`display.text_scale`): 80% to 150% in steps of 10%. KOTOR
    2 has no free text scale. 100% is KOTOR 2's size and the default. The
    scale multiplies the text size everywhere kjk draws interface text:
    the menus, the HUD, the status summaries and notifications, the area
    transition panel, the conversation's line and replies, the bark
    bubble and the floating words. `draw::Sizing` holds the multiplier
    (`Sizing::scale`) inside its text size, and nothing outside
    `ui::draw` reads KOTOR 2's text size without it. The boxes that KOTOR
    2 sizes from its text size grow with the scale: the message box, the
    datapad box, the status summary, the option list rows and the
    transition panel. A layout is read again when the text size changes,
    so that the rows fitted to it are fitted anew.
    KOTOR 2 picks its text size and its "a" or "b" font sheets by the
    window's width alone. A 3440 × 1440 window then draws text twice its
    size, where a 2560 × 1440 window draws it 1.5 times its size, and the
    menus look cramped in the wider window. kjk picks both by the width of
    a 16:9 window of the same height whenever the window is wider than
    16:9 (`draw::sized_width`). The screens then look the same in every
    window from 4:3 to 32:9 of one height. This is a deliberate change
    from KOTOR 2. A window of 16:9 or narrower is sized as KOTOR 2 sizes
    it.
  - Notifications in the corner (`display.corner_notices`).
  - Target block at the top centre (`display.target_top`).
  - Frame rate limit (`display.frame_limit`): Off, 30, 60, 90, 120, 144,
    165 or 240 frames per second. KOTOR 2 has no frame rate limit, and it
    starts off. A rate written in the settings file by hand, such as 75,
    limits the game too. The row shows it, and steps from it to the
    nearest listed rate. The game holds each frame until its time comes
    (`game::display::pace`), in play and while any menu is open, so the
    GPU and the CPU rest between frames. With V-Sync on as well, the lower
    of the two rates holds. The sim keeps its own clock, so the limit never
    changes what the sim does.
  - Join nearby conversations (`game.join_talks`, on while unset). In
    co-op, a player with it on joins a conversation the main character's
    player starts when they stand within 10 meters, can see it, and are
    free (`DIALOGUE.md`, "Joining nearby"). The player's game tells the
    host as the option changes and each time it joins.
  - Two safety ceilings apply to a game with a window (issue 318). When
    V-Sync is off and the limit is Off, play is held to 240 frames per
    second. While a screen is open or the window is in the background, the
    game draws at most 60 frames per second, or the player's limit when it
    is lower. A graphics card that runs flat out with no rest can make a
    weak power supply switch the whole computer off. The benchmark turns
    the ceilings off with `KJK_UNCAPPED=1`, because it measures how fast
    the game can draw.

  The page's Default button puts all seven back to kjk's defaults
  (`settings::page_keys`).
- The combat page (`options_sabers`, "Combat Options") is kjk's own. KOTOR
  2 has no such page. It was named "Saber Options" until the damage
  multipliers joined it. The gameplay page opens it from the unused reverse
  box, named "Combat" and drawn as a button, as the Camera page's is. The
  page is a list of option rows on the Feedback page's layout
  (`optfeedback`), as the Interface page is, with the Feedback page's
  description box, Default button and Close button. Its rows are all
  deliberate additions to KOTOR 2 (`docs/SABER-RULES.md` sections 12 and
  13):
  - Combat rules (`game.saber_rules`): a stepped row. Its values are Jedi
    Academy (`ja`, the default) and Advanced (`advanced`). A saved `mda` or
    `md` loads as `advanced` (`settings::retire_mda`). `kjk play
    --saber-rules` and `kjk host --saber-rules` hold other rules for one
    run, unsaved, until the page changes them. The rules are fixed for a
    session. The host takes them from the settings when its world is made
    or loaded from the main menu, and keeps them through module changes and
    saves loaded in play. A change of the setting applies to the next
    session. While a session runs, in solo play and in co-op, the row shows
    the session's rules and is greyed. Its description first says "The
    combat rules are chosen before a game starts. Change them from the main
    menu.", and then gives the rules' own description. A guest plays by the
    host's rules, from its snapshots. Advanced needs no Movie Duels.
  - Movie Duels stances (a setting of its own): whether characters stand,
    walk and block in Movie Duels' stances under the Advanced rules. It is
    greyed without Movie Duels installed and while a session runs. In
    co-op, every player needs Movie Duels only when the host has it on.
  - Overhead Stamina bars (`game.duel_bars`, off).
  - Unblockable warning (`game.duel_warning`, off, as Movie Duels has it).
  - Perfect block slow motion (`game.duel_slow_motion`, on). It slows time
    after the player's own perfect blocks alone (`settings::DuelOptions`).
  - Hilt (`saber.hilt.single`) and Double-bladed hilt
    (`saber.hilt.double`): stepped rows. They step through the hilts
    KOTOR's single-bladed and double-bladed lightsabers are drawn with.
    The values are KOTOR's own (unset, the default), Jedi Academy's
    (`single_1`, `dual_1`) and a few of Movie Duels': Luke, Obi-Wan,
    Vader, Ahsoka, Dooku and the Darksaber for the hilt, and Bastila's,
    Maul's, an Inquisitor's and Cal Kestis's for the double-bladed hilt
    (`docs/COMBAT.md`).
  - Damage taken by party (`game.damage_party`) and Damage taken by
    enemies (`game.damage_enemies`): the last two rows, stepped from 10% to
    100% in steps of 10%, then to 800% in steps of 50%. Both are 100% by
    default. Each row has KOTOR 2's minus and plus arrows at its ends, as
    the Advanced Graphics page's stepped options have. A click on the minus
    arrow steps the value down, and a click on the plus arrow or on the
    rest of the row steps it up (`paint::Row::arrows`). Each multiplies the damage of every hit on a party member, or
    on any other character (`sim::combat::damage::Taken`). The story's
    staged blows and deaths are never multiplied. A new or loaded game
    takes the values the settings have then, and the world keeps them in
    its snapshot, so a module change, a save and a replay keep them too. In
    co-op, the host's values apply. A headless run has no settings and
    plays at 100%.

  The hilts are read when a run loads. A hilt the install lacks leaves
  KOTOR's, and the run's notes say why. The page's Default button puts all
  the rows back to their defaults (`settings::page_keys`). A saved
  `game.duel_stances`, the retired setting that once turned Movie Duels'
  stances on, is dropped when the settings load
  (`settings::retire_duel_stances`).

  Movie Duels' choices are offered only with Movie Duels installed beside
  Jedi Academy (`ViewModel::movie_duels`, from `settings::movie_duels`;
  `docs/VFS.md`). kjk's rules need none of its files, and this is out of
  respect for its makers. Without it, the Movie Duels stances row is
  greyed, the two hilt rows step through KOTOR's and Jedi Academy's hilts
  alone, and a saved Movie Duels hilt shows KOTOR's. The description of the
  stances row then begins with "Needs Movie Duels installed beside Jedi
  Academy.", and the description of each hilt row begins with "Movie Duels'
  hilts need Movie Duels installed beside Jedi Academy." When Movie Duels'
  archives are there but none holds its stances or its hilts
  (`ViewModel::movie_duels_incomplete`, `vfs::MovieDuels::Incomplete`),
  the rows are greyed the same way, and each description begins with
  "Movie Duels is missing some of its files. Reinstall Movie Duels to use
  its stances and hilts." A greyed row can
  still be pointed at, so its description shows. Activating it does
  nothing. A guest plays by the host's rules either way. `kjk gui screen`
  draws a page with the saved settings (`KJK_SETTINGS` names another file)
  and the install found (`KJK_JA`).
- The combat arena's panel (`ui::screens::arena_panel`), in every arena but
  the floors stage: under the HUD's minimap in the HUD's frame, KOTOR 2's
  rounded rim (its option buttons' `uibit_brdr_16wc`/`16we`) in the
  campaign's colour over a dark fill, "COMBAT DEBUG", then a row per key
  (the key in the highlight colour, what it does, its state at the right:
  the foe F2 calls in, god mode bright when on).
The equip page: a slot chosen lists what the party carries that fits it,
after "None" and, on a line of its own, what the slot holds marked
"(Equipped)" (choosing that row changes nothing; the list's rows grow to
their text, `ListView::fit` with `pick`); Equip wears the chosen one on the member the bar's portraits bring up
(`GameAction::Equip { who, .. }` → `Action::EquipOn`) when the bar marks
it dressed (`MemberView::dresses`): the seat's own body, and for the host
the companions nobody plays. A guest's party starts with its body
(`cast::party_of`). TSL's second weapon set shows in both games (KOTOR's
own screen has one set: a remaster liberty, `WORLD-STATE.md`): its two
slots (`BTN_INV_WEAP_R2`, `…L2`; "Config 2" in the layout) list what the
first set's hands take and wear it in slots 18 and 19, and Switch Weapons
(`BTN_SWAPWEAPONS`) exchanges the sets of the member shown
(`GameAction::SwitchWeapons` → `Action::SwitchWeaponsOn`), greyed for one
this player does not dress. The swap key (B; TSL's own is H, the
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

What a character wears on its head is drawn as reone draws a mask: the
head slot item's model (`itemclass` and its `ModelVariation`, as a
weapon's) hung on the head model's `gogglehook` (`viewer::hang_headgear`),
for creatures as their templates dress them, disguises, and the seats'
bodies (`wardrobe`). The head slot carries an eye in its top right corner
(`BTN_HEADGEAR`, a remaster liberty: KOTOR always draws headgear), shown
while something is worn there: a click hides or shows the member's
headgear, its tip saying which ("Hide headgear", "Show headgear"). The
choice is drawing only (the item stays worn, its properties count) and the
player's own: a setting by the character (`headgear.hidden.seat<n>` for a
seat's, `headgear.hidden.<tag>` for a companion's, `game::headgear`), kept
in their settings file, so another player in co-op still sees it.

An item's description, wherever a screen shows one (equip, inventory,
store, workbench, upgrade bench: `game::character::described`), begins
with what a character needs to use the item (`ItemLook::requirements`),
as both games' executables write it. Each kind of requirement has a
heading from the talk table, then a line for each requirement, then a
blank line.

- "Feats Required" (both games) names the feats the base item requires
  (`baseitems.2da` `reqfeat0` to `reqfeat4`), then the feats of the item's
  feat limitation properties (`itempropdef.2da` row 57). An armour names
  only the highest armour proficiency its base item requires, so heavy
  armour shows "Armor Proficiency -  Heavy" and not Light or Medium. A
  feat name's colon becomes " - ", and the space after the colon stays,
  as in both games.
- KOTOR 2 writes "Attributes Required" before the feats, a line such as
  "Minimum Constitution: 12" for each attribute limitation (row 69, the
  minimum read from `iprp_attribcost.2da`). Its implants need these, not
  feats.
- KOTOR 2 writes "Gender Required", "Race Required" and "Useable By"
  after the feats, for the gender, subrace and companion limitations
  (rows 60, 61 and 62). A companion limitation of row 0 names the player.

Grenades, lightsaber crystals and KOTOR 2's rockets (`itemtype` 6, 46
and 49) list no requirements. The limitation properties listed as
requirements are not listed again among the stats. In TSL, once Bao-Dur
has been met (his influence set: `Screens::bao_dur_met`, KJKS 65), one of
the base items he cannot equip (the Jedi robes, the armoured flight suit,
the Zeison Sha armour, and a sonic grenade, QUIRKS.md Q-0266) then has
"Cannot Equip:" and "Bao-Dur" then a blank line, as TSL's executable
writes it (`sim::cannot_equip`). The stats follow, then the UTI's
`DescIdentified`.


The story's progress (`docs/WORLD-STATE.md`) rides in the snapshot
(`sim::Progress`: the journal and the party's XP):

- the journal page lists the quests, the latest updated first, each by
  name with the text of the entry reached (`global.jrl`, from the level's
  library); a quest whose entry ends it counts 1;
- the character page's Stance (ours, a copy of Close beside it, as far as
  the member's view reaches; `pages::Character`). It shows the stance set
  the member stands in, or the one it waits for, read from the snapshot
  (`CharacterView::stance`). It shows "Stance: Default" while the member
  stands in no set. A press steps the member's saved pick through its
  row's set (`data/duel_stances.toml`) and every skeleton offered
  (`ViewModel::stances`). The pick is kept in the player's settings as
  `saber.stance.<who>` (`CharacterView::stance_setting`, the headgear's
  `who`), and the game sends it to the host (`stance_sets::wishes`,
  `docs/SKELETON.md`). Its tip says what it is. Unless the session plays
  by the Advanced rules with Movie Duels' stances and its level has stance sets, it is greyed, a press does
  nothing, and its tip says "Stances come from Movie Duels. Turn on the
  Movie Duels stances setting to use them." It
  is hidden for a member the choice cannot be kept for, and for a member
  this player may not pick for (a guest picks only for its own
  character);
- the character page shows each ability score as items and effects leave
  it. The score and its modifier are gold when items and effects raise
  the character's own score, red when they lower it, and the plain green
  otherwise (KOTOR 2's `CSWGuiInGameCharacter::SetLabelBonusPenaltyColour`);
- the skills page shows the chosen skill's own ranks, its bonus and its
  total, as KOTOR 2's does (`CSWGuiInGameAbilities::OnEnterSkill`). The
  total is the skill's `GetSkillRank` (`CharacterView::skill_totals`): the
  ranks plus what items, effects and feats add and the key ability's
  modifier. The bonus is the total less the ranks, coloured as the
  character page colours a score. The rows are named "Skill Rank",
  "Bonus" and "Total Rank", as KOTOR 2's executable names them for its
  skills. Its layout's own words there, "Base Cost" and "Dark Side", are
  the powers' rows;
- the character page shows the XP and what the next level asks, and Level
  Up while the character may take one: it opens the level-up screen, whose
  Finish sends its class (the one made in creation, else Soldier), ability
  and skill ranks, feats and powers to the host
  (`GameAction::LevelUp` → `sim::action::Action::LevelUpOn`); the host picks
  the feats and powers left unpicked, checks the level and takes it;
  - the level-up screen shows the character's own ability scores and skill
    ranks, without what its items and effects add, and counts its skill
    points from its own Intelligence (`CharacterView::own_abilities`). The
    host takes the level from those, as TSL's executable does (its
    level-up counts the skill points from the base Intelligence). A
    skill's cap counts only its own ranks, so a character with Gear Head
    or an item that adds Repair can still buy Repair ranks up to the level
    + 3;
  - a feat or power whose prerequisite is another pick of the same level
    opens once that pick is made (`PicksView::feat_needs`, `power_needs`).
    TSL's executable reads a prerequisite from the level's own picks too.
    Dropping a pick drops the picks that need it;
  - a level the host refuses is told in a message box over the screen
    open, with the reason (`sim::levels::Refusal`, `loot::level_refused`);
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
- while the Advanced combat rules are played (`game::duel_view`, rulebook
  "Stamina Bars"), the Stamina bar of every character who blocks (one with
  a melee weapon in hand; a blaster, a stun baton and bare fists have
  none). The bar is green in four shades, from darkest green at
  the lowest band to bright green at the highest, and flashes when little
  is left. It wears KOTOR 2's
  portrait bar art, the vitality bar's own (its layout's `PB_VIT1`
  progress art, `uibit_bar_vp_p`: a framed bar rounded at one end over a
  third-clear inside): its backing in the layout's border colour (as the
  skin recolours it), then the bar's colour over its share, the art cut to
  the share rather than squeezed (`hud::meter`, `BarArt`). The player's own
  stands upright left of its portrait's vitality bar, as tall and
  level with its frame but 7 pixels wide to its 12, filling up from the
  bottom (`hud::duel_bars`, `HudView::duel`). Its width and the gap beside
  the vitality bar are stretched as the HUD's layout is stretched down
  (`hud::place`), so it keeps its shape in every window. With
  "Overhead Stamina bars" on, each living enemy that is in combat with
  the player or the party, or that the player has targeted, has two
  bars over its head, never a companion, a neutral or a civilian
  (`duel_view::Fight`). Each is the art turned a quarter, 60 by 6 pixels,
  filling rightward, in the vitality bar's art, sloped at its top corners
  (`floaty::OverheadArt`). The red health bar stands one pixel over the
  Stamina bar. The Stamina bar stands 4 pixels over a point 0.05 m over
  the top of the character's drawn head: its head hit-location node as
  posed, plus how far that model's top stands over the node at rest. The
  bars follow the head when it leans, swings low or lies down, and their
  height eases towards it in about 0.08 s. A body with no head node (a
  droid) has the point 0.05 m over the top of its body and head models at the scale it is drawn at, not
  counting what it holds (`duel_view::anchor`). They are faded out from 10 to 25 m
  (`ui::screens::floaty::bars`, `ViewModel::overhead`). They are placed
  with the frame's camera, in the floating words' layer
  (`ui_view::float_layers`). The Stamina bars are kjk's own: KOTOR 2's HUD
  has none, and none show under Jedi Academy's rules.
  With "Unblockable
  warning" on, a character swinging what only a perfect block stops
  (`Duel::warn`) wears a red shell, as the stealth field's
  (`stealth_view::follow`), never the player's own, nor one in stealth,
  nor during the player's conversation. With "Perfect block slow motion"
  on, in a game played alone, the player's own perfect block slows the host's clock a moment (`sim_host::Slowed`, rulebook "Perfect-
  Block Slow Motion"); a session of more than one player never slows.
- a script's message box of pages (`ui::screens::message::Pages`, the
  `confirm` layout, no cancel, OK turning the page): KOTOR's tutorial
  window (`ShowTutorialWindow`: the row's `Message_PC0` to `2`; its icon
  not drawn [verify]; off with the "Tutorial Popups" option,
  `feedback.tutorials`, on by default, kept in the settings file: the
  feedback page's list that holds it in KOTOR stays greyed here) and
  TSL's datapad (`DisplayDatapad`: the item's `DescIdentified`). KOTOR
  pauses under both; here the world goes on (a pause is a player's own).
  `ui script tutorial 51`, `ui script datapad qu_fakewill` open them.
- The action bar (`sim::combat::bar`, `game::bar_view`,
  `ui::screens::hud`). The HUD draws the action bar of the character the
  player controls where KOTOR draws its action columns. It shows six slots
  while the keys play, and the slots after the sixth up to the last one
  that holds something (the bar has ten). It shows four while a pad plays. Each slot shows the icon
  of the power, feat attack, item or mode in it, and its key or chord (`Action bar slot 1` to `10` in the
  key map; 1 to 9 and 0, and LB with A, B, X or Y on a pad). An item kind's slot
  also shows the uses left of the item it uses now. A slot the character
  cannot use now is drawn grey and at 45% opacity: too few Force points, no
  item of that kind left, or a feat attack the weapon in hand cannot make.
  An empty slot shows only its frame and its key. A slot whose item kind
  has two or more different items carried shows a stack of frames behind
  its icon. The grenades' slot is lit while a grenade is readied. A slot
  contains a Force power, a feat attack, mines, stealth mode, or one kind of item (medpacs,
  stims, energy shields or grenades). A change to the bar is a player
  action sent to the host (`Action::Bar`, wire 65), and the bar is kept on
  the character sheet, so it is saved and travels in snapshots. Not
  KOTOR's: its action columns are picked from with the mouse.
- TSL's Forms have no HUD column. The next-Form key (V, `Next Form` in the
  key map) puts the next Form the character knows in use at once, with no
  action and no cost, in a fight too (`Action::Form`, wire 47;
  `game::form_view`). The game's line for it ("{Shii-Cho} Determination
  Initiated" …, 124098 to 124108) tells it in the feedback log
  (`sim::combat::forms`, `docs/COMBAT.md`). [verify] KOTOR 2 keeps its
  stance column fixed on the HUD.
- The feat attack in use shows only on a bar slot that holds it. The
  next-feat key (T, the D-pad's up on a pad, `Next feat attack` in
  the key map) moves on the feat attack the weapon in hand makes
  (`Action::NextFeat`, wire 57; `game::feat_view`), and a label over that
  slot names it. With no feat attack on the bar, the HUD shows no sign of
  the change. [verify] KOTOR 2 picks from the column's list with the mouse
  and shows that line 4 s on the HUD.
- The item slots (`game::pick_view`, `sim::combat::picks`). Hold the key of
  a slot that holds an item kind and turn the mouse wheel to move through
  the items of that kind the character carries (`Action::PickItem`, wire
  58). On a pad, hold the slot's chord and press the D-pad's left or right.
  A label over the slot names each item with its uses left. Letting go
  after moving uses nothing. A kind is used on the press when the
  character carries one item of the kind, and on the let-go when it
  carries more.
- Readied grenades (`sim::world::movement::ready_grenades`,
  `Action::Grenade`, wire 67). A grenade slot's key, or the grenades' entry
  on the wheel, readies the grenade the slot uses. A grenade picked from
  the wheel's arc is readied too. Readying holsters the weapon drawn, and
  the character holds the grenade in the right hand in the first pose of
  KOTOR's `throwgren`. JA's `thermal/select` sound plays as the grenade is
  readied. While a grenade is readied, the attack button (the left mouse
  button, RT on a pad) charges and throws it, and the grenade in the hand
  glows yellow from the start of the charge. The other attack button (the
  right mouse button, LT on a pad) puts it away. Neither reaches the
  weapon. Choosing grenades again also puts it away. So do the holster key,
  switching weapon sets, and using a Force power, a feat attack, another
  item kind, mines or stealth mode. The grenade is put away when the
  character has no grenade left or goes down, when no seat plays the body
  any more, while a conversation holds the world or the player restrict
  mode is on, and when a movie starts. After a throw, and after every way
  of putting it away but the holster key, a going down and a change of
  leader, the character draws the weapon again if readying holstered it
  (`Pack::redraw`).
- The power wheel (`game::wheel_view`, `ui::screens::wheel`). Holding
  `Power wheel` (F, RB on a pad) opens one ring of everything
  the character can use that is not on its bar and not hidden
  (`bar::wheel`). An item kind on the bar stays on the wheel while the
  character carries two or more different items of that kind. The ring
  lists offensive powers, then defensive powers, then item kinds, then
  mines, stealth mode and feat attacks (`bar::Group`), with a small gap
  between groups. A power used on an enemy (`spells.2da` `hostilesetting`)
  is offensive, and every other power is defensive. The mouse or the right
  stick points at an entry, and letting go of the key uses it. A small dot
  shows where the pointer is. The centre of the wheel highlights nothing:
  a pointer closer than 0.08 of the screen's height, or the right stick
  pushed less than 30%, is in the centre, and letting go there does
  nothing. Moving back to the centre clears the highlight, so the player
  can open the wheel and back out. Each use joins a queue and is carried
  out after the ones before it, once the character has a free hand. A
  Force power or a mine pressed on the bar while the character is busy
  joins the same queue. Laying a mine stops a run: the movement keys move
  the character no more until the player lets go of them. The camera does not turn while the
  wheel is open. An item kind with two or more different items carried
  opens an outer arc that lists every item of that kind the character
  carries, and its entry shows a stack of frames behind it. The mouse
  wheel, or LB on a pad, steps through the arc. The ring shows up to
  28 entries at once (`RING_SHOWN`) and grows with the number of entries.
  When the character has more than 28 entries, each notch of the mouse
  wheel turns the ring by one entry. On a pad, each press of LB turns it on
  by one entry, and no button turns it back. The ring turns only while the
  highlighted entry has no arc of two or more items, because the same
  steps move through an arc. In a
  session that was started with one player seat, the host's clock runs at
  25% of normal speed while the wheel is open (`wheel_view::SLOW`). In a
  session started with two or more seats the clock never slows, even while
  a seat is still empty. The number of seats is set when the session
  starts. Entries the character cannot use now are grey and faded, as on
  the bar. Headless runs drive it with `ui powerwheel hold`, `up`,
  `point X Y` and `step N`, and `dump` writes a `wheel` fact. Headless runs
  hold a bar slot's key with `ui key hold slot N`, where N is the slot from
  1 to 10, and let it go with `ui key up slot N`. They hold [Aim] and
  [Attack] with `ui key hold aim` and `ui key hold attack`, and let them go
  with `ui key up aim` and `ui key up attack`. `ui look 10 -5` turns the
  player's view 10° to the right and 5° up. `ui look to 90 0` turns it to a
  yaw of 90° and a level pitch. `ui look at g_sithtroop01` turns it so the
  crosshair's line meets the chest of the nearest living creature with that
  tag. A `dump` taken while the player has a gun out, seen from behind, writes a
  `play.sight` fact. It gives the angle between the barrel and the line
  from the muzzle to the point under the crosshair, the angles between the
  head's facing and the gun and the point, and the last shot's push.
- The action bar setup (`ui::screens::action_bar`,
  `game::bar_setup_view`). The Abilities page's "Action bar" button opens
  it, and so does `Action bar setup` in the key map, which has no default
  key or chord. The page shows the ten slots at the top, every power, feat
  attack and item kind the character can put on the bar as small icons in
  groups under them, and an info box on the right. The info box names the
  icon or slot under the pointer, or under the pad's focus, and shows the
  game's description of it. For an item kind, the info box has the kind's
  name as its title and says which item the slot uses. Force Pull, which has no row of its own in
  `spells.2da`, is named "Force Pull" and shows Force Push's icon, cost and
  description. The player drags an icon onto a slot to put it
  there. A slot dragged onto another slot moves there, or swaps with it
  when that slot is filled. A slot's icon dragged off the bar empties the
  slot. A right click on a power hides it from the wheel or shows it again.
  With the pointer over an icon, a slot's key puts the icon in that slot.
  On a pad, A on an icon and then A on a slot puts it there, X on a slot
  empties it, and Y hides or shows a power. On this page grey means only
  that a power is hidden from the wheel. A party member another player
  controls gets no edits from this page. `ui down X Y`, `ui drag X Y` and
  `ui up X Y` drive a drag headless.
- KOTOR's status summary (`statussummary`, KOTOR 2's `statussummary_p`
  with the skin): what the scripts told the party (`docs/WORLD-STATE.md`'s
  table: journal entry added, XP and credits received or lost, light or
  dark side points, items received, named, and lost, KOTOR 2's influence
  and most Force points raised or lowered (`LBL_MAX_FP_GAINED`, `_LOST`:
  48610, 48611, each shown while what the statuses added to it is not 0);
  never what the party loots, trades or kills). As KOTOR's client, the statuses gather while a
  conversation, a fade or a screen is up, and the summary shows 0.25 s
  after its first status came and none is up, so that statuses raised a
  few frames apart show together: the layout's rows that apply, in its order, each as tall
  as its words (never less than its icon), the panel closing under the
  last. By default kjk shows it as notifications in the corner (below),
  because the mouse turns the camera in play and is not free to press an
  OK. With the Interface option "Notifications in the corner" off, it works
  as KOTOR 2's does. It stands in the middle of
  the window with an OK under the rows and stays until the player presses
  the OK, Enter, Escape, or a pad's A, B, X or Y. Statuses that come
  meanwhile gather into the next summary, shown 0.25 s after this one
  closes. In a window, while no other player has joined, the world waits
  under it, and a co-op session goes on. A headless run always shows the
  notifications in the corner, whatever the option says, because nobody is
  there to press the OK (`ui status journal`,
  `ui status xp N`, `ui status credits N`, `ui status light`, `ui status dark`,
  `ui status received ITEM` and `ui status lost` raise one from the console,
  and `ui datapad TEMPLATE` a datapad's pickup box).
  With "Notifications in the corner" on (the default), it has no OK,
  pauses nothing, stands just under the menu buttons in the top right
  corner, as wide as they are and lined up with their right edge (its
  words start just after their icons and wrap within it), slides in over 0.25 s, stays 6 s and fades over 0.6 s,
  a later one under it. Its words are in K1's own layout's blue, which the skin's
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
  it (`Screen::click`, a click on no control). Return to the Ebon Hawk is
  labelled as `k_sup_guiopen` set it (`SetReturnStrref`, run on the host
  each time this player opens an in-game screen: `Action::ScreenOpened`),
  greyed with no label; while the return is refused (`Transit::refused`)
  it is greyed yet takes a press, which says why in a message box, as
  K1's map page does; else a press asks the label's question
  (`return_confirm`) and OK sends `Action::ReturnToShip`, the in-game
  screens closing; a click on the HUD's minimap opens the page
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

## The crosshair and hit marks

- **The reticle** (`ui::screens::hud::reticle`, `game::reticle_view`):
  with a blaster in hand the crosshair is its reticle, its shape by the
  gun (a cross for pistols and rifles, three prongs for repeaters, two
  brackets for bowcasters, long lines for disruptors, a broken ring for
  ion weapons, a ring of dashes for sonic weapons), thin white strokes on
  a dark edge. Its gap is the spread the next shot may stray by
  (`sim::combat::bloom::shot_cone`) as it lands on the screen through the
  camera's field of view (`hud::gap_px`), so it widens as the trigger
  stays held and closes after a rest: wider at once, narrower eased
  (`reticle_view::ease`). A gun holstered or lowered out of a fight
  (`reticle_view::drawn`), a melee weapon, and a disruptor's Sniper
  Shot while it is steadied show the dot alone. It is drawn over the HUD as kept, in the HUD's frame as anchored,
  so a changing gap never redraws the HUD.
- **Hit marks** (`hud::hitmarker`, `game::hitmarker_view`): each blaster
  shot of the player's own that lands flashes four strokes on the
  diagonals outside the reticle, growing a little and fading: white for a
  hit, gold for a critical or an assured one, a kill four red ticks
  further out, a bolt a blade parried or deflected a short grey mark. A
  blade's, a power's or a grenade's hit marks nothing. Several in one step
  (a flechette's shards) are one mark, the strongest. Probes:
  `kjk gui probes` (`reticle_shapes`, `reticle_gap_matches_fov`,
  `hitmarker_fades`) and `kjk sim probes --only "reticle view,hitmarkers"`.

## Targeting and use

KOTOR's click on a door, container or creature, made for the JA camera:

- **The target** (`sim::target`, render side, `game::target_view`):
  what the crosshair (at the screen's middle: a dot, or with a blaster in
  hand its reticle, below) rests on within 10 m
  of the player and in sight, else the nearest within 4° of it and in
  sight, else the nearest within 2.5 m in front and in sight of the
  player's eyes (what lies at the character's feet). It holds 0.2 s.
  Pick volumes are cylinders (a creature 0.4 m by 1.9, a placeable 0.5 by
  1.5, a shut door 1 by 2.5; an open door none, its brushes only). A
  placeable hung over the floor the navigation mesh covers (a door's
  panel, invisible before its console: Yavin Station's, 1.1 to 1.7 m up)
  reaches down to that floor, one over no floor hangs about where it is
  placed, so the console drawn below its position is picked. A placeable
  with action points (the two use positions of its walkmesh) is also in
  sight when the line to its action point nearest the player is clear,
  ending as high above that point as a use looks
  (`interaction.using.action_sight_m`). Such a placeable is
  picked even where a solid stands between the camera and its body. Lonna
  Vash's body on Korriban lies inside a force cage, and its action point
  stands outside the cage. An action point behind a wall still hides the
  placeable. Targetable: doors but static ones, useable placeables,
  living creatures, and a downed party member (a seat's avatar, "Player
  N", or a companion, driven or not; lying, 0.8 m by 0.5). A living
  companion is a target whoever drives it, its AI or another player
  (owner, 2026-10-30: as KOTOR's party members are clicked and talked to),
  but only clear ahead of the player, more than its own radius in front
  (it follows at the heels and can stand close to the player: one there
  is not picked by a look past it); never the body the
  player itself plays; seats' avatars standing are not (no conversation).
  Its Dialog runs its dialogue hook as any creature's (K1's
  `k_hen_dialogue01` through `k_ai_master`, TSL's `k_hen_kreiadlg`, …),
  which begins its own conversation (`k_hcar_dialog`, `kreia`, …).
- **The block** (`ui::screens::hud`, K2's `mipc28x6_p`): fixed at the top
  centre, the target's name, a creature's vitality, up to three of what
  may be done to it with KOTOR's icons (`i_opendoor`, `i_openplace`,
  `i_useplace`, `i_dialog`, `isk_security`), the chosen lit and spelt out
  beneath (Open 365, Use 366, Dialog 371, Security 255). The mouse wheel
  chooses. On a pad, the D-pad's left and right choose, on every
  object alike (a mine's Disarm and Recover too). While a pad plays and
  the target offers more than one action, those two buttons are drawn
  beside the first and the last slot (`hud::cycle_badges`). A locked door or container: Open (its key, else its
  fail-to-open script and "Locked"), Security, Security with the best
  spike carried (owner, 2026-10-09: Open is the default; Security only
  for a body with ranks in it and not on a lock only its key opens, as
  K1). E on Open where Security is offered (the lock refuses it) moves the
  choice to Security, so the next E picks the lock: the games' lessons ask
  for a click on the Security icon, which play's crosshair has no cursor
  for (#231); an open door: Use (its `OnClick`), and before it Close (1582, the
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
  opened, a creature turned hostile). A walk that comes no nearer for a
  walker's stall while within the whole reach does the choice there: a
  shopkeeper behind a counter no way leads round (Greeta Holda in
  Anchorhead's Czerka office, talked to from 3.4 m; issue 204). 10 s of
  the sim's time or three stalls short of the reach: "out of reach". The `USE` bit is scripted `--moves … use`'s.
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

KOTOR names the area a door or exit leads to while the player faces it:

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
A save hosted holds the seats chosen (its guests take its companions;
`kjk host --load <id>`). A guest keeps a copy of every save its host
makes, listed as
"<name> (from <host>)"; another build's copy is refused, with why.

The fall (`docs/FALLING.md`): a revive shows in the target
block (`TargetView::progress`, above); the whole party down holds the
lost box (`lost`, on `confirm`) open on every seat, from the snapshot's
`lost` (`ui_view::fall`): the host's Load raises `Intent::ReloadLatest`
(the host's own latest save, never a copy, loaded in place for everyone;
`ui reload`; one that does not read is told over the box), a guest's box
has no button.

Waiting: screenshots in the save list.
