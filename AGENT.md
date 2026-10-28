# For agents

Workflow, journal and glossary: `CONTEXT.md`. Goals, principles and
decisions: `ROADMAP.md`. Do not touch other agents' `context/mrs/*` without being asked.

## What ships

The runtime and the documentation needed to use and develop it. Not shipped:
the journal, reference clones, dumps, and anything from a game. That is a rule
about *what* ships, not about what may be said. The games studied and the
projects read are named in `README.md` and `NOTICE`, and belong there.

## Rules that never bend

* Game install folders are read-only. Nothing writes into them.
* No game files, extracted assets or dumps are committed, in any form.
  Test fixtures are data we create ourselves.
* Machine paths live in `.env` (see `.env.example`). Nothing is hardcoded.
* No original memory addresses and no decompiler names (`FUN_…`, `DAT_…`). A
  standalone runtime resolves nothing against a retail image, so they are dead
  weight wherever they appear.
* Reference code (reone, xoreos, OpenJK; all GPL) is read, specified in our own
  words and matched. It is never copied or translated line by line. The
  project is MIT.
* Behaviour matches the reference, proven by a differential probe. A reference
  bug is reproduced, not fixed, and gets an entry in `QUIRKS.md`. Fixing one
  is a divergence the owner approves.
* Permanent tests live only in `crates/approved_tests`.
* Every number a game rule plays by lives in the rulebook
  (`data/rules/*.toml`, `data/rules/README.md`), with what it does, and the
  code reads it from there (`rules::…`); never a literal in the code, nor
  a number restated in a new comment or doc. A change to a rule is an edit
  of the book, then `cargo xtask rulebook`; the approved test `rulebook`
  fails on a stale page, an unread rule or a new literal constant in the
  sim (inline literals it cannot see: review them).

`cargo xtask publish-check` greps what the git index would commit for the
forbidden shapes; the pre-commit hook runs it. Passing it proves only that
those shapes are absent.

## Working in parallel (subagents)

The parent session splits work across agents in their own git worktrees and
merges it. What has gone wrong before, and the rule that prevents it:

* **Contract first.** When two agents' work meets (collision and movement),
  the parent commits the shared interface before either starts, and both
  code against it. Additions are fine; changing it is reported.
* **The gate before every commit**, run from the worktree root:
  `cargo fmt --all -- --check && cargo clippy -q --workspace --all-targets
  -- -D warnings`, then `cargo test --release -p approved_tests` (the
  owner-approved scenarios; `--features rig` adds the timeline jolt
  ceilings, which need the game installs). A change to what the verbs
  share with play also gets `cargo clippy -q -p launcher --all-targets
  --no-default-features -- -D warnings`: the game without the `probes`
  feature (the verbs that check and measure, and the layers' probes).
  Chain them so a failure stops the commit. Write files with LF endings: a
  CRLF edit once failed the fmt check after it was committed.
* **The sim hash and format versions are numbered at merge.** Never edit
  `KJK_SIM_HASH` or a `VERSION` number on a branch: say the change in
  `bumps/<branch>.toml` (`sim = "why"`, `[formats] snapshot = "…"`) and
  the merge pins and numbers it (`bumps/README.md`). Say so in the report.
* **Reserved numbers.** The parent hands each agent its `QUIRKS.md` numbers
  (e.g. Q-0023 to Q-0029); agents never guess the next free one.
* **Journals in the worktree** (`<worktree>/context/artifacts/…`); the
  parent copies them into the main checkout, which an isolated agent may
  not reach.
* **Never push, never merge.** Report branch, commits, evidence, decisions
  made, quirks, and what is not done.
* **A worktree's own build folder, seeded.** A new worktree's `target/`
  starts as a clone of the main checkout's (`docs/BUILD.md`), made
  before its first `cargo` command.
  Never a shared `CARGO_TARGET_DIR`: builds then wait on each other's lock.
* **Wait on notices, not loops.** A long build, test or playthrough runs in
  the background and its completion notice says when it ended; never a
  `sleep`/`pgrep` loop, nor waiting on another agent's build. Never touch
  every source to force a rebuild: cargo replays a crate's warnings.
* **Walkthroughs by what changed** (`docs/PLAYTESTS.md`): while working,
  the route the change touches, from its checkpoint (`kjk chain k1 ROUTE
  --alone`); before a merge, `kjk chain` from the first route the change
  can reach, and the whole chain to both endings after any change to the
  sim or the harness (an unchanged sim hash is no proof). A block is fixed
  by the rule that let it happen, engine or harness, with a probe, never
  by editing the route around it ("Keeping the chain robust").
* **Chains, the rules that save hours** (`docs/PLAYTESTS.md`, "Running
  the chain"):
  - **Never rerun a blocked route or chain on an unchanged build.** The
    sim is deterministic: it blocks the same way, byte for byte (the
    chain's cache answers such a rerun at once, from the run before).
    Fix the rule, rebuild, then replay that route from its checkpoint
    (`kjk chain k1 ROUTE --alone`), and the chain from it once it meets.
  - **Long chains in the background**, waiting for the completion
    notice: no `sleep`/poll loops, and no foreground command with a
    timeout over 10 minutes (the Bash tool's cap kills it, whatever
    timeout is asked).
  - **Never two chains at once on one machine**, across worktrees too:
    each costs the other up to 30% of its wall time.
  - The chain's own defaults are the ones to use: it stops at the first
    block (`--keep-going` plays on), plays each route with the game's
    chain flags (`data/routes/k1/chain.flags`: `--mighty --skill
    security=2`), and takes an identical rerun from its cache; none of
    them needs remembering.
* **Read before building.** Before a feature area, survey every rule the
  reference applies to it (all of the cgame's per-animation rules, all of
  pmove's special cases) and write the list in the journal, so behaviours
  are found once, up front, not one feature at a time.

## Before the parent merges

* A **fresh-eyes review** of the branch by a separate agent: bugs, dead or
  duplicated code, idiom, docs matching code, and closeness to reference
  code (behaviour in our words, never a translation).
* The gate, the pinned hash, the area's probes and measures rerun on the
  merged tree, and renders looked at, not just produced.
* CI read with `gh run list` / `gh run view` after pushing.
