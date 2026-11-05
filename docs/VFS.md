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
   level alone.

So a mod's file in `override/` beats a module's own DLG, NCS or UTC, as in
the games. A `.txt` there is a document (a mod's readme, Aspyr's controller
settings) and is not read; the co-op fingerprint covers exactly the
override files that are (`resolver::read_from_override`). On the
unmodded installs this order picks the same file as the order kjk used
before for every name in every module (checked 2026-11-05). Steam Workshop items are not read yet. `kjk games` names them,
and the setup screen and the log say they are not loaded.

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
one digest per top-level folder so a mismatch names what differs. Per-file
hashes are cached in the store's `cache/fingerprint/<game>.tsv` (`IMPORT.md`;
`kjk-artifacts/cache/fingerprint/` when the store cannot be opened) by path,
size and modification time; `--no-cache` rehashes everything. Changing what is
covered means bumping `fingerprint::VERSION`.
