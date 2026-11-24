# Dynamic crosshair, hitmarkers, bloom and recoil: design

2026-11-09. The owner asked for this ("recreate dynamic crosshairs similarly", after a study of
Star Wars Battlefront II (2017)). They skipped the questions, so the calls below are mine
unless marked as the owner's.
The reference notes and stills were kept out of the repo.

## Intent

The player's own blaster fire should read like a modern shooter:
- The crosshair shows how accurate the next shot is: its gap is the live spread cone.
- Its shape tells you what kind of gun you hold.
- A short mark at the centre confirms every hit, with its colour telling a plain hit, a strong
  (critical) hit and a kill apart.
- Sustained fire blooms, and each shot kicks the view a little.

Today kjk draws a static 6 px dot ([hud.rs](../../../crates/ui/src/screens/hud.rs) `crosshair`),
only while looking at something. A player's blaster has a fixed spread (0° for the pistol and
rifle). Nothing confirms a hit except the target's flinch and the red floating damage number.

## Goals

1. **Bloom (sim).** A player's blaster spread grows with each shot and shrinks when the trigger
   rests. The first shot after a rest flies true for the pistol, rifle, disruptor and bowcaster.
   Moving and being in the air widen it. Aiming a Sniper Shot narrows it.
2. **Recoil (sim).** Each shot adds an upward kick (plus a small random sideways one) to the
   view the shot and the camera use. The kick recovers on its own. The player's own view angles
   (mouse/stick) are never changed, so recoil never fights the input and leaves no drift behind.
3. **Dynamic crosshair (render).** With a blaster in hand, a reticle is drawn at the screen
   centre in a shape per blaster family. Its gap is the true projected size of the spread cone
   at the current field of view. It widens and closes smoothly.
4. **Hitmarkers (render).** For every blaster shot the player's own character lands (blaster
   shots only, the owner's decision), four short diagonal strokes flash outside the reticle:
   - white for a hit;
   - gold for a critical or automatic hit;
   - a kill adds four red ticks further out on the same diagonals;
   - a bolt a saber deflects or parries shows a short grey "turned" mark instead.
5. Every number lives in `data/rules/*.toml` (the rulebook), with `source = "kjk"` where it is
   kjk's own and not from JA or KOTOR.

## Non-goals

- No heat bar, overheating or cooling minigame. KOTOR and JA blasters have no heat.
- No crosshair colour picker or style options in this pass (the reticle is one colour, as in the
  reference). The Options screen can take one later.
- No changes to creature (AI) gunners. Their spread stays JA's `currentAim` (`combat::troop`).
- No reticle for sabers, melee or powers. They keep today's dot.
- No new hit sound in this pass. The impact sounds already play. The owner may want one later,
  and it would be a KOTOR sound (see Anti-goals).
- No damage numbers change. The floating red numbers stay as they are.

## Anti-goals (would be a failure even if it works)

- **Hand-drawn effects that look like slop.** The reticle and hitmarker are thin, crisp UI
  strokes, not glows or particles. The owner rejected drawn VFX before (memory:
  original-visuals-only). This is HUD geometry, which KOTOR also draws flat, but keep it
  minimal and crisp.
- **Copying Battlefront's art.** Take the behaviour (the gap is the spread, colour shows hit
  strength, shape shows the weapon) and draw kjk's own shapes in its blue-steel K2 palette.
- **A crosshair that lies.** The gap must be the real cone. A shot must never land outside the
  drawn gap, and the reticle must never show a spread the sim isn't using.
- **Recoil that moves the player's aim permanently** or fights the mouse (no drift after a burst).
- **Breaking determinism or co-op.** Bloom and recoil are sim state, predicted the same on every
  seat; the chain stays 29/29.

## Constraints

- Sim state is deterministic and carried in snapshots. New fields in `combat::specials::Shooting`
  change the snapshot (KJKS) and the sim hash, so:
  - follow the save-migration path (memory: save-migrations, save re-stamp);
  - bump `bumps/SIM_HASH` (merge-time pins).
- Spread randomness keeps using `SimRng::new(tick, id << 8 | shard)` as now.
- UI quads: `ui::draw::Quad` with `spin` (radians, about the rect centre). Diagonals are spun
  thin rects, and an arc is 3–5 short spun segments.
- The crosshair is placed by `anchor` like today's dot, in layout pixels, crisp at any
  resolution (memory: k2-ui-retint, "crisp").
- Projected gap in layout pixels: `r = tan(spread) / tan(vfov / 2) * (layout_height / 2)`,
  from the camera's actual vertical FOV (first person and third person differ; camera options
  change it).
