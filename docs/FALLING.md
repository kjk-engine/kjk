# Falling, reviving, losing (`sim::falls`; I30 P11–P13)

- **The rule** (`combat::damage::apply`): at 0 HP a party member (a seat's
  avatar, a companion, driven or not) is downed: `Life::Downed`, recorded
  in `Fighter::fallen` (when, who revives it); anyone else dies. A
  script's `EffectDeath` kills a party member too, standing or downed, past
  shields and protect; Min1HP still holds (Trask's scene lifts it). JA's
  saber-lock loss (C22, P12) downs a party member, kills a creature.
- `alive()` stays "can act": the downed press nothing, are not hit, healed
  (medpacs, Force heals [verify]) or perceived, lie as the dead do (no
  ragdoll; `game::cast`), and hold no exit (no doorway's or trigger's
  tenancy: they neither ask nor count ready).
- **Getting up** (`falls::falls`, each authority step after `hurt`): out of
  combat (`world::InCombat`) and `DOWN_MIN_MS` (2 s) down, with `RISE_HP`
  (1 HP [verify]) and JA's get-up; at once on leaving the party; standing
  in a module installed meanwhile.
- **Reviving**: a seat's acting body (its avatar, the companion it drives,
  the companion leading; AI never) holding `buttons::USE_HELD` in the
  command it acts on (none while stunned or knocked down), standing still,
  no action, a downed party member within `containers::REACH` ahead. `USE`
  alone revives nothing. It takes `revive_ms(rank)`: 3000 ms less 100 a
  rank of Treat Injury (Repair for a droid), at least 1000.
  - In play the downed member is a target offering Revive (`UI.md`): E
    walks there, asks for that body (`Action::Revive`; a notice when it is
    not down, not ahead in reach, or another revives it) and holds
    `USE_HELD` without a key (`target::Reviving`) until it stands or the
    hold ends (the player's move, jump or action; the revive breaking off).
    `--moves`' `use` holds `USE` and `USE_HELD`: the nearest.
  - Breaks off, progress lost: letting go, moving, any action button, a
    stun or knockdown, a hit landing since it began, the reviver down, the
    body out of reach. A revive under way keeps its reviver; two seats
    starting on one body in a step, the lowest wins.
  - A seat's use spent on a revive (`falls::Reviving`) uses nothing behind
    the body (prediction, running no `falls`, may for a frame). The target
    block's bar shows `Revive::done(now)` to the reviver, anyone targeting
    the body and the downed (its reviver named).
- **The loss**: every party member down is `Snapshot::lost` (sim ms),
  final until the world is replaced. Every seat gets the lost box
  (`ui::screens::lost`): the host's Load, a guest's "the host loads the
  last save". The host's Load (`ui reload`) loads its own latest save
  (never a copy) in place for every machine (`sim_host::ReloadRequest`,
  `play::from_save`); one that does not read is told over the box; with
  no save, the menu.
- **Control**: when the body the first seat leads through goes down, the
  next standing party member in the leader key's order leads
  (`world::scripts::passes_on`, as Tab would), past a companion a guest
  drives; it keeps leading once the fallen get up. The new body says one
  of its Select lines (`game::grunts`).
- **Effects**: going down and getting up strip the body's effects
  (`combat::effects::strip`), as the Aurora engines' death strips them
  (NWN keeps none through a death; its EE added "unyielding" effects for
  that): the poison and what it drained, Affliction's and Plague's sap
  and slow, Valor, Energy Resistance, the shield and stim, its own powers,
  every timed script effect. Permanent script effects stay (a plot's
  disguise or visual) [verify: whether K1's executable keeps any]. A poison
  that downed a body no longer downs it again as it gets up.
- **Getting up**: the get-up for the side the death left the body on
  (`combat::death::getup`): JA's `BOTH_GETUP4` from the back, `BOTH_GETUP3`
  from the front, as JA's knockdowns pick theirs; each death's side
  measured on `_humanoid` (`kjk rig a --moves BOTH_DEATHn --trace`). The
  view blends it in from the pose lain in (`cast::Rising`, 400 ms) and
  turns it to start where the death left the head, turning back to the
  body's facing as it stands. A KOTOR-clip body (a droid, a beast) plays
  its `getupdead` after `dead`, as KOTOR's party does. `arena down N`
  (the combat arena) knocks the seat's body out in death N and revives it
  where it lies 7 s later, to look at each.
- **Scripts**: a fall raises no `OnDeath`, records no killer, gives no XP;
  the death does. Part of a point is 1 HP (`Fighter::whole_hp`);
  `GetNearestCreature` skips the downed unless asked [verify]; the downed
  have 0 HP (`GetIsDead` [verify]), stay in their script triggers, keep
  their bodies when the party is picked again. No `Mod_OnPlrDying`/`Death`.
- **Probes**: `kjk sim probes --only falls` (17), `kjk combat duel --only
  lock_down`, `kjk gui probes`; approved test `downed_and_revive`.
  [verify] in K1: 1 HP up, the loss, medpacs.
