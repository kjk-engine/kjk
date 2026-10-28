# Data formats (`fmt_odyssey`)

Pure parsers over `&[u8]`; no file I/O. Containers are in [`VFS.md`](VFS.md).

| format | module | writes back | checked on |
|---|---|---|---|
| GFF V3.2 (are, git, ifo, dlg, gui, ut*, …) | `gff`, `gff_write`, `gff_audit` | yes | every copy in K1 and TSL |
| 2DA V2.b | `twoda` | yes | every copy |
| TLK V3.0 | `tlk` | no | `dialog.tlk` of both games |
| SSF V1.1 | `ssf` | yes | every copy |
| TPC, TGA, TXI | `texture`, `txi`, `dxt` | no | every copy; PNG export |
| MDL/MDX | `mdl` (reader), `asset_odyssey::model` (IR) | no | every copy parsed and converted |
| LYT, VIS | `lyt`, `vis` | no | every copy |
| WOK, PWK, DWK (walkmeshes) | `bwm` | no | every copy |
| LIP | `lip` | no | every copy |

```bash
kjk formats k1                             # parse, write back, compare; exit 1 on failure or loss
kjk show k1 m01aa.are --module end_m01aa   # GFF as a tree, strings looked up in dialog.tlk
kjk show k1 appearance.2da                 # 2DA as rows
kjk show k1 c_bantha.mdl                   # model: animations, node tree, sample vertices
kjk texture k1 lda_bark04                  # texture to PNG (tpc first, then tga)
```

## What "writes back" means

`formats` sorts every write-back into:

- **identical**: the same bytes;
- **other layout**: a different order, same tree (the file came from a
  different BioWare tool, e.g. dialogues with `SoundExists` appended last);
- **irregular**: the file holds something a tree cannot represent, counted
  by `gff_audit` (unreached bytes, repeated labels, shared records);
- **lossy**: reading our output back gives a different tree. Always a bug.

The GFF writer uses BioWare's write order, measured from the games' files:
depth first; children written immediately; a struct's field-index block
reserved at full size on its second field; labels by first use.

## Reading rules the references follow

- GFF: when a struct repeats a label, `get` returns the last one (xoreos).
  No stock file has a repeated label with different values.
- 2DA: tab or NUL separate names (xoreos); `****` and `""` both mean "no
  value"; column names match ignoring case (xoreos).
- TLK: text only when flag bit 0 is set (both references).
- Strings are bytes in the game's code page; nothing decodes them yet.
- TPC rows run bottom to top; textures upload in stored order with UVs as is.

## Malformed files

A broken or modded file is an error, never an abort, hang or panic:

- counts read from a file reserve at most what the file has room for;
- MDL node and AABB trees may read their headers at most four times the
  file's size, and GFF structs and fields at most four times their record
  count, so a file whose lists name a shared child twice per level fails
  instead of expanding exponentially (a tree reads each record once);
- a TXI flip-book needs `numx` and `numy` within the image's width and
  height, so a texture always has at least one layer;
- an MDL mesh whose faces name a vertex it lacks is refused
  (`Refusal::FaceOutsideVertices`); the rest of the model is drawn;
- in `fmt_ja`, a GLA needs at least one frame and every bone's parent chain
  must end at a root, and `{ … }` blocks nest at most 64 deep.
