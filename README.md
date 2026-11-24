<p align="center"><img src="docs/logo/banner.png" alt="kjk: KOTOR 1, KOTOR 2, in co-op with Jedi Academy combat"></p>

# kjk

**[Join the Discord](https://discord.gg/23bAu2wvH)**

A Rust engine (Bevy, wgpu) that plays **KOTOR** and **KOTOR II** with
**Jedi Academy**'s real-time movement, sabers and Force, plus native co-op.
KOTOR 1 comes first.

It reads the games from installs you own. **No game data is included here or
in any release.**

## How to play

1. Own and install **KOTOR 1**, **Jedi Academy** and **KOTOR II** (Steam or
   GOG copies are found automatically).
2. Download the file for your computer (macOS, Windows or Linux) from the
   [Releases](https://github.com/kjk-engine/kjk/releases) page and unzip it.
3. Open `kjk`. The first time, it finds your games and sets itself up, then
   shows the main menu.

The builds are not signed, so your computer warns you the first time:

- **macOS:** open System Settings → Privacy & Security and click "Open Anyway".
- **Windows:** click "More info", then "Run anyway".

kjk plays the games' movies itself, so you do not need to install anything
else for them. This includes high-resolution movies from mods. A movie in
a format other than the games' own (Bink 2, for example) still needs
[ffmpeg](https://ffmpeg.org/download.html). Without ffmpeg, kjk skips such
a movie.

**Movie Duels (optional).** With Movie Duels installed for Jedi Academy
(its `MD` folder beside `base/`, or its `MD_*.pk3` archives copied into
`base/` itself), kjk finds it by itself
(`KJK_JA_MOD` or `"ja.mod"` in kjk's `settings.json` names another folder).
Options → Gameplay → Sabers then offers Movie Duels' stances, and its hilts
for KOTOR's lightsabers. With the stances on, characters stand, walk and block
in Movie Duels' stances under the Advanced combat rules. The Advanced combat
rules themselves are kjk's own and need no Movie Duels. Without Movie Duels the
stances and its hilts are greyed. Its files come from your own install and are
never shipped with kjk. In co-op, a player needs Movie Duels only when the
host has "Movie Duels stances" on, and then needs the same Movie Duels stance
sets as the host ([`docs/VFS.md`](docs/VFS.md)).

### Anti-aliasing and DLSS

Options → Graphics → Advanced → Anti-Aliasing: Off, 2x, 4x and TAA. TAA
blends each frame with the frames before it, which smooths grass and hair
as well as edges.

The downloads do not have NVIDIA's DLSS yet. kjk has the code for DLAA and
for DLSS Quality, Balanced, Performance and Ultra Performance. It will be
in the Windows and Linux downloads once NVIDIA has approved the use of its
marks, which its licence asks for.

- A build from source with the `dlss` feature has DLSS today
  (`docs/BUILD.md`). It needs an NVIDIA RTX card.
- The DLSS choices are shown only when the build, the card and its driver
  offer DLSS. Elsewhere, a saved DLSS choice plays as TAA.
- The code is for NVIDIA's DLSS Super Resolution and DLAA, from the DLSS
  SDK 310.5.3. The neural rendering NVIDIA calls DLSS 5 is for RTX 50
  cards and its SDK is not public, so kjk does not have it.

(Building from source is only needed for development; see
[Development](#development).)

## Playing together

Up to three players: the host plays the main character, friends play the
companions, and the host keeps the save.

- **To host:** choose **Host** on the main menu, then start a new game or load
  a save. Your friends need your IP address, and port 29170 (UDP) must be open
  on your router, unless you are on the same network.
- **To join:** choose **Join** on the main menu, type the host's IP address,
  then your name.

In play: Z takes a break, Y says you are ready to leave an area, and N follows
the next party member while you watch. More in [`docs/COOP.md`](docs/COOP.md).

## Controls

Every key and mouse button rebinds in the options; fighting in detail is in
[`docs/COMBAT.md`](docs/COMBAT.md).

| key | does |
|---|---|
| mouse, W A S D | look, move |
| Space / C / Shift | jump / crouch or roll / walk |
| left / right mouse | attack / weapon's other mode (block, with a melee weapon, under the Advanced combat rules). With a grenade readied, throw it / put it away |
| R | weapon's other mode on a key of its own. With a lightsaber it throws the saber, and pressed with attack it starts the kata of the saber style. Under the Advanced combat rules it never kicks |
| G | kick. Under the Advanced combat rules a character kicks with any weapon. Under the Jedi Academy rules only a character in the staff style kicks |
| E, mouse wheel | use the target, choose the action |
| 1 to 9, and 0 | use the ten slots of the action bar |
| hold F | open the power wheel |
| ] / [ | right / left hand weapon |
| L | saber style |
| T | next feat attack (guns, two blades, double-bladed, fists, stun baton) |
| H / B / V | draw or holster / weapon set / Form |
| X / U | stealth / Security |
| I J M P K | inventory, journal, map, party, abilities |
| Tab | switch party leader |
| Escape | menu |
| F5 / F9 | quick save / load |
| F1 | first / third person |
| Alt+Enter | borderless / windowed |
| 1–9 | dialogue reply |

### The action bar and the power wheel

The action bar has ten slots. Keys 1 to 9 use the first nine, and key 0 uses the tenth. The HUD always shows the first six slots, and it shows a slot after the sixth only when that slot or a later slot contains something. A slot can contain a
Force power, a feat attack, mines, stealth mode, or one kind of item:
medpacs, stims, energy shields or grenades. The HUD shows each slot's icon
and key. A slot that the character cannot use at the moment is drawn grey
and faded. This happens when the character has too few Force points, has no
items of that kind left, or holds a weapon that cannot make that feat
attack. On a controller the bar has four slots. Hold LB and press A, B, X
or Y to use slots 1 to 4. The HUD shows the four slots in a cross, placed
as the four buttons are on the controller. Slot 1 (A) is at the bottom,
slot 2 (B) at the right, slot 3 (X) at the left and slot 4 (Y) at the top.

A new character's bar fills slots 1 to 4 by itself. It puts in the
character's attack powers first, then the feat attacks that fit the weapon
in hand, then grenades, then medpacs. When the character learns a new
attack power, the power goes into the first empty slot of the four. A
stronger version of a power that is already on the bar takes that power's
slot. A new feat attack goes into the first empty slot only when the
character knows no Force powers. The game changes no other slot.

Hold F to open the power wheel. On a controller, hold RB. The
wheel is one ring of entries. It lists the character's Force powers, item
kinds, mines, stealth mode and feat attacks that are not on the bar. It
leaves out the powers the player hid. An item kind that is on the bar stays
on the wheel while the character carries two or more different items of
that kind. A weaker version of a power, such as Shock for a character who
knows Force Lightning, is hidden until the player shows it on the setup
page. Point at an entry with the mouse or the right stick. When the player
lets go of F, or of RB on a controller, the character uses
that entry. After the wheel closes, the right stick does not turn the
camera until it has been back in its middle. Letting go with no entry pointed at uses nothing. As on the
bar, an entry the character cannot use at the moment is grey and faded.

The wheel lists offensive Force powers first, then defensive Force powers,
then item kinds, and last mines, stealth mode and feat attacks. A small gap
separates each group from the next. A Force power that is used on an enemy
is offensive. Every other Force power is defensive.

The ring shows up to 28 entries at once, and it grows larger as the number
of entries grows. When the character has more than 28 entries, each notch
of the mouse wheel turns the ring by one entry. On a controller, LB turns
the ring on by one entry. The ring turns
only while the wheel points at no item kind with an outer arc, because the
same controls step through that arc.

A game started with one player seat runs at 25% of its normal speed while
the wheel is open. A game started with two or more player seats never slows
down, even while a seat is still empty. The number of seats is set when the
game starts and does not change when a player joins.

A character may carry two or more different items of one kind, such as two
different stims. When the wheel points at that kind, an outer arc opens
beside it. The arc lists every item of the kind that the character carries.
The mouse wheel steps through the arc. On a controller, push the right
stick all the way towards the kind to reach the arc. The arc starts on the
item the character uses now. Turn the stick along the edge to move to the
next item. LB also steps on by one item. On the bar, hold the slot's key and turn the mouse wheel to
choose which item the slot uses. On a controller, hold the slot's buttons
and press left or right on the D-pad. A slot or a wheel entry whose kind
has two or more different items shows a stack of frames behind its icon.

Grenades are readied before they are thrown. Pressing the key of the
grenades' slot readies the grenade that the slot uses. Choosing grenades on
the wheel does the same. When the character carries two or more different
grenades, the slot readies the grenade when its key is let go, because
holding the key and turning the mouse wheel chooses another grenade.
Choosing another grenade in that way readies it at once. The character puts the weapon in hand away and holds the grenade in the right
hand. Hold the left mouse button to charge the throw, and let go of it to
throw the grenade. On a controller, use RT in the same way. The throw
reaches full strength after 0.9 seconds. After the throw, the character
draws the weapon that readying put away. A weapon that was already in its
holster stays there.

The right mouse button puts a readied grenade away without throwing it. On
a controller, LT does the same. Pressing the grenades' slot key again, or
choosing grenades on the wheel again, also puts it away. The character also
puts the grenade away when the player switches weapon sets, presses H, or
uses a Force power, a feat attack, another kind of item, mines or stealth
mode. The grenade is put away when the character runs out of grenades or
goes down, when the player takes control of another party member, and when
a conversation, a cutscene or a movie starts. The weapon stays in its
holster when H puts the grenade away, when the character goes down, and
when the player takes control of another party member. In every other case
the character draws the weapon again if readying put it away.

The **Action bar** button on the Abilities page (K) opens the action bar
setup. The setup also has its own entry in the Keymap, which has no key
until the player binds one. The setup shows the ten slots at the top, and
under them every power, feat attack and item kind the character can put on
the bar, as small icons in groups. The box on the right names the icon
under the pointer and shows its description. For an item kind, the box has
the kind's name as its title and says which item the slot uses.

- Drag an icon onto a slot to put it there.
- Drag a slot onto another slot to move it there. If the other slot is
  filled, the two slots swap.
- Drag a slot's icon off the bar to empty the slot.
- Right-click a power to hide it from the wheel, and right-click it again
  to show it. A hidden power is drawn grey on this page. On this page, grey
  means only that a power is hidden from the wheel.
- With the pointer over an icon, press a slot's key to put the icon in
  that slot.
- On a controller, press A on an icon and then A on a slot to put it
  there. X on a slot empties it. Y on a power hides it or shows it again.

### Controllers and the Steam Deck

Any pad works alongside the keys (Xbox, PlayStation, Switch Pro, the
Deck's own controls): the left stick moves as far as it is pushed, from
a slow walk to a run; the right stick looks, turning faster while held
over. Hold LB to block. Hold LB and press the right trigger for a perfect
block. Hold LB and press another button for a second layer of actions.
After LB is used with another button in this way, the character does not
block until LB is let go and held again. When the target offers more
than one action (a mine's Disarm Mine and Recover Mine, a locked door's
Open and Security), press left or right on the D-pad to
choose one. The target's panel shows those two buttons beside its
actions. Every screen runs on the D-pad, A, B and X, and the bumpers step
through the menu's tabs. The
prompts name the pad's own buttons while you play with it. Look speed,
invert, dead zone and vibration are under Options → Gameplay →
Controller. A stick that sits off its middle when it is first heard (a
flight throttle or pedals plugged in, which show up as a pad) is ignored
until it comes back to the middle, so it never turns the view or walks
on its own.

Every action rebinds under Options → Gameplay → Keymap, on its
Keyboard and Mouse tabs (any key or mouse button, Mouse 4 and 5
included) or its Controller tab (any button, or LB with another button, shown as
your pad's own buttons). A binding another action has is named first;
press it again and the two swap. Default resets the tab's device.

On the Steam Deck, add kjk to Steam as a non-Steam game (in Desktop
Mode: Games → Add a Non-Steam Game) and start it from **Game Mode**: there
Steam hands it the Deck's controls as a gamepad. Started from Desktop Mode,
Steam turns them into a keyboard and mouse.

![The controller layout](docs/controls/controller.svg)

![The Steam Deck layout](docs/controls/steam-deck.svg)

## Status

KOTOR 1 plays from the Endar Spire to the credits, light and dark side.
KOTOR II plays from Peragus to the credits as a created Jedi Exile.

- **World:** KOTOR's modules with JA's movement and camera, doors, effects, music, voice and movies.
- **Story:** the games' own scripts run on our NWScript VM: dialogue, journal, XP, party, items, stores.
- **Combat:** KOTOR's character sheets under JA's sabers, blasters and Force.
- **Screens:** KOTOR's menus and HUD in TSL's style.
- **Minigames:** pazaak, swoop racing, the Ebon Hawk's turret.
- **Saves:** from the menus, F5 / F9 quick save and load.

**Before reporting a bug, read [`KNOWN-ISSUES.md`](KNOWN-ISSUES.md).**

## Development

Building from source needs Rust (the pinned version installs itself) and is
covered in [`docs/BUILD.md`](docs/BUILD.md). Games outside Steam and GOG go in
`.env` (copy `.env.example`).

```bash
cargo run --release -p launcher            # build and open the main menu
kjk play --stage end_m01aa                 # one module (--class jedi for the Force)
kjk host --stage end_m01aa                 # host co-op from the command line
kjk join 192.0.2.20                        # join by IP, or by Steam name (source builds only)
kjk chain k1                               # the whole campaign, headless
```

| path | what |
|---|---|
| `crates/` | the engine, one crate per part (`sim`, `collision`, `script_ncs`, `net`, `ui`, `game`, `launcher`, …) |
| `data/` | our own data: rules, clip brushes, routes |
| `docs/` | how each part works ([`docs/INDEX.md`](docs/INDEX.md)) |
| `ROADMAP.md` | goals, decisions, what's next |
| `QUIRKS.md` | original behaviour kept on purpose |
| `KNOWN-ISSUES.md` | known bugs and unfinished parts |

Contributing: [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

MIT ([`LICENSE`](LICENSE)). No game data, ever; behaviour references are
credited in [`NOTICE`](NOTICE).

Star Wars, Knights of the Old Republic and Jedi Academy are trademarks of
their owners; kjk is not affiliated with or endorsed by them.
