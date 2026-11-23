# Player-Facing Text Guidelines

This document defines how player-facing descriptions in `kjk` (`data/rules/*.toml` `summary` fields, in-game tooltips, and UI text) should be written.

**Core principle:** Write for a player glancing at a tooltip during a game, not a spec lawyer auditing the simulation. Every sentence should answer *"what does this do for me?"* in the fewest, clearest words possible.

---

## No-Nos ("Claudisms") & How to Fix Them

### 1. Inventing intermediate jargon to stay "technically exact"
- **No-no:** Coining a fake game term just to connect two separate rules, then repeating it in every sentence.
  - *Bad:* "The power counts only damage up to a limit for each hit." → "Force Aura prevents 40% of the damage it counts."
- **Instead:** State the normal case in plain words, and state the edge case separately.
  - *Good:* "Reduces incoming damage by 40% (Force Aura), 60% (Force Shield), or 80% (Force Armor)."
  - *Edge case (separate):* "On a single hit, protection only applies to damage up to 100% of your max Vitality (Force Aura), 200% (Force Shield), or 400% (Force Armor). Any damage beyond that goes straight through."

### 2. Translating code/math literally instead of explaining the effect
- **No-no:** Reading a formula line by line into English (especially percentages of percentages).
  - *Bad:* "For each 1% of maximum vitality points that the power counts, the Jedi loses part of 1% of maximum Force points. Force Aura uses up 100% of that 1%, Force Shield uses up 50%, and Force Armor uses up 25%."
- **Instead:** Describe what happens to the player's bars in one mental step. If the ratio is still subtle, add a short concrete example.
  - *Good:* "Blocking a hit drains your Force bar in proportion to the hit's share of your max Vitality: 100% of that share with Force Aura, 50% with Force Shield, and 25% with Force Armor."

### 3. Leaking engine internals and AI rules into player tooltips
- **No-no:** Putting fallback constants, implementation notes, or enemy AI behavior in a player-facing `summary`.
  - *Bad:* "Any other toggled power without a duration of its own lasts as long."
  - *Bad:* "Where no scripts run, a creature may use a protective power on itself when that power is not already active. It has a chance of 0.3 to do so each time it looks for enemies."
- **Instead:** Keep `summary` strictly about what happens when *you* use the power. Move engine fallbacks and rationale to `detail` (which appears in `docs/RULEBOOK.md`, not in-game), and keep AI rules off player power descriptions.

### 4. Meta-commentary and self-repetition
- **No-no:** Talking *about* the description inside the description, or repeating the same number in both a section `summary` and its child rule `summary`.
  - *Bad:* "The power's description lists its bonus to Defense and saving throws."
  - *Bad:* Stating "lasts 20 seconds" in `[protect]` and again in `[protect.length_ms]`, or "+2 / +4 / +6" in `[protect]` and again in `[protect.bonus]`.
- **Instead:** Say each fact once. Cut sentences that point to another paragraph in the same tooltip.

### 5. Robotic "plain-language" overcorrection
- **No-no:** Over-formal, repetitive third-person templates (*"The Jedi..."*, *"Each time X, Y. In exchange, Z."*, repeating the same verb three times for three ranks).
  - *Bad:* "Each time the Jedi takes damage to vitality points, the power prevents part of it. In exchange, the Jedi loses Force points."
  - *Bad:* "Force Aura prevents 40%... Force Shield prevents 60%, and Force Armor prevents 80%."
- **Instead:** Use direct player voice (*"you / your"*) and compact, scannable RPG phrasing.
  - *Good:* "Absorbs part of incoming damage at the cost of Force points."
  - *Good:* "Reduces incoming damage by 40% (Force Aura), 60% (Force Shield), or 80% (Force Armor)."

### 6. Burying the headline
- **No-no:** Leading with classification or limits (*"These are toggled powers..."*, *"The power counts only damage up to a limit..."*) before saying what the power actually does.
- **Instead:** Lead with the core effect (what it gives you), then the cost, then restrictions or caps.

### 7. Clunky third-person ("his or her", "No grip can seize the Jedi")
- **No-no:** Legalistic gender-neutral third-person phrasing or dramatic prose instead of direct second-person RPG text.
  - *Bad:* "The Jedi regains 6% of his or her maximum Force points..."
  - *Bad:* "No grip can seize the Jedi."
- **Instead:** Use *"you / your"* and plain verbs.
  - *Good:* "Restores 6% of your max Force..."
  - *Good:* "You cannot be gripped while active."

