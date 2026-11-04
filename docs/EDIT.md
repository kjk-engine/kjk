# The save edit API (`crates/edit`)

What the save editor stands on (spec:
`docs/superpowers/specs/2026-11-04-save-editor-design.md`): a kjk save
opened, its characters listed, changed by kjk's own rules, and checked
before it is written. No game and no window: `edit` depends on `sim`,
`session`, `saves`, `store`, `vfs` and the format readers, never on `game`
or Bevy's renderer. Its checks are the approved test `save_edits`
(`edit::probes`, `docs/TESTS.md`).

## The one rule

`edit` never has a rule of its own. Each change calls the rule kjk plays
by, made `pub` where it lives:

| Edit | The rule it goes through |
|---|---|
| add, remove, restack an item | `script::State::make_item` (stacks as scripts' items join) |
| equip, unequip | `script::State::wear_outside`: the players' own `wear` (slot, droid or human, hand pairing, splitting a stack), the sheet dressed as a step's end dresses it; the proficiencies and use limits warned of (`Editor::equip_warning`, from `State::equip_refusal`), not held to |
| grant, remove a feat | `combat::passive::learn` / `unlearn` (Toughness's HP for every level, Force Sensitive's FP) |
| grant, remove a power | the level-up's insert into `Sheet::powers` |
| set an ability | the score, a Constitution moving the most HP as effects move them (`combat::effects::constitution_moved`: the modifier's change a level) |
| hit points | held to the most, as a Constitution lost holds them (`effects::worn`) |
| feat and power warnings | `levels::Classes::why_not_feat` / `why_not_power`, the level-up's own checks, told in words (`edit::feat_warning`, `power_warning`) |
| party membership | the NWScript routines the games' scripts call, queued (below) |

Warnings never block: experimenting is the point.

## Versions

`edit::open` reads saves from `sim::SAVE_OLDEST` to `sim::SAVE_FORMAT`. A
newer one is refused naming both numbers (`Refusal::Newer`), an older one
likewise (`Refusal::Older`), a cut or damaged one with what failed, and a
folder with no save as no save. Nothing is half-opened. Edits change no
format: what `edit` writes is what this kjk writes.

## Authority and views

A save's authority is the scripts' state (`save.script`: items, credits,
journal, XP, globals, the party table, careers) and each body's `sheet`
and `fighter` in the snapshot. The snapshot's goods and progress are views
of the state, gathered again on a step in a module with scripts. The
editor writes the authority, and shows credits, XP and the journal in the
views as set, so the views agree before that step too.

A character is a seat's player, a companion in the party (its body in the
module), or one the party table keeps away from it (`Who::Away`: its
`Kept` sheet, points and slots). Every per-character edit reaches all
three. An away companion whose creature stands in the module (made, not
joined) is edited on that body too, since joining takes it as it stands.
One with no creature is first slotted as the table keeps it
(`State::dress_as_kept`), as the creature made of it will be.

A creature its module placed holds no sheet in the snapshot until its
equipment changes. Its sheet is its module's: give the editor the save's
level (`edit::level_of`, `Editor::with_level`) to edit it, and to list it
(`characters`, `unsheeted`).

## Party changes are queued

Making a companion's creature is the engine's (`make_npc`: its sheet,
items and career as the table keeps them), and a save alone cannot do
it. So `make_available`, `join`, `leave` and `remove_available` queue the
routines the games' own scripts call (`AddAvailableNPCByTemplate`;
`SpawnAvailableNPC` beside the first player, then `AddPartyMember`;
`RemovePartyMember`; `RemoveAvailableNPC`) as a pending run of the module.
They run at the end of the first step after the save loads.

A queued run is a pending script whose name is `kjk:` and its calls
written out (`sim::script::calls::queued`). No resref holds a `:`, so none
is taken for a game's script. Its program is assembled from the level's
own routine catalogue each time it runs (`calls::read`). `pending_party`
lists the queued changes in order, for the editor's change list, and
`party_after` the table they leave. A change that table would not take
(joining twice, joining one not available, sending off one not in the
party) is refused.

## Checks before a write

- `round_trip`: the save encodes and decodes to itself. Always run.
- `load_and_step`: with an install, the save's module planned as kjk plans
  it (`session::plan`; animation lengths stood in, as the launcher's own
  module checks plan them), the save made a world (`Save::world`:
  re-stamped and reconciled), and stepped. Its queued party changes run,
  and one the party then does not show made fails the check, named. A
  module the install lacks is refused by name.

A panic in either is an error naming what it was doing, never unwound
into the editor. Writes go through `saves` (a temporary folder, then a
rename); `edit` writes no file itself.
