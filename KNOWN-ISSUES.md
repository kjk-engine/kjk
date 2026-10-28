# Known issues

kjk is a work in progress. Everything below is known and on the list; there
is no need to report it. A report is still welcome when it adds something
this page does not say: a crash, a stuck story, a place where the game cannot
go on, or a case far worse than described here.

How to report anything else: [`CONTRIBUTING.md`](CONTRIBUTING.md), "Bug
reports".

## What has been played

- **Only the Endar Spire and parts of Taris have been played through
  thoroughly by hand.** The rest of KOTOR 1 (Dantooine, Tatooine, Kashyyyk,
  Manaan, Korriban, the Leviathan, the Star Forge and the rest) is checked
  headless, route by route (`kjk chain k1`), from the Endar Spire to the
  credits, both endings. That proves the story can be finished; it does not
  prove it looks or plays right. Reports from those planets are the most
  useful ones.
- **KOTOR II** loads and draws every module and its systems are in, but its
  campaign is only played as far as Peragus. Past that, expect it to break.
- **Co-op** has been played between two machines, but far less than single
  player.

## Looks and animation

- **Cutscenes are janky.** They are rebuilt on our engine from the games'
  scripts and conversations, not played back, and some still stand, move,
  face or frame wrongly: a character in the wrong spot, a stiff or missing
  animation, a camera that cuts oddly.
- **Bodies look wonky.** Characters are KOTOR's models moved by Jedi
  Academy's animations ([`docs/SKELETON.md`](docs/SKELETON.md)). The two
  skeletons do not match, so arms, hands, shoulders and hips sometimes bend or
  clip wrongly. **Female characters look the oddest**, since JA's clips were
  made for differently proportioned bodies.

## Menus and controls

- **The menus are not finished.** Inventory, item descriptions, and other
  screens are missing pieces, show placeholder text or lay out badly.
- **Choosing a Force power, stim, energy shield, mine and the like is not
  designed yet.** Today it is a set of keys (`F`, `[`, `]`, `3`, `4`, `5`,
  `O`, …; the README's "Controls"). It works, but it is a stand-in, and it
  will change.
- No gamepad scheme yet; keyboard and mouse only.

## Rules and combat

**How damage works is not final and will change.** Today it is a blend:

- KOTOR's character sheet decides how good a hit is. Your attack bonus
  against the target's defence gives a chance to hit (5% to 95%); a weapon's
  damage is its dice, bonuses, Strength (melee) and feats, with criticals,
  sneak attacks, resistances, immunities and damage reduction as KOTOR has
  them.
- JA's real-time swings and bolts decide whether a hit lands at all. When one
  does, the damage is scaled by that chance to hit (no all-or-nothing roll),
  by where it struck the body, and by a combat pace so fights last about as
  long as KOTOR's rounds would.
- Force powers keep KOTOR's saves and costs under JA's casting.

The numbers live in `data/rules/` and are listed in
[`docs/RULEBOOK.md`](docs/RULEBOOK.md); the detail is in
[`docs/COMBAT.md`](docs/COMBAT.md). Balance (fights too easy, too hard, too
long, companions too slow to join in) is expected to be off in places, and
the model itself may be reworked.

## The world

- **Collision is approximate.** Walls, floors and objects are guessed from
  the rooms' walk maps and models, not from a real collision map. Some things
  that should block you do not, and some that should not do: an invisible
  edge, a prop you walk through, a gap you snag on.

## Saves and releases

- **Saves may not survive an update.** kjk uses its own save format, and a
  new version can refuse an older save. KOTOR's original saves are not read
  (and will not be).
- **Releases are not signed**, so macOS and Windows warn on the first start
  (the README's "Install" says how to get past it). There is no release for
  Intel Macs; build from source there.
