# The rulebook (`data/rules/`)

The files in this folder list every number that the game's own rules use,
and explain what each number does. The `rules` crate compiles these files
into Rust constants. `crates/rules/build.rs` describes the file format.

- **The game plays by these numbers.** The simulation reads
  `rules::combat::damage::PACE` and has no number of its own. To change a
  rule, change its value here and build. The game then plays by the new
  value, and nothing else needs to be updated by hand.
- **The pages quote these numbers.** `docs/RULEBOOK.md` (written by `cargo
  xtask rulebook`), the wiki (written by `kjk rules k1`, which also shows
  the game's own data) and the in-game descriptions all read from this
  rulebook. No page can therefore quote a number that the game does not use.

Numbers that come from a game's own data are never copied here. Examples
are a weapon's damage dice in `baseitems.2da`, a Force power's cost in
`spells.2da` and the rows of a feat. To change one of those, write the
change in `tables/<game>/<table>.toml` (see below). The game then reads its
tables with that change applied.

## A chapter

Each file is one chapter. A file starts with a `[chapter]` table, which has
the keys `title`, `order`, `summary` and `detail`. Sections and rules
follow, nested in the same way as the code's modules:

```toml
[chapter]
title = "Lightsabers and Melee Weapons"
order = 20
summary = "How a blade cuts, what stops it, and what happens in a saber lock."

[sweep]
title = "Cuts"
summary = "A blade is checked for hits every {sabers.sweep.step:m} along its length…"

[sweep.least_share]
name = "Graze Damage"
type = "u32"
value = 500
unit = "‰"
range = [0, 1000]
summary = "Any cut deals at least {sabers.sweep.least_share:pct} of the damage roll."
detail = "A graze that gets through still deals noticeable damage."
source = "the owner's balance, 2026-10-15"
see = "sim::combat::saber::sweep::Swing"
applies = ["kind:lightsaber", "kind:melee"]
```

- A table that has a `value` is a rule. Any other table is a section. A
  rule's constant is its path in capital letters. For example, the rule
  `sabers.sweep.least_share` becomes the constant
  `rules::sabers::sweep::LEAST_SHARE`.
- `type` is the constant's Rust type, written exactly as the code had it
  (for example `i32`, `u64` or `f32`). When `type` is left out, the type is
  `i32`, `f32` or `bool`, depending on the value. Write a float exactly as
  the code wrote it. The literal is copied as it is, so the constant keeps
  the same bits.
- `title` names a chapter or a section, and `name` names a rule. Both are
  short noun phrases, like an entry in a game's patch notes. They contain no
  colons.
- `summary` is written for a player. It says what the rule does, in plain
  words, in one to three sentences. `detail` explains how the rule works and
  why. Both follow the writing rules in the next list.
- `summary` and `detail` never retype another rule's number. They name the
  rule as `{chapter.section.rule}` instead, and the page fills in the
  number with its unit. A suffix changes how the number is shown:
  - `{…:bare}` shows the number without its unit.
  - `{…:m}` shows a distance or a speed in `units` or `units/s` in meters.
  - `{…:s}` shows a time in `ms` in seconds.
  - `{…:pct}` shows a value in `‰` as a percentage.
  - `{…:fp}` shows a value in `hundredths FP` in Force points.
  - `{…:can}` shows a `bool` as "can" or "cannot".
  - `{…[2]}` shows one entry of an array, counted from 0.
- A key name in square brackets, such as `[Attack]` or `[Alt attack]`, is
  replaced on the page with the key that the player has bound to that
  action.
- `unit` is one of `ms`, `s`, `units` (Jedi Academy's units, about 40 to a
  meter), `units/s`, `m`, `m/s`, `HP`, `FP`, `hundredths FP`, `%`, `‰`,
  `°`, `°/s`, `×`, `points`, `count`, `ticks`, `in 10` or `in 20`. A rule
  may also have no unit.
- `labels` names the entries of an array, for example `["none", "the feat",
  "Improved", "Master"]`. `range` sets the lowest and highest value that a
  playtest may set. `source` says where the number comes from, such as an
  executable, OpenJK, reone, or an owner's decision and its date. `see`
  names the code that applies the rule.
- `applies` tags the rule for the pages about a particular thing. A tag
  names either a table row or a kind of thing. A row is named by its label,
  for example `spell:FORCE_POWER_FORCE_PUSH`, `feat:POWER_ATTACK`,
  `skill:Security`, `baseitem:Blaster_Pistol` or `class:JediGuardian`. The
  match ignores case. Where KOTOR 1 and KOTOR 2 share a row, use KOTOR 1's
  label. A kind is one of `kind:weapon`, `kind:blaster`, `kind:lightsaber`,
  `kind:melee`, `kind:grenade`, `kind:medpac`, `kind:stim`, `kind:shield`,
  `kind:mine`, `kind:force`, `kind:stealth`, `kind:creature` and others.
- A section may contain no rules at all. Such a section explains a system
  whose numbers all come from the game's data, and names the table and the
  column. Examples are experience points from `exptable.2da` and Force
  power costs from `spells.2da`.

### Writing rules for `title`, `name`, `summary` and `detail`

- Write full, ordinary sentences with a subject, a verb and an object. Put
  one fact in each sentence.
- Say who does what: "the character", "the attacker", "the enemy", "the
  player".
- Give the actual number, through its placeholder, or the actual list. Do
  not write "less" or "weapons" when a number or a list is meant.
- Define a term the first time it appears, or replace it with a common
  word. Use game words such as attack, hit, damage, penalty, Defense, Force
  points, round, second and meter. Do not use engine jargon.
- Do not join two ideas with a colon, a semicolon or a dash. Write two
  sentences instead.
- Do not use abstract words that hide what happens. Avoid "gives", "pays",
  "holds", "carries", "earns", "buys", "lands", "owns", "drives" and
  "feeds". Avoid "pace", "beat", "budget", "weight" and "share" when they
  stand for a number or a mechanism.
- `summary` describes the effect that the player sees. The mechanism and
  the reason for the rule go in `detail`.

## In the code

- When the code had a constant, that constant becomes an alias of its rule
  and keeps its name, for example
  `pub const NOMINAL: f32 = rules::sabers::sweep::NOMINAL;`. Give the alias
  at most one line of doc comment, because the rulebook explains what the
  rule does. A number written directly inside a formula also becomes a
  rule, and the formula reads the rule where it uses it.
- Refer to a rule by a path that ends in its module, such as
  `sweep::NOMINAL` or `rules::sabers::sweep::NOMINAL`. Never import a rule
  with a braced import. The approved test `rulebook` finds where each rule
  is used by that name, and it fails on a rule that no code reads.
- A code comment never repeats a rule's number. It names the rule instead.
- Some literal constants in `crates/sim` are not rules. Examples are a
  table's row number, a bit flag, a file format's version number and the
  number of a script constant. Each of these is listed in
  `data/rules/not-rules.txt` with the reason. The test fails on a new
  literal constant that is in neither the rulebook nor that list, so no one
  can add a rule outside the rulebook.

## Changing the game's own tables (`tables/<game>/<table>.toml`)

```toml
[table]
summary = "Playtest changes to K1's baseitems.2da."

[blaster_pistol]           # the row's label (the `label` column), or its number
numdice = "2"              # a cell: column = new text
why = "Pistols felt weak next to rifles (playtest, 2026-10-24)."
```

Each table is named after the row it changes, either by the row's label
(its `label` column) or by its number. Each other key is a column, and its
value is the cell's new text. `why` says why the cell changed.

The game reads every table with these cells changed, through every path
(`vfs::Resolver`). `kjk rules` shows each change next to the install's own
value and its `why`.

## After a change

Run `cargo xtask rulebook` to rewrite `docs/RULEBOOK.md`. The approved test
`rulebook` fails until that file is up to date. A changed value also
changes the simulation's hash when the pinned run uses that value (see
`docs/TESTS.md`).
