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

1. `chitin.key` BIFs
2. `rims/`: mainmenu, mainmenudx, legal, legaldx, global, subglobaldx,
   miniglobaldx, globaldx, chargen, chargendx
3. `texturepacks/swpc_tex_gui.erf`, then the quality pack (`tpa` for now)
4. `lips/global.mod`, `lips/localization.mod`
5. `patch.erf`
6. `override/`, top level only
7. the module: `.rim`, `_s.rim`, `.mod`, `lips/<m>_loc.mod`, `_dlg.erf`

Stream folders (`streammusic`, `streamsounds`, `streamwaves`/`streamvoice`,
`movies`) are separate namespaces, asked for by folder, never stacked. The
voice folder is whichever of the two the install has (K1's `streamwaves`,
TSL's `streamvoice`).
The references this was measured against, and where they disagree, are in the
I01 journal.

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
