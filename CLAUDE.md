# Writing

This applies to everything written for a person to read: chat replies, summaries, PR titles and descriptions, commit messages, code comments, docs, Discord posts, help text and memory notes.

Write the way a person writes patch notes or a game manual. Do not write in a clipped, clever or compressed style.

## The main failure: cramming

The worst habit is packing a lot of information into one short sentence. That sentence then leaves things unsaid: who does what, how much, and why. Its parts are glued together with colons, commas and clever wording.

The fix is always to use more sentences, not fewer words.
- Split it. One fact per sentence.
- Put back what was left out: the subject, the number, the list, the reason.
- If something does not fit, it goes in the next sentence or the description, not into the same line.
- Being short is good only when nothing is lost. A longer, plain text beats a short, dense one.

Example of cramming and its fix:
- Bad: "Kicks: absorbed with block held, walking, 35+ points, else floored and saber out."
- Good: "A character absorbs a kick if they are holding block, walking, and have at least 35 block points. Otherwise the kick knocks them down and turns off their lightsaber."

## Titles (PRs, commits, posts, headings)

A title names the thing. Most of the time it is a short noun phrase, like an entry in a game's patch notes.

- Name the feature or the fix. Do not describe the mechanism, list the causes, or carry exceptions.
- No colons. Never write "X: Y does Z".
- No caveats or edge cases in a title. They go in the description.
- Test: would a player write this in a patch-notes list? Could you say it out loud to someone without it sounding odd?

| Bad | Good |
|---|---|
| Props break: weapons, grenades and Force powers destroy what scripts don't need | Destructible environments and props |
| Menus look like KOTOR 2's: its text size, its check boxes, its minus arrows, and a description for each option | KOTOR 2 style menus |
| A walk to talk or use never leaves the area: C8-42 can be talked to at the Crystal Cave's mouth | Fix talking to C8-42 at the Crystal Cave |
| Loading a game in a window no longer crashes, and no screen opens over a conversation | Fix crash when loading a game in windowed mode (and a second PR or a second line for the other fix) |
| TSL's sounds come from TSL after a load: Bao-Dur grunts with his own voice | Fix TSL using K1 sounds after loading a save |

## Body text (descriptions, posts, guides, help text)

- Write full, ordinary sentences: subject, verb, object. "A character can kick with a lightsaber, any other weapon, or empty hands." Never fragments or aphorisms.
- Say who does what. Name the subject ("the character", "the attacker", "the enemy", "the player"). Do not hide it behind vague phrases like "anything in your hands", "what scripts don't need", "the view's", "pay less".
- Give the actual number or the actual list. "80% of the normal cost", not "less". "Lightsabers, blasters and grenades", not "weapons".
- Define a term the first time it appears ("A character with Master Jedi Defense, or of level 20 or higher, counts as a master.").
- One idea per sentence. If a sentence needs a colon, a semicolon or a dash to hold two ideas, make it two sentences.
- No invented shorthand, no code names, no file paths in text meant for players.
- A good model is KOTOR 2's own Force power descriptions: "An opponent targeted by this power is pushed back 5 meters, thrown to the ground stunned for 3 seconds, and suffers damage equal to the attacking character's level."

| Bad | Good |
|---|---|
| Press G to kick with anything in your hands. | Press G to kick. A character can kick with a lightsaber, any other weapon, or empty hands. |
| Masters pay less to block, and their sabers never wear. | A character with Master Jedi Defense, or of level 20 or higher, counts as a master. Masters pay 80% of the normal block cost against lightsabers and 60% against blaster bolts. Their lightsabers never wear down. |

Before sending any text a person will read, reread it and rewrite every sentence that leaves its subject, its number or its meaning unsaid.

# Roadmap

The big pieces of work are tracked as cards on a private board called "kjk roadmap". Each card is an issue in the private repo `kjk-engine/roadmap`. Only the two developers and their AI sessions can read it.

A card has one of five statuses.

| Status | Meaning |
|---|---|
| Idea | The developers have not agreed on it yet. |
| Planned | The developers agreed on it. Nobody has started. |
| In progress | Someone is working on it now. |
| On hold | Work started or was decided, and then stopped for a stated reason. |
| Done | The work is merged into main. |

## What a session does

- Before starting a large piece of work, list the cards with `gh issue list --repo kjk-engine/roadmap` and look for the card it belongs to.
- When work on a card starts, set the card's status to In progress and assign the developer who started the session.
- A pull request that finishes a card says `Closes kjk-engine/roadmap#N` in its description. The card then moves to Done when the pull request merges.
- A pull request that does only part of a card says `Part of kjk-engine/roadmap#N` instead. The card stays open.
- When work on a card stops before it is finished, set the status to On hold. Add a comment on the card with the reason, the branch name and what is left to do.
- Bug fixes and small changes do not get a card. Player bug reports stay in the issues of this repo.
- Create a new card only when a developer asks for one.
- Never copy the text of a card into a pull request, a commit message or a Discord post. This repo is public and the roadmap is private.
- If `gh` reports that it has no access to the roadmap, say so to the developer and carry on with the work.

## Setting a card's status

The board is project number 1 of the `kjk-engine` organization. The login needs the `project` scope, which `gh auth refresh -s project` adds.

```bash
# Find the card's item id on the board by its issue number.
gh project item-list 1 --owner kjk-engine --limit 200 --format json \
  -q '.items[] | select(.content.number==N) | .id'

# Set the status. Use the option id from the table below.
gh project item-edit --project-id PVT_kwDOFNBy-c4Br_63 --id ITEM_ID \
  --field-id PVTSSF_lADOFNBy-c4Br_63zhqQBKQ --single-select-option-id OPTION_ID
```

| Status | Option id |
|---|---|
| Idea | `3ee0bad6` |
| Planned | `c1a14f1f` |
| In progress | `8da6407d` |
| On hold | `005edd1d` |
| Done | `57b81c1a` |
