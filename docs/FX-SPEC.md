# Saber ignition and lightning

Behaviour `asset_ja` reproduces. Units are Quake's (and ms).

The detailed specification of both that stood here was removed. The
behaviour is specified in the clean-room specifications kept by the
maintainer and implemented in `crates/asset_ja/src/lib.rs`
(`blade_lengths`) and `crates/asset_ja/src/fx.rs` (`fx::bolt`).

## Saber ignition (`blade_lengths`)
Evaluated once per 60 fps client frame, so every machine sees the same blade
(the game moves it per frame, whatever the frame rate: Q-0005). The renderer
draws nothing under half a unit, and a blade shorter than full is drawn
fatter (`scene`).

## Lightning (`fx::bolt`)
An electricity primitive from `start` to `end`, seen from the eye. Branches:
Q-0015 (fixed).