### 8. Ambiguous trailing qualifiers ("...for each rank of the power")
- **No-no:** Tacking a scaling clause onto the end of a sentence when two different powers are mentioned (yours vs. the attacker's).
  - *Bad:* "The Jedi regains 6% of his or her maximum Force points from each push or pull, for each rank of the power." *(Which power — the push/pull or your Resistance/Immunity?)*
- **Instead:** State the scaling explicitly in terms of the player's power.
  - *Good:* "Restores 6% of your max Force when hit by a push or pull (doubled with Force Immunity)."

### 9. Cross-referencing `RULEBOOK.md` headings inside in-game tooltips
- **No-no:** Writing *"as described under Saving Throws and Force Resistance"* in a `summary` that shows up in-game where no such heading exists.
- **Instead:** State the fact briefly in `summary`, and put any rulebook cross-reference in `detail` (which only renders in `docs/RULEBOOK.md`).

### 10. Stating zero bonuses
- **No-no:** Explicitly listing a tier that gives nothing, just to mirror an array like `[0, 1, 2]`.
  - *Bad:* "Knight Speed grants 1 extra attack each round, and Master Speed grants 2. Burst of Speed grants 0."
- **Instead:** Only mention the tiers that actually grant the bonus.
  - *Good:* "Knight Speed adds 1 extra attack per round, and Master Speed adds 2."

### 11. Dumping raw physics / vector math into player tooltips
- **No-no:** Explaining dot-product thresholds (`0.6`, where `1` is a line and `0` is a hemisphere) or spamming m/s throw velocity, falloff, minimum shove, and upward lift constants into a power's tooltip.
  - *Bad:* "The number 0.6 sets how wide that area is. A value of 1 is a straight line, and a value of 0 covers everything in front of the Jedi."
  - *Bad:* Four separate tooltip paragraphs for base throw speed (`6.4 m/s`), distance falloff (`0.7 m/s` per meter), min speed (`0.4 m/s`), and lift (`3.2 m/s`).
- **Instead:** Summarize the feel in one plain sentence in `summary` (*"Throws enemies backward and off their feet; closer targets are thrown harder."*), and keep the tuning constants in `docs/RULEBOOK.md` (no `applies` tag on raw physics tuning rules) or in `detail`.

### 12. Splitting a simple damage range across multiple additive sentences
- **No-no:** Turning a base + bonus roll into multiple separate tooltip paragraphs (*"deals 1%..."* followed by *"can deal up to 1% more..."*).
- **Instead:** Combine them into one compact phrase in the section `summary` (*"dealing 1% (plus up to 1% extra) of the target's max Vitality per tick"*) and drop `applies` on the split child rules.

### 13. Defining standard RPG words the long way around
- **No-no:** Writing *"These two powers of KOTOR 2 are always in effect, and a character never uses them"* instead of *"Passive"*.
- **Instead:** Use the standard one-word RPG term (*"Passive."*).

### 14. Exposing raw internal math representations
- **No-no:** Exposing squared distances or internal storage formats in player text.
  - *Bad:* "They also end when the target sees an enemy within the square root of 10 meters."
  - *Bad:* "The game compares the square of the distance to the enemy with 10 m²."
- **Instead:** Give the human-readable number in `summary` (*"within about 3.2 m"*), and put the squared-distance implementation note in `detail`.

### 15. Tagging NWScript / engine-only effects with `kind:force`
- **No-no:** Giving internal script effects (`EffectHaste`, `EffectKnockdown`, `EffectDamageShield`) `applies = ["kind:force"]`, which dumps 3–4 paragraphs of NWScript internals onto every Force power tooltip in the game.
- **Instead:** Leave `applies` off engine-script-only sections so they stay in `docs/RULEBOOK.md` without polluting player tooltips.

### 16. Turning one mechanic into 10 separate tooltip paragraphs
- **No-no:** Putting `applies` on every sub-constant of a single mechanic (e.g., 10 separate child rules for crowd-control diminishing returns, or 18 for Force Grip), repeating the section overview sentence by sentence.
- **Instead:** Put all the player-relevant numbers cleanly in the section `summary` once, and omit `applies` on redundant child rules (they still appear in `docs/RULEBOOK.md`).

### 17. Showing raw milliseconds (`1500 ms`) instead of seconds (`1.5 seconds`)
- **No-no:** Using `{...:bare} ms` in player-facing text (*"plus 1500 ms"*).
- **Instead:** Use `{...:s}` (*"plus 1.5 seconds"*).

---

## Quick Checklist Before Saving Any `summary`

1. **Would a player understand this in 2 seconds?**
2. **Is any jargon invented just to bridge two rules?** If so, drop it and separate the normal case from the edge case.
3. **Is any math phrased as a literal formula ("X% of Y%")?** Rephrase in terms of what happens to the player's bar/health/damage.
4. **Is there any engine fallback or AI behavior in `summary`?** Move it to `detail` or remove the player-facing `applies` tag.
5. **Is any number or fact stated twice across parent and child entries?** Say it once.
6. **Does it use "you / your" and compact phrasing instead of stiff "the Jedi..." templates?**
