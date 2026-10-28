# `context/` and how the maintainer works

This repository is the runtime. Everything an agent needs that is *not* the
product lives under `context/`, which is ignored in full and never committed:
the journal of how each change was made, reference clones, and per-agent clones.
This file is tracked; a fresh clone has these rules and an empty `context/`.

This is the **maintainer's** workflow. Contributors do not need it; their door
is `CONTRIBUTING.md`.

Read in this order: this file → `AGENT.md` → `ROADMAP.md` → `docs/INDEX.md`.

## Layout

```
context/
  artifacts/   one folder per iteration: the journal of how we got here
  externals/   reference clones (iw4L, reone, xoreos, OpenJK). Never a source of truth
  mrs/         per-agent clones of the repo, only when agents work in parallel
```

## Glossary

* **artifact**: `context/artifacts/YYYY-MM-DD-<slug>/`, one piece of work
  (an iteration) with its slices and evidence. Knowledge that is not
  in an artifact does not exist for the next agent. Chat is not a record.
* **slice**: one focused change inside an iteration, done in one sitting, with
  a diff the owner can review in a few minutes (as a guide, under ~400 changed
  lines, excluding lockfiles).
* **evidence**: something rerunnable that makes a claim true: a command and its
  output, a screenshot, a log, a trace, an approved test. "It compiles" is
  evidence only for "it compiles".
* **external reference**: a clone under `context/externals/`. It proves nothing
  about this runtime. A value read from it and written down as measured is
  defective work.
* **differential probe**: the same inputs through a reference, run locally, and
  through our code, compared against a stated tolerance. It is how parity is
  proven (see `ROADMAP.md`, "Reference parity").
* **quirk**: a reference bug that we reproduce on purpose. Every quirk has an
  entry in `QUIRKS.md`.

## How we work: one slice at a time

1. Before an iteration starts, propose its slice list (typically 3–6, one line
   each) and wait for the owner's approval.
2. Do **one** slice: finish it, gather evidence, write the report, commit
   locally, then **stop** until the owner says "next" or corrects course.
   Never start the next slice or iteration unasked. Never push unasked.
3. Stay inside the slice. No work from later iterations, no placeholder crates
   (a crate is created by the slice that first needs it), no speculative
   abstractions. Things noticed for later go under "Notes for later".
4. When a slice turns out bigger than expected, or a problem does not yield
   after a few attempts, stop and report what was tried and found, with a
   proposed split. "Stuck, here is why" is a good result.
5. Ask instead of guessing on anything the owner decides: licensing, design
   choices and scope. One question at a time, with
   a recommendation.
6. Keep `ROADMAP.md` true. When reality changes the plan, propose the edit in the
   report, and apply it once the owner agrees.

## Artifacts

```
context/artifacts/2026-09-29-i00-foundation/
  README.md                 living surface: header, one line, current slice
  1-REPO-BIRTH-READY.md     a slice that needed no extra files
  3-EMPTY-WINDOW-FINAL/     a slice that did: README.md plus logs, screenshots
```

Every slice is `<N>-<MEANINGFUL-NAME>-<SUFFIX>`. The suffix answers one
question: **whose move is it now?**

| suffix | meaning | whose move |
|---|---|---|
| `-FINAL` | the slice goal is reached | nobody's: open the next slice |
| `-PART` | code work remains | another agent, from where this one stopped |
| `-READY` | done on the agent's side, but the next step is not code | the owner: a decision, playtest or check |

When unsure, pick the suffix that keeps the responsibility on your side:
`-PART` over `-FINAL`, `-READY` over `-FINAL`.

The artifact `README.md` carries a header and a one-line summary:

```markdown
---
status: active   # planned | active | blocked | landed | closed | superseded
updated: 2026-09-29
branch: main
next: what comes next
---
```

**The journal is not rewritten.** Earlier slice files and their evidence are
never edited. A correction is a new slice that says what the earlier one got
wrong. Only the artifact `README.md` and living maps (`ROADMAP.md`, `QUIRKS.md`,
`docs/*`) are updated.

## The slice report

Every slice ends with this, in chat and in its journal file:

```
## <iteration>.<slice> — <name> — FINAL | PART | READY

What changed:      (files and crates, one line each)
Evidence:          (commands run + results, screenshot/log paths)
Not done:          (required: gaps, shortcuts, known issues)
Probes removed:    (temporary code taken back out, or "none added")
Notes for later:   (things noticed that belong to future iterations)
Next:              (the proposed next slice, one line)
Question for you:  (only if a decision is needed; otherwise omit)
```

"Not done" is what decides the suffix. A report without it is incomplete.

## Before each commit: the scaffolding goes back out

Read the slice's own diff and ask of every added file, test and print: *what
breaks tomorrow if this is gone?* "Nothing" means it goes now:

* probe code: a `main` that prints a struct, a temporary `dump_*`, an
  `examples/` binary that answered its question, a `#[test]` written to
  eyeball one call;
* debug leftovers: `dbg!`, an unflagged `eprintln!`, commented-out code.

Permanent tests live only in `crates/approved_tests`, for scenarios the owner
approved by name. A probe worth rerunning is kept in the artifact
(`probe.rs.txt`), not in `crates/`. Its output stays in the journal as
evidence. Then run `cargo fmt` and `cargo clippy` on what you touched.

## Making a public push

A push is its own pass, done only when the owner asks:

1. Squash the unpushed research path into commits that describe the effect.
   History that is already pushed is never rewritten.
2. Tests: only `crates/approved_tests`.
3. Comments: none by default. Keep one only if, without it, the next edit
   would break something the code cannot show (an ordering, a race, a
   lifetime). Never in code: an address, a reference `file:line`, a
   decompiler name, an iteration tag, "as in the original", "used to".
   `QUIRKS.md` and `docs/` may name a reference project and file, never quote
   its code.
4. Names are our domain's names. An original identifier that means nothing
   here does not land.
5. The owner reads the diff, then `cargo xtask publish-check`, then push.

## git

`/context/` is in `.gitignore`, with no exceptions. `git add -f` is not an
escape hatch: the pre-commit hook (`.githooks/pre-commit`, installed once per
clone with `cargo xtask hooks`) runs `publish-check`, which refuses any path
under `context/` in the index.
