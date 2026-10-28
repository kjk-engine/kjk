# The brush clip, specified (`collision::brush::clip`)

What one sweep does against one convex brush, and how the list worlds
(`BoxWorld`, `BrushWorld`) combine brushes. `Level` combines them its own
way (`COLLISION.md`: lowest fraction, then lowest brush). `ε` is
`CONTACT_GAP` (0.125 units in metres).

The per-plane heights, the classification of each plane and the inside
interval were specified here; that specification was removed. The behaviour
is specified in the clean-room specifications kept by the maintainer and
implemented in `crates/collision/src/brush.rs` (`clip`).

## List worlds (`BoxWorld`, `BrushWorld`)
Solids are tried in list order, skipping those whose contents are not in
the mask and those whose bounds the swept box does not reach
(`reaches`, Q-0020). Start-solid marks the trace and goes on. The first
all-solid solid ends the trace: fraction 0, end at the start, its contents,
surface and entity, the plane left as the best hit so far had it. A hit
replaces the best only when strictly nearer, so of equal hits the first
solid wins. At the end, a trace that stopped short ends at
`start + fraction × (end − start)`, per axis.

A `BoxWorld` solid is the brush of its six axial planes (+X, −X, +Y, −Y,
+Z, −Z); a `BrushWorld` brush is its own plane list. Both go through the
same `clip` as `Level`, so a box answers alike in all three.
