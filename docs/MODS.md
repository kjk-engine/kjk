# Force power and feat mods

kjk plays the game folder as you installed it. A mod that adds Force
powers or feats works the same way it works in the original game, as far
as kjk carries out what the mod's scripts ask for.

## Installing a mod

Install the mod into the game folder with HoloPatcher (or TSLPatcher),
exactly as you would for the original game. Install your mods in the order
their own instructions give. Then start kjk. There is nothing to install
or set in kjk itself.

Install the mod with the patcher, and do not copy its files by hand. Most
power mods ship their scripts as source code. The patcher compiles those
scripts with the row numbers it gave the mod's powers. Without that step
the mod's powers can be learned and shown, but casting them does nothing.

## What kjk reads from a mod

- The rows the mod added to the powers table and the feats table, with
  their names, descriptions, icons, costs, levels and prerequisites.
- The mod's scripts, as the patcher compiled them, and its talk table lines.
- The mod's icons, models, visual effects and sounds.
- Creature templates the mod changed, such as a companion who now knows a
  new power.
- Files the mod put in the Override folder and modules it put in the
  Modules folder. A module that a mod ships as a `.mod` file is read in
  place of the game's own module files.

A power or feat that a mod adds at a place in the table where the game
already has one keeps working. kjk finds the powers and feats its own rules
know by their names in the table, not by their place in it.

## KOTOR 2's powers and feats in a modded KOTOR 1

kjk adds KOTOR 2's Force powers and feats to KOTOR 1 when both games are
installed. A KOTOR 1 game with power or feat mods still gets them. They
come after the mods' own rows.

A power or feat is not added when a mod already brings one of the same
name. For example, K2 Force Powers for K1 brings its own Master Heal and
Force Scream, so KOTOR 2's are not added a second time. Each power then
appears once at level-up, as the mod made it.

K2 Force Powers for K1 names two of its rows Improved Force Barrier. The
second of them is its Master Force Barrier. kjk cannot tell from the name
that this row is Master Force Barrier, so KOTOR 2's own Master Force
Barrier is added as well. A Jedi then sees Master Force Barrier twice at
level-up.

## What does not work

- Changes to the game's executable. kjk does not run the game's
  executable, so a mod or patch that edits it has no effect.
- Two mods that replace the same script. The mod installed last wins, as
  in the original game, and the other mod's change is lost.
- A few effects that kjk does not carry out yet. High-Level Force Powers'
  Omniscience does not make every blaster bolt deflect. Its Force Pull
  damages, knocks down and pulls the enemy, but the enemy does not leap
  toward the caster.
- A mod that adds effect icons to KOTOR 1 may take the numbers of KOTOR
  2's effect icons. A KOTOR 2 power then shows the mod's icon in the list
  of effects on a character, while the power itself works.

## Playing together

Co-op needs the same mods, installed in the same order, on both
computers. kjk compares the game data of both computers, and a computer
with other mods cannot join.

## Saves

A kjk save names powers and feats by their place in kjk's own numbering.
When you install or remove a mod that adds powers or feats, a save made
before may name other powers or feats than the ones the character had.
Saves made before kjk's version of 17 November 2026 on a game with such
mods installed may do the same.
