# The look: legs, spine and head

The look of `asset_ja::angles`. The rest of the look (the spine's shares,
the motion correction, the lock breaks) is in `angles`' module docs.

The sim runs the same look on the skeleton it finds the blades with
(`sim::combat::saber::look`), as JA's server runs it: each frame from the
view's yaw, so the legs reach their aim at once (no swing carries over),
the motion correction switching at once, the head level (QUIRKS.md
Q-0082).

The detailed specification of the swings, the legs' lean toward travel and
the head's share was removed. The behaviour is specified in the clean-room
specifications kept by the maintainer and implemented in
`crates/asset_ja/src/angles.rs`.