- The camera's ray, not the gun, picks the target (`mind::crosshair`, `firing.rs` `aimed_at`),
  so the reticle stays at the screen centre. Recoil moves the view the ray uses, and the camera
  moves with it.

## How it works

### Sim: bloom

New state on `Shooting` (per fighter, encoded):
- `bloom: f32`, extra spread in degrees on top of the weapon's base;
- `bloom_at: u64`, sim ms of the last shot.

On each player shot (`world::firing`, the `fired` loop), in this order:
1. Settle the bloom for the time since `bloom_at`. After `rest_ms` it falls at `decay` degrees a
   second, down to 0.
2. The shot's spread is `cone = (base + bloom) * stance` (degrees, a *radius*), where:
   - `base` is today's `Fire::spread`;
   - `stance` is the product of `moving` (horizontal speed above walk), `airborne` (not on the
     ground) and `aiming` (`Shooting::aiming`, the Sniper Shot).

   The cone replaces `spec.spread` in the existing jitter. The rule "the first shard flies true"
   for multi-shard weapons stays. For single-shard weapons, the first shot after a full rest
   (bloom 0) with base 0 is exact, which is what happens today.
3. Add `per_shot` to the bloom, capped at `max`. Every shot counts, a special's too: a Rapid
   Shot burst blooms, and so does a Power Blast.

**Inside a disc, not a square (my call).** Today's jitter adds `range(-s, s)` to pitch and to
yaw separately, so a shot can land √2 × s off the line on the diagonals. That would put shots
outside a round reticle. For players, the jitter samples a point uniformly inside a disc of
radius `cone`: an angle and `sqrt(u) * cone` from the same `SimRng`. This changes the
repeater's and the flechette's pattern a little, but the hash changes anyway.

Creatures (`creature == true`) are left untouched, square jitter included.

**A pistol in each hand (my call):** one bloom and one kick, shared by both guns. They alternate,
the reticle is one, and the snapshot carries one set of fields.

**The pistol barely blooms on taps (my call).** Its 800 ms refire outlasts the rest and decay,
so single shots stay true. It blooms under Rapid Shot and dual pistols. That suits a slow,
heavy-hitting sidearm.

The render side reads the same `(base + settled bloom) * stance` from the presented snapshot,
through one pure function in `sim::combat::weapons` (`cone(fire, shooting, stance, now)`). The
reticle and the shot can't disagree.

### Sim: recoil

New state on `Shooting`: `kick: [f32; 2]`, pitch and yaw degrees (pitch negative = up, as in
`view`).
- Each shot adds `kick_up` up and a `SimRng` yaw in `±kick_side`. The total is capped at
  `kick_max`.
- It recovers toward 0 at `recover` degrees a second, starting `recover_after_ms` after the last
  shot.
- The kick is added to `ps.view` only where a shot is aimed: `mind::crosshair`'s ray and the
  `forward` used for `aimed_at`. The camera adds the same kick when it places itself
  (`play_view`), so the screen centre stays on the kicked line.
- `MotionState::view` (the player's own command angles) is never written.
- **It replaces today's screen-only kick (my call).** `play_view.rs` (~1462) already turns the
  camera up on each of the player's shots through `cast::Figure::camera_kick`. Keeping both would
  kick twice and move the screen centre off the aim line. The sim kick takes over for the
  player's blaster shots. Any other user of `camera_kick` stays.

### Numbers (first pass, all in `data/rules/weapons.toml`, tuned in play)

| Blaster | per_shot ° | max ° | rest_ms | decay °/s | kick_up ° | kick_side ° |
|---|---|---|---|---|---|---|
| Bryar pistol | 0.6 | 2.5 | 150 | 8 | 0.8 | 0.15 |
| E-11 rifle | 0.3 | 3.0 | 120 | 6 | 0.35 | 0.2 |
| Repeater | 0.12 | 2.0 | 100 | 4 | 0.2 | 0.25 |
| Bowcaster | 0.8 | 2.0 | 200 | 6 | 1.2 | 0.2 |
| Disruptor | 0 | 0 | — | — | 2.0 | 0.1 |
| DEMP2 (ion) | 0.5 | 2.0 | 150 | 6 | 0.6 | 0.15 |
| Flechette | 0.4 | 2.0 | 200 | 6 | 1.0 | 0.3 |

Shared values:
- stance multipliers: `moving` 1.5, `airborne` 2.5, `aiming` 0.25;
- `kick_max` 4°, `recover` 12°/s, `recover_after_ms` 80.

All `source = "kjk"`, with a detail line saying they follow modern shooters' feel (first shot
true, bloom on sustained fire).

