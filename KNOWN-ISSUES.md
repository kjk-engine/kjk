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
- A character who holds a pistol in each hand and fires them raised at
  the ground close by brings both hands together in front of the chest.
  Seen from the camera, the arms then look crossed, or one arm lies on top
  of the other.
- A modded texture whose alpha is a shine mask looks see-through when it
  has no `.txi` naming an env map (`envmaptexture CM_Baremetal`) and
  replaces no game texture whose own TXI names one. KOTOR draws it the same
  way. A weapon's texture is drawn solid instead
  (`weapons_in_hand.look.solid`). Adding the `.txi` files to `override/`
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

Aiming a blaster with [Aim] is new. These faults are known:

- The right hand rests on top of a rifle instead of holding the rifle's grip.
- Characters animated on KOTOR's own creature animations, such as HK-47, do not raise their gun to the eye. They still aim as well as anyone else, but their gun stays where their own animation holds it.
- The head does not tilt toward the sights while a character aims.
- When the camera options set the camera's shoulder offset to its largest value, an aimed shot can land beside the crosshair instead of on it.
- When the camera options put the blaster camera very close to the character, aiming moves the camera closer still. Only the right arm and the gun then show, at the left edge of the screen.
- Pressing [Aim] while the gun hangs at the character's side draws the gun and raises it at the same time. The gun swings up across the body in about a fifth of a second.
- Jedi Academy's hit locations count a hit at the chest's aim point as an arm hit, which deals 85% of the damage. A perfectly straight shot at the chest therefore deals less damage than a shot at the waist. This is older than aiming.
- A settings file from the pre-release v0.17.1-md-rc.1 that saved Block on R leaves [Feat attack] without a key. The player has to bind [Feat attack] in the key mapping screen.

## World

- Collision is guessed from the walkmesh, not a real collision map, so some things block that shouldn't and vice versa.

## Saves and releases

- Saves carry over to new versions; one that fails to load after an update is a bug worth reporting. Original KOTOR saves are never read.
- Releases are unsigned, and there is no Intel Mac build.
