# Known issues

Everything here is known; no need to report it. Do report crashes, a stuck
story, or anything much worse than described ([`CONTRIBUTING.md`](CONTRIBUTING.md), "Bug reports").

## Testing

- Only the Endar Spire and parts of Taris are well tested by hand; reports from later planets help most.
- The rest of KOTOR 1 is only checked by an automated playthrough: it can be finished, but may look or play wrong.
- KOTOR II past Peragus is only checked by an automated playthrough (a created Jedi Exile, to the credits).
- Co-op is far less tested than single player.
- A co-op guest's remote droid is new: its camera does not look up or down,
  its HUD buttons have no icons yet, and its shots have no bolt or ring of
  their own. Fights the scripts end themselves that no protection covers
  (`kjk remote census k1|tsl`, the `[verify]` rows: 23 in K1, 17 in TSL)
  are checked only by `kjk chain k1 --droids chaos`; TSL past Peragus not at all.

## Looks

- Cutscenes are rebuilt, not played back, so some still move, face or frame wrongly.
- KOTOR models on JA animations don't fit well, so limbs bend and clip oddly; female characters look worst.
- A modded texture whose alpha is a shine mask but that comes without a
  `.txi` naming an env map (`envmaptexture CM_Baremetal`) looks see-through,
  as it does in KOTOR, unless it is a weapon's (weapons are drawn solid,
  `weapons_in_hand.look.solid`). Adding the `.txi` files to `override/`
  fixes the rest.

## Menus and controls

- Menus are unfinished: inventory, item descriptions and others have gaps.
- Choosing Force powers, stims, shields and mines is a stand-in key scheme and will be redesigned.
- Gamepads and the Steam Deck work, but the controller layout is a first draft; proposals for a better one are welcome.

## Combat

Damage is not final and will change. Today:

- KOTOR's stats give a chance to hit (5–95%) and a damage roll (dice, Strength, feats, crits, resistances).
- JA's swings and bolts decide if a hit lands; damage is then scaled by that chance, the body part hit, and a pace factor.
- Expect balance to be off in places. Numbers: [`docs/RULEBOOK.md`](docs/RULEBOOK.md); detail: [`docs/COMBAT.md`](docs/COMBAT.md).

## World

- Collision is guessed from the walkmesh, not a real collision map, so some things block that shouldn't and vice versa.

## Saves and releases

- Saves carry over to new versions; one that fails to load after an update is a bug worth reporting. Original KOTOR saves are never read.
- Releases are unsigned, and there is no Intel Mac build.
