# The rulebook (`data/rules/`)

Every number the game's own rules use, and what it does, in one place. The
`rules` crate compiles these files into constants (`crates/rules/build.rs`
has the format), so:

- **the game plays by them**: the sim reads `rules::combat::damage::PACE`,
  not a number of its own. Change a value here, build, and the game plays by
  the new one. There is nothing to keep in step by hand.
- **the pages say them**: `docs/RULEBOOK.md` (`cargo xtask rulebook`), the
  wiki with the game's own data beside them (`kjk rules k1`) and the game's
  descriptions are drawn from the same book, so no page can quote a number
  the game does not play.

The numbers a game's data holds (a weapon's dice in `baseitems.2da`, a
power's cost in `spells.2da`, a feat's row) are the install's own and are
never copied here. A change to one of them is written in
`tables/<game>/<table>.toml` (below) and the game reads its tables so
changed.

## A chapter

One file a chapter: `[chapter]` (`title`, `order`, `summary`, `detail`),
then sections and rules, nested as the code's modules are:

```toml
[chapter]
title = "Sabers: blades, blocks and locks"
order = 20
summary = "How a blade cuts, what turns it, and what a lock does."

[sweep]
title = "The sweep"
summary = "A blade is traced every {sabers.sweep.step} up its length…"

[sweep.least_share]
name = "Graze floor"
type = "u32"
value = 500
unit = "‰"
range = [0, 1000]
summary = "The least share of a roll a blade that cuts a body delivers to it."
detail = "Half a whole sweep, so a graze that gets through still tells."
source = "the owner's balance, 2026-10-15"
see = "sim::combat::saber::sweep::Swing"
applies = ["kind:lightsaber", "kind:melee"]
```

- A table with a `value` is a rule; any other is a section. A rule's
  constant is its path in capitals: `sabers.sweep.least_share` is
  `rules::sabers::sweep::LEAST_SHARE`.
- `type` is the constant's Rust type exactly as the code had it (`i32`,
  `f32` or `bool` by the value when left out). Write a float as the code wrote it: the literal is
  copied, so its bits are the same.
- `summary` is for a player: what it does, in plain words, one to three
  sentences. `detail` is the how and the why. Both name other rules'
  values as `{chapter.section.rule}` (`{…:bare}` without the unit,
  `{…[2]}` an array's entry), never by retyping the number.
- `unit`: `ms`, `s`, `units` (JA's; 40 to a metre), `units/s`, `m`,
  `HP`, `FP`, `%`, `‰`, `°`, `°/s`, `×`, `points`, `ticks`, or none.
- `labels` names an array's entries (`["none", "the feat", "Improved",
  "Master"]`), `range` bounds what a playtest may set it to, `source` is
  where the number comes from (an executable, OpenJK, reone, an owner's
  call and its date), `see` the code that applies it.
- `applies` tags it for the pages about a thing: a table row by its label
  (`spell:FORCE_POWER_FORCE_PUSH`, `feat:POWER_ATTACK`, `skill:Security`,
  `baseitem:Blaster_Pistol`, `class:JediGuardian`; matched ignoring case;
  K1's label where K1 and TSL share a row) or a kind
  (`kind:weapon`, `kind:blaster`, `kind:lightsaber`, `kind:melee`,
  `kind:grenade`, `kind:medpac`, `kind:stim`, `kind:shield`, `kind:mine`,
  `kind:force`, `kind:stealth`, `kind:creature`, …).
- A section may hold no rules at all: how a system works where the
  numbers are the game data's (experience from `exptable.2da`, costs from
  `spells.2da`), named by table and column.

## In the code

- A constant the code had becomes an alias of its rule, its name kept:
  `pub const NOMINAL: f32 = rules::sabers::sweep::NOMINAL;` (one line of
  doc at most; the book says what it does). A number written inline in a
  formula becomes a rule too, read where it is used.
- Name rules by a path ending in their module (`sweep::NOMINAL`,
  `rules::sabers::sweep::NOMINAL`), never by a braced import: the approved
  test `rulebook` finds each rule's use by that name and fails on one no
  code reads.
- A comment never restates a rule's number; it names the rule.
- A literal constant left in `crates/sim` is not a rule (a table's row, a
  bit flag, a format's version, a script constant's number) and is listed
  in `data/rules/not-rules.txt` with why; the test fails on a new literal
  constant in neither place. So a rule cannot be added outside the book.

## Changing the game's own tables (`tables/<game>/<table>.toml`)

```toml
[table]
summary = "Playtest changes to K1's baseitems.2da."

[blaster_pistol]           # the row's label (the `label` column), or its number
numdice = "2"              # a cell: column = new text
why = "Pistols felt weak next to rifles (playtest, 2026-10-24)."
```

The game reads every table with these cells changed, through every path
(`vfs::Resolver`), and `kjk rules` shows each change beside the install's
value and its `why`.

## After a change

`cargo xtask rulebook` rewrites `docs/RULEBOOK.md`; the approved test
`rulebook` fails until it is current. A changed value moves the sim's hash
when the pinned run meets it (`docs/TESTS.md`).
