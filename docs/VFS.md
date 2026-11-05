# Game installs and resources (`vfs`)

`vfs` finds installs, reads their archives and resolves names. Parsers live
in `fmt_odyssey` (KEY, BIF, ERF/MOD/SAV, RIM) and `fmt_ja` (PK3) and open no
files. Installs are only ever read.

## Commands

```bash
kjk games                          # installs found, from KJK_* / .env, Steam, GOG
kjk check k1|tsl|ja                # every archive parsed; typed problems; exit 1 on any
kjk ls k1 'm01aa.*' --module end_m01aa [--shadowed]
kjk extract k1 m01aa.git --module end_m01aa   # to kjk-artifacts/extract/<game>/
kjk fingerprint [game] [--no-cache]
```

An install's folder is named by `KJK_KOTOR1`, `KJK_KOTOR2` or `KJK_JA`, in
the environment or in `.env` in the folder `kjk` runs from (the repo root;
`.env.example`); a variable wins over Steam's libraries and GOG's default
folders, and one that names something else is refused, never replaced.
`KJK_WORKSHOP=off` plays the game folder without its Steam Workshop items.

## KOTOR precedence (lowest first; highest holding a name wins)

This is the executables' order (`CExoResMan::GetKeyEntry`, read from TSL's
Mac executable; journal `context/artifacts/2026-11-05-override-precedence`).
Each source has a kind (`resolver::Rank`). A higher kind wins whatever was
added when; within one kind, the source added last wins.

1. `chitin.key` BIFs.
2. ERFs: `texturepacks/swpc_tex_gui.erf`, then the quality pack (`tpa`
   for now), `lips/global.mod`, `lips/localization.mod`, then the module's
   `lips/<m>_loc.mod`, and its `.mod` (or, without one, its `_dlg.erf`).
3. RIMs: `rims/` (mainmenu, mainmenudx, legal, legaldx, global,
   subglobaldx, miniglobaldx, globaldx, chargen, chargendx), then the
   module's `_s.rim` and `.rim`. A module with a `.mod` has neither: the
   `.mod` is the whole module.
4. `patch.erf`.
5. `override/`: its top level, then (TSL only) its subfolders one level
   down, in name order. The first file of a name wins. K1 reads the top
   level alone. Over it, each Steam Workshop item's `override/` (below).

So a mod's file in `override/` beats a module's own DLG, NCS or UTC, as in
the games. A `.txt` there is a document (a mod's readme, Aspyr's controller
settings) and is not read; the co-op fingerprint covers exactly the
override files that are (`resolver::read_from_override`). On the
unmodded installs this order picks the same file as the order kjk used
before for every name in every module (checked 2026-11-05).

## Steam Workshop items (TSL)

TSL's executable loads the Workshop items its player is subscribed to
(`LoadSteamMods`, `GetOverridePath`); K1's has no Workshop code. Each item
is a folder `steamapps/workshop/content/208580/<id>/` laid out like the
game's data folder (TSLRCM is item 485537937). `DirIndex::of_install`
indexes the installed ones in their load order, lowest first:

- **The order** is Steam's subscribed-items list, which the game asks
  for (`GetSubscribedItems`, at most 30). Steam keeps it in
  `userdata/<account>/ugc/208580_subscriptions.vdf`, numbered from 0, the
  newest subscription first; kjk takes it in that numbering, leaving out
  items disabled locally and items not installed. Which order Steam hands
  the game is not in its documentation; the list's numbering is the
  assumption. Without a list, the items are taken in id order.
- **`override/`**: every item's beats the game's, the last item's winning.
  Only its top level is read (only the folder named `OVERRIDE:` reads
  subfolders).
- **A module**: every copy of its `.mod` is read, the game folder's lowest
  and the last item's highest, and then none of its RIMs or `_dlg.erf`.
  Without one: every `_dlg.erf` the same way; the `.rim` is the last
  item's that has one (`LoadModule` searches from the last item); the
  `_s.rim` the *first* item's (`AsyncLoad` searches it from the first);
  the game folder's when no item has it. A module only an item has (nine
  of TSLRCM's) is a module of the install.
- **Lips**: the game folder's `<m>_loc.mod`, then each item's
  `<m>_loc.mod` and `localization.mod` (`AddModuleResources`).
- **`dialog.tlk`, movies and the stream folders**, file by file: the last
  item's that has it, the game folder's when none has.

Layers from an item are named after it: `kjk ls tsl --shadowed` shows
`workshop/485537937/modules/001ebo.mod` or `workshop/485537937/override`
winning. `kjk games`, the setup screen and the log list the items loaded.
The approved tests play the game folders alone (they were approved on
the games' own data) unless `KJK_WORKSHOP` is set in the environment.

Stream folders (`streammusic`, `streamsounds`, `streamwaves`/`streamvoice`,
`movies`) are separate namespaces, asked for by folder, never stacked. The
voice folder is whichever of the two the install has (K1's `streamwaves`,
TSL's `streamvoice`).
The references this was measured against are credited in `NOTICE`.

## Jedi Academy

`base/*.pk3` in the engine's sort (QUIRKS Q-0001), later archives winning;
loose files in `base/` are below every PK3. Within one PK3 the later entry of
a name wins.

## Reads

A resource whose span runs past its file's end is an error before anything
is allocated (an archive's or KEY's sizes are not trusted). A PK3 entry
inflates to at most its directory size.

## Fingerprint

BLAKE3 over every file the resolvers read (not saves, configs or exes), with
one digest per top-level folder so a mismatch names what differs. Workshop
items' files are covered under `workshop/<id>/` (`covered_in_item`), with
the items' order; co-op compares each item's world files as one group
(`workshop/<id>`, its place among the items in its digest), so a refusal
names the item that differs or moved, and the items past the eight groups
a handshake carries share `workshop/later`. An item of movies, voices or
music alone leaves the world as without it. Per-file
hashes are cached in the store's `cache/fingerprint/<game>.tsv` (`IMPORT.md`;
`kjk-artifacts/cache/fingerprint/` when the store cannot be opened) by path,
size and modification time; `--no-cache` rehashes everything. Changing what is
covered means bumping `fingerprint::VERSION`. The Workshop items did not:
their files are new names that no older fingerprint has, so an install
with items already differs from an older build's print of it, and one
without has the same print as before.