### Render: the reticle

A pure function in `ui::screens::hud`, `reticle(gui, family, gap_px, color) -> Vec<Quad>`,
replaces `crosshair` when a blaster is in hand. The dot stays for everything else.
- **Family by blaster** (`Blaster` to `Family`):
  - pistol and rifle: **cross**, a 2 px dot plus 4 bars (N/E/S/W) starting `gap_px` out, 6 px long;
  - repeater: **prongs**, a dot plus 3 bars (up-left, up-right, down) at `gap_px`;
  - bowcaster: **brackets**, a dot plus two 3-segment arcs left and right;
  - disruptor: **long lines**, a dot plus thin 14 px bars, which close to just the dot while
    zoomed (Sniper Shot aiming);
  - DEMP2: **broken ring**, 4 arcs of 3 segments at `gap_px`;
  - flechette: **wide ring**, 8 short dashes at `gap_px`.
- Strokes are 2 px at 1080 layout height, white at 0.9 alpha with a 1 px dark edge at 0.5. The
  edge is a slightly bigger dark quad under each stroke, as the dot does today.
- A minimum gap of 4 px keeps the bars clear of the dot when the cone is 0.
- The gap eases toward its target over about 60 ms (render only, so it doesn't twitch at
  60 Hz). Widening is instant on a shot, so the reticle never shows less spread than the next
  shot will have.
- It shows whenever the player's own character holds a blaster and isn't in a conversation,
  menu or cutscene. Today's "only while looking at something" applies only to the dot.

### Render: hitmarkers

A small system beside `floaty_view` (same pattern: per-fighter `seen` sequence; `generation`
reset on scene swap) watches every fighter's new `Event`s with `by == own avatar` **and
`weapon == Some(Delivery::Blaster(_))`**. Only blaster shots get a mark (the owner's decision,
2026-11-09). Saber, melee, Force and grenade hits get none, so a mark always sits around a
blaster reticle.

| Outcome | Mark |
|---|---|
| `Hit` | white X |
| `Critical`, `Automatic` | gold X (`[0.95, 0.72, 0.2]`) |
| any hit after which the target is dead (`!fighter.alive()` on the same snapshot) | the X (white or gold) plus red ticks (`[0.85, 0.15, 0.1]`) further out |
| `Parried`, `Deflected` | grey X at half length |
| `Refused`, `Visual` | nothing |

- **X:** 4 strokes at 45°, from `gap_px + 6` to `gap_px + 14`. **Red ticks:** 4 strokes from
  `gap_px + 18` to `gap_px + 24`.
- Shown for 140 ms, growing 15% and fading over the last 80 ms. A new hit restarts it.
- Several hits in one step (flechette shards) are one mark, the strongest outcome wins
  (kill > critical > hit > turned).
- Co-op: each seat marks its own avatar's hits only.

## Tests

- **Bloom/recoil maths:** pure functions over a table of shot timings, one parameterised test.
  Covers the first shot exact, growth to the cap, decay after the rest, stance multipliers and
  recoil capping and recovery.
- **Reticle geometry:** one table-driven test over the 6 families. Each produces quads at the
  expected gap, and the gap formula matches `tan` at a given FOV.
- **Hitmarker classification:** a table over the outcomes, plus dead or alive afterwards.
- **"The reticle doesn't lie":** a headless probe fires 30 rifle shots and 30 flechette shots at
  a wall at a fixed distance. It compares each bolt's angle from the (kicked) aim line with the
  cone radius the render function reports on that tick. No shot may exceed it.
- The K1 chain stays 29/29 (all runs `--headless --mute`), plus clippy and `cargo test` for the
  touched crates.
- Visual evidence for the owner: a short clip of rifle hip fire with bloom and recoil, one per
  reticle family, and a kill hitmarker.

## Docs

- `docs/COMBAT.md`: bloom and recoil for player blasters, and that creatures keep JA's aim.
- `docs/UI.md`: the reticle families and the hitmarker colours.
- `docs/RULEBOOK.md` picks up the new rule rows automatically (`kjk rules`).

## Left to the builder

- Exact field names and where the stance inputs are read (speed, ground and aiming are all on
  `MotionState`/`Shooting` already).
- How the camera code gets the kick (from the presented snapshot's `Shooting`).
- Whether arcs are 3 or 5 segments, and stroke widths at very small layouts (keep them ≥ 1 px).
- The snapshot migration details, following the existing save-migration pattern.
