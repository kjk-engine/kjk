# The block arcs

After playing it, the owner replaced the aim against the enemy's chest with the direction of the player's last camera turn, for the rule and for the aim arc alike.

Date: 2026-11-15. Status: approved by the owner. It replaces the section "The reticule" of `2026-11-15-targeted-blocks-design.md`. The blocking rule does not change.

## Why

The three marks around the crosshair are small and they jump between left, right and above. The owner wants an arc that circles the centre of the screen and slides smoothly as the player moves the mouse, and a second arc that shows the swing. The owner wants no full ring and no dot in the middle. The owner first chose to change the picture only, and the rule kept the crosshair's place against the enemy's chest. After playing that build, the owner found the chest aim confusing. The owner then chose the direction of the last camera turn as the aim, for the rule and for the aim arc alike. The rule keeps its three sides.

## Approaches

1. **Picture only.** An arc shows the player's aim as a smooth angle. The rule and its verdict stay at three sides. The owner picked this one first.
2. **Rule too.** A swing gets an exact angle and the block is perfect within some number of degrees of it. This changes the rule, its numbers, the probes and the rulebook. It is left for later.
3. **An arc that follows mouse movement.** At first this was dropped, because the arc would fade when the mouse stops while the rule needed the crosshair to stay on the side until the blow arrives. After playing approach 1, the owner chose a form of it for the rule and the arc together. The aim is the direction of the last camera turn, and it stays when the mouse stops, so the arc does not fade.

## The design

The arcs are shown while the player holds the block button, the body the player controls holds a melee weapon out in hand, the setting is on, and the rules are Advanced. No arcs are shown while the blade is put away or knocked out of the hand. They stay while the body cannot guard for a moment, as after a perfect block. The threat is chosen as before. It decides which enemy's swing the swing arc shows, and it has no effect on the aim arc.

- **The circle.** Both arcs lie on one invisible circle around the centre of the screen. No full ring is drawn. The circle's size is Claude's call: a radius of about 2.5% of the screen's height, which is about four times as wide as the old marks.
- **No dot.** In this case the crosshair's middle dot is not drawn. In every other case the crosshair is drawn as it is today.
- **The aim arc.** A short bright white arc sits on the circle at the direction of the player's last camera turn, which the targeted blocks spec describes under "The aim". After a turn up, the arc is at the top of the circle. The arc is 40 degrees long (Claude's call). Before the camera turns, no arc is drawn. While the turn points down and has no side, the arc is drawn dim.
- **The swing arc.** From the start of the threat's swing, an arc on the swing's side is drawn in the warning colour (the hostile red). It is thinner than the aim arc and the aim arc is drawn over it. Before a swing starts, no swing arc is drawn. The swing arcs cover exactly the angles that the rule gives each side. Above runs from 315 to 45 degrees, right from 45 to 135 degrees, and left from 225 to 315 degrees. The bottom quarter belongs to no side.
- **Agreement.** The swing arc changes to the confirm colour (the poisoned green) while the rule says the side of the last turn equals the swing's side. The colour follows the rule's own verdict, so the arcs can never disagree with the block.
- **A swing with no side.** All three swing arcs (left, right and above) are drawn in the confirm colour.
- **No guard.** While the block button is down but the body cannot guard, as in the knockaway after a perfect block, every swing arc is drawn dim. It shows the side of the swing but gives no red or green verdict, because a blow at that moment meets no guard.
- **A co-op guest.** The arc and the colours come from the presented snapshot, as the marks do today, so they agree with the host's verdict.

## Player text

The help sentence "Marks around the crosshair show the side." becomes "An arc around the centre of the screen shows the side." The sentence "Turn the camera so the crosshair is on the side of the enemy that the attack comes from: left, right or above." becomes "Turn the camera toward the side that the attack comes from. The sides are left, right and above." The section on targeted blocks in `docs/SABER-RULES.md` is reworded the same way.

## Constraints

- The Jedi Academy trace and the Advanced trace with the flag off stay identical to the base.
- The aim's direction and the turns of the last 200 ms are sim state, kept in the snapshot, so a co-op guest draws the host's aim.
- The arc and the rule read the side of the aim from one function (`aim::aim_side`), so they cannot drift apart.
- Every other constraint of the targeted blocks spec still applies.

## Left to the builder

- The exact line thickness and the dim colour, using the HUD's existing drawing code.
- Probes for the view: the angle of a table of camera turns, no arc before a turn, the dim arc after a turn down, and the colours for each case.
