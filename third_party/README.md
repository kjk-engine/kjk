# Crates of others carried here

Each folder is a crate from crates.io, copied whole at the version named,
with its own licence files, and built in its place by `[patch.crates-io]`
in the workspace's `Cargo.toml`. Each is changed only as said here. None is
a member of the workspace, so kjk's lints and formatting do not apply to it.

- `transform-gizmo` 0.11.0 and `transform-gizmo-egui` 0.11.0
  (https://github.com/urholaukkarinen/transform-gizmo, MIT OR Apache-2.0,
  Urho Laukkarinen): the level editor's move, rotate and scale handles.
  Changed: their `epaint` and `egui` dependencies take no default features,
  and `transform-gizmo-egui`'s example dependency (`eframe`) is gone. Their
  default features pull egui's own fonts into the program, under OFL-1.1
  and the Ubuntu font licence, which kjk's licence list (`about.toml`) does
  not take; the level editor draws with the system's fonts instead. The
  licence files are the project's own, from its repository; the crates.io
  packages leave them out.

To move one to a newer version, copy the new package from cargo's registry
over its folder and make the same changes again.
