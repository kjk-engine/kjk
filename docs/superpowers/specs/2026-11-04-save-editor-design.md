# kjk save editor: design

2026-11-04. A desktop app for opening a kjk save, changing it and writing it
back. It's in the spirit of KSE (the KotOR Savegame Editor), but it's a new
program in the kjk repo (MIT), written in Rust on kjk's own crates, and
shipped in the same package as the game.

## Why

kjk players want to:

1. **Experiment:** give a character a different weapon, a modded item or a
   Force power, load the save in kjk and see whether it works (whether the
   model shows, the saber swings, the power fires), then try something else.
2. **Fix a save to keep playing:** a quest is stuck, a companion is missing
   or broken, or gear was lost.

KSE does this for the original games' `.sav` files. kjk saves are kjk's own
format (`save.kjkv` + `meta.json`, see `crates/saves`), so KSE can't open
them, and its code wouldn't help even if the license allowed it.

## Goals

- **Easy, self-explanatory, clean, smooth** (the owner's words). Someone who
  has never seen it can open a save and give their character a lightsaber
  without reading anything.
- **Finds everything by itself.** On launch it lists the saves and knows the
  game installs, with no setup.
- **Any path can be given instead, and it gets checked.** For a saves folder
  or a game install the user picks, the app says in plain words what it
  found ("kjk saves folder, 12 saves"; "TSL install, override has 34 items";
  "not a saves folder: no save.kjkv inside"). An invalid path is never
  accepted silently.
- **Warns when it can't read a save, and says why.** "Made by a newer kjk
  (format 74), this editor reads up to 71: use the editor that came with
  that kjk" or "damaged:
  <details>". It never half-opens a save.
- **Edits, covering what KSE covers:**
  - Inventory and equipment per character: add, remove, equip, change stack
    counts. This includes modded items from the install's `override/`,
    marked as mods.
  - Force powers and feats per character.
  - Attributes, skills, level/XP, HP/FP, alignment.
  - Party: who's in it, who's available, repair a companion (missing,
    broken gear, wrong state).
  - Credits, the party's XP.
  - Quests: the journal by quest name (stage and done), and the story
    flags behind them (globals) by name, with the raw values editable.
- **Better than KSE:**
  - Search everywhere, by real name ("saber", "Force Lightning", a mod's
    item name).
  - Real names, icons and stats from the game data, never bare resrefs
    (the resref is shown as secondary text).
  - All pending changes listed before saving, with undo.
  - The original save is always kept as a backup before writing.
  - Warnings for edits the game's rules wouldn't allow (a power without the
    levels or prerequisites for it, an item the class can't use). They warn
    but don't block, since experimenting is the point.
- **K1 and TSL saves both work.**
- **Native on Mac, Windows, Linux and the Steam Deck**, in the same release
  package as the game: `kjk-save-editor(.exe)` next to `kjk(.exe)`, and on
  macOS `kjk Save Editor.app` next to `kjk.app`.

## Non-goals

- Editing the original games' `.sav` files. That's KSE's job.
- Editing game data or making mods (items, models, 2DAs). The editor uses
  what's installed, it doesn't author it.
- Running inside kjk, or an in-game console.
- Making the world consistent after story-flag edits. If you set a global,
  you get what the scripts do with it. The quest view shows names, and the
  raw view is marked "advanced".

## Anti-goals (failure even if it works)

- **Breaking a save silently.** Writing something kjk can't load, or that
  loads wrong without saying so. Every write must round-trip (see the
  constraints).
- **Guessing.** Opening a save it only partly understands, accepting a path
  it didn't check, or showing an item under a made-up name.
- **Copying KSE.** No code, names, artwork or text from KSE (GPLv3). The
  editor is written from kjk's own types and the KOTOR format knowledge
  already in `fmt_odyssey`. Its features are ideas and are free to share.
  Nobody working on it reads KSE's source. The app isn't called KSE.
- **Feeling like a 2005 tool:** walls of tabs and tiny fields, raw codes,
  modal error boxes in jargon.

## Constraints

- **License:** MIT, matching kjk.
- **Same repo, same package** (the owner's choice, 2026-11-04). The edit API
  is `crates/edit`, and the app is `crates/save_editor` (binary
  `kjk-save-editor`). `cargo xtask dist` builds and packs it with `kjk`, and
  the release check starts it once, as it does `kjk --help`. The editor in a
  package is built from the same commit as the game in it, so they always
  read and write the same save format. There's no pinning.
- **Version rule:** kjk reads saves back to v0.1.0 (`sim::save::OLDEST`)
  and writes its current format, and the editor does exactly the same. A
  save made by a newer kjk is refused by name and version. Writing an older
  save upgrades it, so the result loads only in this kjk or newer. The save
  screen says so before writing.
- **Light dependencies only:** use `sim`, `saves`, `fmt_odyssey`, `vfs`,
  `rules` and `ui` (for `CharacterDesc`), never `game`, which pulls in
  Bevy's renderer. Two things the app needs live in `game` today and must
  be reproduced or moved:
  - The saves folder (`game::save_view::folder`): `KJK_SAVES`, else
    `saves` beside kjk's settings folder (`game::settings::folder`).
  - Install discovery is already in `vfs::discover` and can be used as it
    is.
- **Round-trip guarantee:** before writing, the editor encodes the edited
  `sim::Save`, decodes it again and compares. If that fails, nothing is
  written. Writes go through `saves` (temp folder + rename) so a crash
  never leaves half a save.
- **Backups:** before the first write to a save in a session, copy its
  folder to `<id>.backup-<timestamp>` next to it, or in an editor-owned
  folder if kjk would list that name. The builder checks which; kjk lists
  only ids it knows.
- **Game data is needed to show names, icons, stats and powers:** a K1 or
  TSL install found by `vfs::discover` or given by the user. A save from a
  game with no install found still opens, and shows resrefs plus a banner
  that says what's missing.

## Decided

- Desktop app with a window (not a CLI), in Rust with **egui/eframe**.
  This was my call and the owner didn't object. It gives one native binary
  per platform with no web stack.
- In the kjk repo, shipped in the game's package (owner, replacing the
  earlier separate-repo choice).
- Both uses, experimenting and fixing, are first-class (owner).
- Saves and installs are found automatically, a path can be given, and
  every path is checked with a plain-words result (owner).
- Saves it can't read get a warning with the reason (owner).
- **Super compact** (owner): the lists are very long everywhere, so it
  should be dense, not airy. One line per row, small type, tight spacing,
  no cards or big padding. Long lists are virtualised so they scroll
  smoothly.
- **Small in-game icons** (owner) next to every item (weapons, armour,
  gear, upgrades), feat and Force power: the same icons the game shows,
  read from the install's TPC/TGA textures at small size (about row
  height). With no icon, a blank square of the same size keeps the rows
  aligned.
- **Steam Deck: Desktop Mode only** for v1, from the Linux package. Game Mode
  with controller navigation comes later if people ask. (My call. The
  owner didn't answer this one.)

## My calls after the builder check (check these)

- **Editing needs a kjk change first.** A save stores each character's
  derived sheet (`Sheet`: BAB, saves, max HP/FP, defence, weapons, gear),
  which kjk recomputes only on a gear change (`inventory::rewear`).
  Equipping (`script/items.rs` `equip`) and turning a UTI into `ItemFacts`
  (`session::items::facts`) are `pub(crate)`. So step one is a kjk PR that
  adds a public edit API (add/remove/equip item, add/remove feat or power,
  set attributes/skills/level, party membership, credits, globals, journal)
  and recomputes derived state the way kjk's own rules do. The editor app
  builds on it (a second PR, or the same branch).
- **The editor may depend on `session`** (no Bevy). The ban is only on
  `game`.
- **Two checks before a write:** (1) encode → decode → compare, always;
  (2) when the save's game install is found (it's needed for names anyway),
  load the edited save into a headless world and step it a few ticks, as
  `kjk saves check` does. If either fails, nothing is written. Without an
  install only (1) runs, and the save screen says the deeper check was
  skipped.
- **Tests in public CI** can't use the private save corpus
  (`kjk-artifacts/save-corpus/`, git-ignored). They use saves built from
  `sim`'s fixtures and synthetic newer-format headers. The private corpus is
  for a local `--corpus` run.

## Left to the builder

- **Layout,** within "super compact". My suggestion: a narrow save list on
  the left (one line each: name, module, date, kjk version), a strip of
  party portraits along the top, then per-character pages (Equipment,
  Inventory, Powers & feats, Stats) plus Party and Quests & flags as tabs.
  Search is one field that works on every page. Rows are icon + name +
  a few short columns (count, type, a stat), and details for the selected
  row go in a side panel, not in the list.
- **How edits reach the snapshot.** Many `Snapshot`/`EntityState` fields are
  `pub`, but some are `pub(crate)`. Where the editor needs something that
  isn't public, or an edit needs kjk's own rule (equipping, adding a feat
  with its powers), add it to `crates/edit` or make the rule `pub`
  where it lives. Don't reimplement kjk's
  rules in the editor, and don't use workarounds.
- Where party membership, journal entries and globals live (`Progress`,
  `Goods`, `script::State`, the party table), and how to show them.
- The rule-warning set: start with powers/feats (prerequisites, level) and
  item use (class, proficiency).
- Icons (TPC → texture) and how much item stat detail to show.
- Co-op saves (more than one seat): open them, and show each seat's
  character. Editing guests' characters is fine.
- Tests: round-trip every save in a fixture corpus (kjk's `saves check`
  corpus, if it can be used), plus the version rule (a newer-format save is
  refused with the right message).
- Release: `xtask dist` and `release.yml` pack and check the second binary
  (and the second macOS app, signed like `kjk.app`).
