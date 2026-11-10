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
