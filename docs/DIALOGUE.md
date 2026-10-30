# Conversations (I18; `sim::script::dialogue`)

A conversation is a walk through a DLG tree that the authority runs inside
the scripts' step: it asks condition scripts which lines are open, runs
each line's action script, and waits on a player's reply. Clients see it
through the snapshot and answer with a field of their command.

## The contract with the scripts

Agreed before either side builds on it (AGENT.md, "contract first").
Additions are fine; a change to anything below is reported to the other
side.

### Who owns which routines
- **Dialogue binds**: `ActionStartConversation`, `BeginConversation`,
  `GetIsInConversation`, `GetIsConversationActive`, `GetPCSpeaker`,
  `ActionPauseConversation`, `ActionResumeConversation`,
  `SpeakOneLinerConversation`, `SetCustomToken`, `BarkString`,
  `ActionBarkString`, `EventConversation`, `GetLastConversation`,
  `HoldWorldFadeInForDialog`, `SetLockOrientationInDialog`,
  `SetLockHeadFollowInDialog`, `SetDialogPlaceableCamera`,
  `CancelPostDialogCharacterSwitch`, and K2's `GetScriptParameter`,
  `GetScriptStringParameter`. The scripts' side leaves these unbound, but
  for `CancelPostDialogCharacterSwitch`, bound with nothing to cancel
  (`script::presentation`): TSL's executable, a companion's player starting
  a conversation with an object whose `PartyInteract` is off, has the
  player's own character speak and hands control back to the companion as
  it closes, unless a script cancels that; no such hand-over is made here
  [verify].
- **The scripts bind everything else**, shouts included (`SpeakString`,
  `SetListenPattern`, `GetListenPatternNumber`, `GetLastSpeaker`) and
  `ExecuteScript`.

### The seams in `sim::script`
Built by the dialogue side, each an addition; the scripts' side codes
around them and reports before changing one.
1. **A run that answers.** `Step::run` returns the script's int
   (`vm::Finished::returned`), `None` for no int, a fault or a missing
   script. Conditions call it at once, inside the step, nested like
   `ExecuteScript`.
2. **Script parameters.** `Event` carries `params`: the DLG's call (five
   ints and a string; TSL's `ActionParam…`/`Param…`), none by default.
   `Pending` does not change.
3. **The step's hook.** `run_due` calls `dialogue::advance(&mut step)` once
   a step, after pending events, deferred code and the action queues (so
   a start the queues asked for begins the same step). It starts
   conversations asked for, takes replies from `View::picks`, ends lines
   that have played.
4. **The start action.** `ActionStartConversation` queues a move within
   4 m (unless `bIgnoreStartRange`), then `Action::Converse { with, dialog }`
   (save tag 10), which asks `dialogue` to start when it reaches the front.
   `ActionResumeConversation` queues `Action::Hold` (tag 11); a pause
   takes effect at once and keeps its place in the queue too
   (`Action::Pause`): a queue cleared before it reached the front drops
   it, as the executables' queued pause never ran.
5. **"The player used me".** A player's use of a creature that has a
   `ScriptDialogue` hook (`Hook::Dialogue`; no placeable has one) queues
   `Pending { caller: it, entering: Some(player), number: -1 }` on it
   with `last[(it, SPEAKER)] = player`, as shouts do (`Engine::speak`).
   The games' own handlers (`k_def_dialogue01` → `k_ai_master`) answer
   with `BeginConversation`. One with a `Conversation` and no hook, or a
   door with one, starts it directly.
6. **The data.** `Library` carries the level's DLGs (`dialogues`, by
   lowercase resref, in its digest). `session::script_library` reads each
   object's `Conversation`, the DLG names in scripts' string constants,
   and adds every script those DLGs name (conditions, actions, end and
   abort) to the scripts it loads.
7. **State.** `script::State` gains `talk` (the conversations under way,
   custom tokens), encoded after every existing field; the save version
   moves once for it.

### What dialogue provides
- `sim::script::dialogue` (a child of `script`, for `Step`'s sake): the
  walk, the routines above, the snapshot's conversation view and the
  transcript lines (`Report::talk`). It never runs a script except
  through `Step::run`.
- `fmt_odyssey::dlg` and `fmt_odyssey::lip`: the readers.

## The data
- `fmt_odyssey::dlg::Dlg`: entries, replies, starting links; per node its
  text, speaker tag, action scripts with TSL's parameters, `Sound` and
  `VO_ResRef`, `Delay`, camera, quest and animations; per link its
  conditions (`Active`, TSL's `Active2` with `Logic`, `Not`, `Not2`) and
  `IsChild`. `Dlg::scripts` lists every script it names.
- `fmt_odyssey::lip::Lip`: a line's mouth shapes over time; `shapes_at`
  gives the two shapes around a moment and the blend between them.
- A level's library carries its conversations: the DLGs its objects speak
  (`ObjectData::conversation`, from the instance or its template), those
  its scripts name in string constants, and every script those DLGs name.
  A script a DLG names that the install lacks stays out (K1's
  `end_trask01` names `k_pend_carth11`, which no archive holds).

```bash
kjk dialogue show k1 end_m01aa              # the level's conversations
kjk dialogue show k1 end_m01aa end_trask01  # one tree, with its text
```

## The walk (`sim::script::dialogue`)
From the survey of reone and xoreos (journal `2026-10-01-i18-dialogue`);
where they part, the choice is ours and named.
- **Start.** `ActionStartConversation` (its owner by reone's rule: the
  other party when a player asks, else the asker; an empty resref the
  owner's own DLG; an asker with none, a trigger starting a scene with an
  invisible placeable, gives it to the one conversed with, with its DLG
  [verify]), `BeginConversation` (owner the caller, the player the
  one named, else its last speaker), or a player's use: a creature or
  placeable with a dialogue hook runs it with `GetListenPatternNumber` −1
  and the player as `GetLastSpeaker` (the games' own `k_def_dialogue01` →
  `k_ai_master` answer with `BeginConversation`); a placeable with an
  `OnUsed` runs it (it starts what it will: Taris's sewer console runs
  Mission's codes when she is with you, else its own); one with neither
  starts its `Conversation` (reone starts the conversation first). A door used opens (reone, xoreos: an
  opening door starts no conversation, so room 3's cutscene, which its
  `OnOpen` starts, is not refused); a locked one runs its fail-to-open
  script, then starts its `Conversation` only if the door is still
  locked and neither talks nor barked (`world::scripts::door_speaks`):
  room 3's door explains Security so (its script only brings Trask
  over), while the first door's script unlocks and opens it for Trask
  or speaks itself (reone, xoreos and K1's open-door action start no
  conversation themselves; the rule is read off the games' data) [verify
  in the game]. One conversation per owner
  and per player (the body standing in it): a second is refused, but in
  co-op a player's use of one in another player's own conversation joins
  it (below).
- **Lines.** The first starting link whose speaker is here and whose
  condition holds; none ends it at once (`EndConversation` runs). An
  entry runs its `Script` and `Script2` for the owner as it shows, and
  offers every reply whose link holds, in order, once its line has played
  (or been skipped): TSL's `CSWSDialog::SendDialogEntry` sets the line's
  delay, runs the two scripts, then `CSWSObject::SendDialogReplies`, which
  waits while `IsDialogDelay` holds before the replies' conditions run
  (`Step::offer_replies`; Ralon's E37 sets the local his [Awareness]
  reply checks) [verify in K1's exe]. Nothing is offered while a line
  plays, so no answer is taken then. A taken reply's entry is the
  first of its links, likewise. The speaker (`Step::speaker_for`, both
  executables): none named, the owner; the owner's own tag (any case), the
  owner; else the object of that tag nearest the owner within
  `dialogue.speakers.reach_m`, found in the area's order (Q-0323).
  `OWNER` and `PLAYER` are no keywords there. A link to an entry whose
  `Speaker` no one answers to (a companion not in the party: Mission's
  line in Holdan's conversation, behind `k_con_missionnpm`) is passed
  over before its condition runs, and the next link is tried; the owner
  never says such a line (reone and xoreos let the owner say it). The
  executables also remember each name's answer for the conversation's
  life; we search each time [verify].
  The listener (`Conversation::listener_now`) is the one `Listener` names
  (`OWNER` and `PLAYER` as in `AnimList`); else the previous line's
  speaker; else, at the first line or the speaker talking twice running,
  the player (`Step::player`). A line held for an interjection whose
  speaker left meanwhile is said by the owner, and a `Listener` naming
  that owner then names no one either, so the line is not said to the one
  saying it; a line naming its own speaker as its listener keeps it
  (ten K1 entries do), and the camera films that pair at one point. A
  line starts as an entry shows, as the
  replies are offered (the player says them to the one who spoke) and as
  a reply is taken (the player says it to its `Listener`, else the
  previous speaker; a silent one is no line). At each start, never in an
  `AnimatedCut` DLG, the listener turns to the speaker, then the speaker
  to the listener, at once (exe; the design's S2), before the line's
  scripts [verify the order]: a body with a head arc (`appearance.2da`
  `head_arc_h`: a creature's template's `Body::head_arc_h`, a player's by
  its persona's appearance, `Library::head_arc`) turns only by the angle
  less the arc and 1°, and not at all within the arc (its head follows,
  view side); a body on the head-follow lock list, or with no arc
  ([verify] a 0 arc read as a head that can't follow), turns fully; one
  on the lock-orientation list does not turn. Between starts nothing turns them, so a script's
  `SetFacing` holds until the next line. In every conversation, holding
  the world or not (a body turned short of its listener leaves the rest
  to its head), the pair's heads turn toward each other within their
  arcs, view side (S7, `launcher::cast`'s `Head`): eased at 10 rad/s
  [verify the exe's "10"], after the body's pose, back ahead once the
  line's pair changes; not on the head-follow lock list, nor in an
  animated cut or a staged conversation (its bodies on their stunt
  clips).
- **Conditions** run for the owner. TSL's two combine by `Logic` (0 and,
  1 or), each turned by its `Not`. A missing script, or one that returns
  no int, does not hold (xoreos; reone holds it): K1's `end_trask01`
  names `k_pend_carth11`, which no archive holds.
- **One-liners.** A start whose single reply says nothing and leads
  nowhere is a bark over the speaker, not a conversation (reone); its
  line's and its reply's scripts run as a conversation's would (Dantooine's
  courtyard droid opens the Enclave's door so). A `SpeakOneLinerConversation`
  bark only speaks. A bark lasts as long as its voice plays, else 4 s; a
  `BarkString`'s always 4 s (K1's talk table gives no line's voice a
  length) [verify]. Drawn and voiced: `UI.md`, "Barks".
- **While a conversation holds the world** (both executables' dialog
  state): `BarkString` is dropped, and `ActionStartConversation` and
  `BeginConversation` are refused where they are called, their one-liners
  with them (a locked door's line, its `OnFailToOpen` run as a scene walks
  someone into it, is never said; nothing is queued). A bark asked for as
  one, `SpeakOneLinerConversation` or an `ActionBarkString` whose turn
  comes, is shown (the bubble itself asks no dialog state). TSL's refused
  `BeginConversation` also clears its dialog state, a quirk not reproduced
  [verify]; its dialog action's own `OnDialog` may begin one, which no
  scene here reaches.
- **Timing.** A line lasts its voice's length (`Sound`, else `VO_ResRef`,
  as the install has them; reone's order); with no voice, or one under
  0.15 s, 0.1125 s a character of the text it shows (its own string, else
  its talk-table line's; developer notes and tokens counted [verify]). An
  explicit `Delay` (seconds) raises that to the `Delay`; wait flag 2 holds
  it for the voice; the least is 10 ms. In an `AnimatedCut` a line with no
  wait flags lasts nothing (a `Delay` counts as a flag, and a voiced line
  with neither has flag 2, as the games' loader gives it). Flag 1 holds it
  for its camera model's clip (the clip's length alone; the executables
  hold it to the longest of every flagged wait and its own timer [the
  clip's line is not yet raised to its timer]). Flag 8 holds it for its
  own fade (`FadeType`, never a script's): its `FadeDelay` and
  `FadeLength` from 0.1 s in, a cover or uncover at once (1, 2) its delay
  alone, a node with no fade nothing (K1's and TSL's executables alike).
  Flag 4 holds it until every `AnimList`
  participant's clip has played once (`Library::clip_ms` on its body's
  model; one whose model has no length for it holds nothing [verify], and
  an animated cut's stunt clips are not measured). Flag 8 (unknown) is not
  read, nor the conversation's `DelayEntry` and `DelayReply` [verify]. A
  skip ends a line once it has shown 250 ms (one meant for the line before
  then ends nothing). A line lasts a step at least, so what its script
  queued acts first (Trask's opening pauses the conversation while he
  walks up). `ActionPauseConversation` holds the line at once, from the
  script that calls it, so a skip or the timer cannot get past it;
  `ActionResumeConversation` is queued behind the caller's actions, so
  pausing, acting and resuming holds the line through the acting (reone's
  pause only stops skipping). The pause keeps its place in the queue all
  the same: a `ClearAllActions` before it reached the front drops it with
  the resume, so a line that pauses and clears its speaker's queue to walk
  it in (Citadel Station's Czerka men, `a_czerkaint`) goes on, not held
  for good (probe `pause_cleared_with_its_queue`). A walk-up with no way to the player (a
  player arrived past locked doors) fails at once, so the resume queued
  after it comes (`EVENTS.md`, action queues).
- **Replies.** The player may answer while the line plays (it is cut),
  not while it is paused;
  the one silent reply is taken when the line ends; none ends the
  conversation. A reply runs its scripts, then the first entry it links
  to whose condition holds; none ends it. A skip ends the line where the
  DLG is `Skippable` and the entry is not TSL's `NodeUnskippable`: TSL's
  exe hears a skip only then, so a skip on a marked line is refused and
  the line plays out (whether the ask carries to the next line
  [verify]).
- **End.** `EndConversation` for the owner; `EndConverAbort` when the
  owner or the player is gone or dead (in co-op the owner or the body
  standing in it, its listener: the host's character down does not end a
  guest's own, whose PC it is), and in co-op when it is left or cut short
  (below). A party member knocked out (`COMBAT.md`: it gets up again) is
  neither: its conversation goes on (`Thing::dead_for_good`; TSL's Atton,
  poisoned in Peragus' hangar bay, falls as the escape's `106percut`
  begins) [verify: that the exe does not abort it]. Every way out goes through `Step::end`, so its end
  script runs once. A module left behind drops its conversations without
  their scripts.
- **Who hears the end** (`Step::heard_end`). K1's exe runs, after
  EndConversation, every creature's and placeable's end script in the
  owner's area: a creature's `ScriptEndDialogu`, a placeable's
  `OnEndDialogue`, the owner's among them, in the area's order (its
  instances, then those made since), party members too. The dead (no hit
  points left) hear nothing, as they hear no heartbeat [verify: that the
  exe skips them]. A one-liner's end is heard by its owner only; one cut
  short (`EndConverAbort`) by none. A conversation with nothing to say
  (no starting link holds) ends where it starts, as a normal end the
  area hears [verify: that the exe runs the end scripts then]. The Endar
  Spire's reinforcements, made hostile in the conversation, attack as it
  ends (`k_def_endconv` → `k_ai_master`); the Leviathan's brig computer
  takes back the spikes it lent (`k_plev_resspikes`).
- **Routines bound:** `ActionStartConversation`, `BeginConversation`,
  `SpeakOneLinerConversation`, `BarkString`, `GetIsInConversation` (its
  owner, a body standing in it, its speaker; the host's character also
  in one the companion it leads stands in, as one player's always was),
  `GetIsConversationActive`, `GetPCSpeaker` (the conversation whose
  script runs, else the caller's, else the first player),
  `ActionPause`/`ResumeConversation` (the conversation whose script ran
  it, through code it deferred too, `Action::Hold::talk`,
  `Delayed::talk`, KJKV 40, where the one acting takes part in it or is
  its PC; else the one the actor takes part in), `SetCustomToken`, TSL's
  `GetScriptParameter`/`GetScriptStringParameter`,
  `HoldWorldFadeInForDialog` (`SCRIPTS.md`, "Fades"),
  `SetDialogPlaceableCamera` (the camera, below),
  `SetLockOrientationInDialog` and `SetLockHeadFollowInDialog` (an object
  put on or taken off the lock lists, `Conversation::lock_turn` and
  `lock_head`, of the conversation whose script runs, else the caller's;
  a conversation starts with both empty). A node's `Quest` moves
  the journal (`WORLD-STATE.md`, "The journal"). Not yet: the scripts'
  `bPrivateConversation` (a player's own conversation in co-op is
  another thing, below), `bUseLeader`, the ignore list.
- **Gestures** (`AnimList`, outside animated cutscenes): a shown entry's
  participants (`owner`, `player`: the listener where it is a guest
  seat's body, else the PC, as lines are said to and the camera frames;
  else by tag; one not found left out)
  play its `dialoganimations.2da` row (value − 10000) for the line's
  length, as `PlayAnimation` would (wait flag 4 holds the line for them,
  "Timing"); the row's name gives the constant
  (`Talk_Pleading` is `ANIMATION_LOOPING_TALK_PLEADING`; the library's
  `dialog_animations`), else reone's few aliases (`Victory` the first
  victory, `Scratch_Head`, `Drunk`, `Use_Computer_LP`, `Choked` the choke;
  `DIALOG_ALIASES`); `Ready` plays the wield's ready stance
  (`game::acting::stance_of`). TSL's own rows (`Nod_No`, `Shrug`, `Point`,
  …) name no constant: an `AnimList` value is TSL's animation number (10
  000 and the dialog row), which TSL's exe turns into an `animations.2da`
  row for a humanoid body (`Nod_No` row 467, `nodno`; `Stand_Attention`
  `standstill`; `Collapse` the collapsed loop `Collapselp`), the row's
  name the clip; a body of another creature type finds none and plays
  nothing. Those rows play that clip (`creature::DIALOG_CLIPS`), once or
  looped for the line as the dialog row's `looping` says (`LOOPED`).
  Replies' lists are not read: the K1 executable's line animation pass
  takes entry nodes only. A K1 value under 10000 is an `animations.2da`
  row played only where its `Dialog` column is set, else a `cut` clip by
  range (1000+ `cut%03d`, 1200+ `cut%03dw`, 1400+ `cut%03dl`, 1600+
  `cut%03dwl`, 128 each, numbered from 1), so K1's 35, 40, 41, 44, 70 and
  2000 play nothing.
- **A script's animation numbers** (`ActionPlayAnimation`, `PlayAnimation`
  on a creature; `Library::script_animation`): not the dialog numbers
  above. Both executables turn a constant through their table of
  `ANIMATION_…` values, a value it does not reach (K1's from 10000, a
  negative one, TSL's 10000 itself) taking its default,
  `ANIMATION_LOOPING_PAUSE`. TSL's from 10001 skip the table: 10000 and an
  `animations.2da` row, which its client passes on unchanged and names by
  the row's `name` on the body's chain (Kreia's 10316 `animloop01`, the
  boma beasts' 10263 `ctaunt`), looping where the row's `looping` is set
  (`creature::row_animation`, `ONCE` or `LOOPED` and the row). The sim
  times them by the same clips (`session::library`'s `played_clips`), the
  views draw every row so (`viewer::scripted_clips`).
- **Alien speech** (TSL; `Library::entry_voice`): an entry's race is its
  own `AlienRaceNode`, else, for a line its owner says (no `Speaker`), the
  DLG's `AlienRaceOwner`; for a race (an `alienvo.2da` row, not 0) its
  `Emotion` names an `emotion.2da` row whose `label` is the `alienvo.2da`
  column holding the sound (`AVO_TwilMScarM`). Where that cell is set the
  line plays it in place of its `VO_ResRef` (TSL's loader overwrites the
  voice with it), and so counts as voiced; with a number 1 to 3 after it,
  the variant. Every entry shown moves a walk through 19 variants
  (`VOICE_VARIANTS`) that starts and steps from the DLG's name as the
  default camera angles' walk does; an entry with alien speech takes the
  variant drawn the first time it shows and keeps it
  (`Conversation::voices`, KJKS 68). A race with no sound for the emotion
  (`Silence`) leaves the line its own voice. Replies are not given it
  (the exe adds the variant to entries only). Whether the line's length
  is the alien sound's, as any voice's [verify: TSL holds subtitles of
  longer text]. One-liners (barks) keep their own voice [verify].
- **Not read**, as TSL's exe does nothing with them: a node's
  `FacialAnim` (its loader reads it into the node, and nothing reads it
  back: no conversation or client code sends it on or plays a face by it;
  the heads' faces come from the voice's LIP alone, "Lips" below), and
  `PostProcNode`/`PostProcOwner` (no code names either field: no voice
  filter is put on a droid's or a comm's line by them).
- **Video effects** (`CamVidEffect`, `game::video_effect`): a line's row (0
  on) shows that `videoeffects.2da` row while it shows; -1 on a placed
  camera's line shows none; -1 elsewhere and -2 keep the line before's; a
  conversation begins and ends with none [verify -1 on a placed camera].
- **Fades:** a node's own (`FadeType`, `FadeDelay`, `FadeLength`,
  `FadeColor`; `dlg::NodeFade`) starts as the node plays (an entry shown,
  a reply taken), before its scripts, whose own fades then win: 3 fades
  in from the colour, covered by it through the delay (a first entry's
  opens its scene from black); 4 fades out to it; 1 covers at once (the
  scene moved under it); 2 uncovers at once (TSL's one, `terena`'s reply
  35, before a 4 → 4 (0 s) → 3 chain on a clear screen: a 2 that covered
  would hide the guard its next line shows walking out for 2 s; like 1,
  it never has a FadeLength) [verify]; other kinds nothing. K1 chains
  them 4 → 1 → 3 around a scene change, TSL 4 → 4 (0 s) → 3. What a conversation's
  nodes left covering is lifted as it ends, before its end script, a
  script's fade not (`snapshot::Fade::conversation`). [verify in the
  game] the order against the node's scripts, and the end's lift.

```bash
kjk dialogue run k1 end_m01aa --seconds 90 --picks 1,2   # Trask, headless
```

`kjk dialogue run` sets its player up as `kjk play` does
(`dialogue_verbs::play`): it arrives looking the arrival's way, with each
seat's default persona, or the one `--made` makes.

## Scene mode
KOTOR does not pause its world in a conversation: its scenes are calm
because its rules are. While a conversation holds the world the area runs
on those rules instead of JA's. Nothing is paused, and nothing is
restored when it ends.

- **When** (S1): a party conversation holds the world, and so does any
  conversation while every seat's body stands in one
  (`Told::holds_world`, the sim's `world::scene::held`, answered once a
  step as the step begins, `scene::Held`, for every system and the
  scripts; the view's `Snapshot::holds_world`). One player in any conversation of its own holds
  it; in co-op a guest's private one leaves the area to JA. A
  conversation's own rules (its lines' turns, timing, camera, gestures and
  hidden items) hold in every conversation.
- **Walks** (S3): a body walking a route moves on KOTOR's walker at its
  `creaturespeed.2da` rates (`MOVEMENT.md`, `NAV.md`).
- **Minds** (S4, `world::minds`): every creature fights as a scene's
  fighter (melee holds its ground, aiming squarely with no wander, gunners
  stand, no paced saber AI, no guard hold-off, a script's attack at once
  with no moment of awareness). Outside a held world, those taking part
  in a conversation fight so, and every rule of a scene's fight (its
  pushes, flinches, deaths in place, locks) asks the same one gate,
  `scene::Held::scene` (`COMBAT.md`). A
  companion neither follows, chases nor joins a fight of its own accord; a
  script's move still moves it. A party member uses and opens nothing: a
  click, a door it walks into, and the queue's `Door`, `UseObject` and
  `Lock`, which end failed. It rolls no attack, power or throw but a
  `CutsceneAttack`'s (`scene::ROLLS`). Heartbeats, perception, combat
  rounds and every queue run on.
- **Lines** (S2, S5, S6, S7): the turns at a line's start, its timing and
  pauses, its gestures and heads (above, "Lines", "Timing", "Gestures"),
  and its camera (the conversation screen, below).
- **Items** (S8, view only): a DLG's `UnequipItems` hides every item model
  of each line's speaker and listener until the conversation ends, its
  `UnequipHItem` only the held ones (`weapons::Hide`; a body in two such
  conversations takes the larger). Nothing is unequipped. A held weapon
  `UnequipHItem` hides is drawn nowhere, neither in the hand nor where it
  is carried; the body's other carried models stay.
- **Start and end** (S9): only more than one seat gathers the party ("A
  party conversation", below). At the end the facings and places stay where
  the scene left them, hidden items show again and the camera cuts back.

`kjk dialogue sweep k1 MODULE` films every conversation of a module for
review (`scene_sweep`). It steps the module headless as `kjk dialogue run`
does, once from its entry, or once for each line `data/dialogue/k1.sweep`
gives the module (`kjk play`'s flags that set its story up: `--arrive`,
`--global`, `--talk`, `--made`, `--party`, `--item`, `--skill`, and
`--seconds`, 60 by default). Each conversation that held the world and
said something is filmed by a headless `kjk play` of the same run,
`record`ing at 12 frames a second from its first line until three lines
after its first reply set have played, or until it ends: 12 s at least,
60 s at most. The run's player reads each reply set for 2 s before
taking the first reply, so the shot of the replies is filmed too. The
player's `--moves` send the commands the run sent, so the film plays the
same conversation at the same moment; its transcript says whether it
did. The run folder's `index.html` shows every film with its DLG, its
first words, when it starts and ends and the run's flags.

```bash
kjk dialogue sweep k1 end_m01aa    # Trask's opening
kjk dialogue sweep k1 lev_m40ac    # Malak on the Leviathan (its --talk)
```

`kjk dialogue check k1|tsl [MODULE…]` plays every conversation of a
game's modules headless and checks each line, so a cutscene's faults are found
by the machine (`scene_check`). Each module runs the sweep's runs (from
its entry, or `<game>.sweep`'s lines), then one `--talk` run for each
conversation an object of the module carries that those did not reach,
taking the first reply each time until it ends (240 s at most), then up
to four more for it, each taking another reply where a run before took
the first (the first offer with another reply first). Each
module is checked by a child `kjk`, `--jobs` at once (half the cores by
default); every module of the install when none is named. The run
folder's `report.json` holds every failure with its module, DLG and node
(`E12`, `R3`; `-` out of a line), `SUMMARY.md` the counts by check and by
cause with an example of each, and the coverage: the conversations
reached and how many of their entries and replies were shown. What K1's
executable does too is a note, information beside the failures: a
scripted clip the body's chain lacks (the executable has no fallback), an
`AnimList` value neither table names, a voice the install does not ship,
a reply's `AnimList`, a jump moved off its mark by a body on it, a talk
run's stall or unframed shot (it starts a conversation out of its story's
order, its bodies off the marks its scripts would have walked them to), a
line naming a camera its area lacks (Q-0230), a bark asked for as one
(`SpeakOneLinerConversation`, `ActionBarkString`), a module-only weapon
template (its sounds go by the sheet's base item). A talk run's player
first stands where a use's walk along the walkmesh would bring it
(`by_hand::talk_to`); that jump is the harness's, not judged.
The checks:

1. Animations resolve: every animation a script or the engine plays on a
   body (`Acting`: `ActionPlayAnimation`, a line's `AnimList` gesture, a
   blow's swing and reaction, a death in place) is a clip of its model's
   chain as the view binds them (`viewer::scripted_clips`), and every
   `AnimList` value names a gesture or a stunt clip. A body out of any line
   is reported as free play's (`-`).
2. Voice is heard: a voiced entry's `Sound`/`VO_ResRef` is in the
   install's voice folders, decodes, and is heard as the view cues it
   (`dialogue_view::voice_unheard`); missing voices are their own cause.
3. Cameras frame someone: each line's shot, as the view chooses it
   (`play_view::LineShots`: camera model, placed camera, dialog camera),
   0.3 s and 1.5 s after the line shows, has a body (or a speaking door or
   placeable) in its frustum; the dialog camera's also unoccluded by the
   walls. A staged line's participants are looked for where the view draws
   them, on their stunts' marks (`play_view::staged_places`).
4. Scripted attacks give feedback: each scene blow (a `CutsceneAttack`
   row, a round's, an assured strike) has its target's reaction or death,
   a shot for a gunner (a bolt in flight, or its gun's timer starting over:
   a disruptor's beam has none), and a sound event on a melee swing's
   clip. A target that is no fighter (a placeable shot at), dead before
   the blow, or in its own swing as the blow lands (`blows::struck`) has
   no reaction to play; nor one whose own swing overlaps the blow's half
   round (two fighters trading blows on one step: the later animation set
   wins) [verify].
5. Weapons are shown: the weapons of bodies within 15 m of a line's
   speaker load a model as the armory reads them (`weapons::worn`,
   `sheet_models`), checked again once a script equips others; no slot
   holds a carried stack (a script's equip wears one of a stack,
   `State::one_of`, as a player's does; a template wearing its item's own
   `StackSize` is a note); and a
   creature holding any draws one at least in a hand as the view places
   them (`weapons::Hold::place`), unless the conversation's
   `UnequipHItem`/`UnequipItems` or an animation hides them. A weapon in
   the left hand alone stays there, in a fight too: reone hangs the left
   slot's model on `lhand` whatever the creature wields (its wield is the
   right hand's, fists).
6. The dead stay down: no body rises from death, and a looping lying pose
   (dead, sleep, prone) does not end to idle where it lay.
7. Jumps land: a script's jump lands within 0.5 m of its mark, else the
   body that stood on the mark is named (`sim::script::Jumped`, probes
   only); one that finds no place stays, which is no fault where it stands
   on its mark already (Manaan's ambassadors jumped to their podiums each
   line).
8. Conversations end: none goes 60 s without a line; an ended one's end
   (or abort) script ran.
9. Bodies stay on the floor: at a line's start no participant is without
   a floor under it, in solid geometry (tested as KOTOR's walker is, a
   point a body high, `sim::WALKER_HALF_WIDTH_M`, not JA's box), or within
   0.3 m of another body.
10. Census gaps reached: unbound routines called while a conversation
    runs, DLG node fields `fmt_odyssey::dlg` does not read (`NODE_FIELDS`,
    which passes over the toolset's `SoundExists` and the fields TSL's
    executable never acts on) set on the lines shown, and a reply's
    `AnimList` (not played, a note).
11. Module-only resources a view reads through the install's base (a
    weapon template only the module holds is a note).
12. JA's free play in a held scene: a pain flinch, a saber lock, a one-line
    conversation begun while the scene holds (both executables refuse it).
13. Lines are said by whom they name: an entry naming no speaker is its
    owner's; one naming a speaker is said by a body of that tag (any
    case), never by its owner standing in for one not here; and the
    view's pair (the camera, the facing; the voice and the lips read the
    same speaker) has that body speaking. A tag several bodies share is a
    note. Beside the runs, every entry `Speaker` of the module's
    conversations is counted by what answers to it there
    (`speaker_census`): the owner's own tag, one module object, several,
    one sharing a party member's tag, a party member, a template scripts
    may make, `OWNER`/`PLAYER`, no one.
14. Scripted moves arrive: every script move (`ActionMoveTo*`,
    `ActionForceMoveTo*`) ends where it was sent, not with no way there,
    its walk given up short, or a forced move jumped there at its timeout
    (`sim::script::Moved`, probes only). A move is a conversation's
    when its mover is one the conversation's scripts gave actions to
    (a node's script or its end script changed the mover's queue,
    `Report::directed`), the move begun after that, while the
    conversation ran or within the 3 s after it ended (its end script's
    walk out). Any other move is the mover's own routine (K1's walkways,
    `GN_WalkWayPoints`; an AI's wander, `ActionRandomWalk` or a beast's
    `ActionMoveToObject` to an ambient waypoint), which runs whatever
    line shows, and is a note: judged by time alone, every walkway leg
    under way while a line showed was blamed on that line. A forced
    move whose timeout comes while its walk still carries it on (not
    stalled, moving) is a note too: the script's jump is its plan (K1's
    walkways give a leg its 2D distance over 1.25 m/s, 30 s at least, a
    slower pace than any walk of `creaturespeed.2da` but its `SLOW`
    row's, and a way round longer than the straight line outlasts it).
    In a talk run, a move whose way only a door shut now closes (no way
    through the doors as they stand, none opening for the held party, S4,
    and one with every door open) is a note: the story opens it first
    (the Leviathan's cells before the party's escape, the bridge's door
    before Saul's scene). A talk run steps on while a body still walks a
    script's move, 40 s at most, so its walk out is judged.
15. Lips move: a voiced entry has a LIP (the voice played's, else the
    node's other sound's), and where a creature speaker's head or body has
    a `talk` clip in its model chain that keys it, the view's mouth plays
    it (`dialogue_view::lips_unmoved`); one with none (a helmet, a mask, a
    droid) is a note.
    The mouth is the head hung on the body, else the body itself: a body
    made with its face (`appearance.2da` `modeltype` F with no
    `normalhead`: Calo Nord, the aliens' bodies) has the face's nodes
    (`talkdummy`, `f_jaw_g`, …) and plays its own chain's `talk`, as reone
    plays a speaker's `talk` on the body and everything hung on it. A
    voiced line with no LIP, and lips moved on a body's own face, are
    notes (the census).

The participants' hooks no run raised are listed for information.
`kjk sim probes --only "dialogue check"` runs the checks on data made
in the probe.

```bash
kjk dialogue check k1                       # every module
kjk dialogue check k1 end_m01aa tar_m02ae   # two
kjk dialogue check tsl --jobs 7             # every TSL module
```

## Computers
A DLG with `ConversationType` 1 is a computer's (K1: 114 in its modules,
8 of them Rakatan, `ComputerType` 1); it is walked as any other, with
its owner (the terminal) and the player who used it, the player held,
ended when a line leads nowhere. Everything about Computer Use is the
games' own scripts' (journal `2026-10-02-computer-use`): a start
condition sets `<CUSTOM31>`… to each option's spike cost
(`k_con_tokens`), an option's condition compares the spikes the first
player carries (`GetItemPossessedBy` tag `K_COMPUTER_SPIKE`,
`GetNumStackedItems`) with the cost, and its action takes them
(`SetItemStackSize`, `DestroyObject`); the cost is the option's base less
a quarter of the party leader's `GetSkillRank` (its rank with effects,
`SCRIPTS.md`), at least 1 (0 from 3 below). Repair's droids do the same
with `K_REPAIR_PART`. The engine takes nothing itself.

```bash
# The Endar Spire's security panel: overload the conduit with 3 spikes,
# done at Computer Use 8 (cost 3), refused at 0 (cost 5)
kjk dialogue run k1 end_m01ab --talk end_comp02 --skill computer=8 \
  --item g_i_progspike01=3 --picks 2,3,4 --seconds 25
```

On screen (`docs/UI.md`): KOTOR's `computer.gui`, not the cinematic
bars. A use whose trace hits nothing reaches the nearest usable
placeable in front that speaks, within reach (2.5 m, in height too): the
Endar Spire's panel, whose walkmesh neither stops the player nor the
trace (not known why). A camera feed (a line with no text and a camera)
steps the screen aside to the area's camera (the conversation camera,
below). Not yet: `ActionStartConversation`'s computer type for a
cinematic DLG, the Rakatan look (`ComputerType` 1), the full-window
background (`1600x1200comp0`), items' skill bonuses.

```bash
# The panel used and sliced through the screen: Computer Use 8, 3 spikes
kjk play --stage end_m01ab --arrive end_explode04 --skill computer=8 \
  --item g_i_progspike01=3 --headless \
  --moves '300ms yaw 326; 1450ms w yaw 326; 300ms yaw 18; 600ms use yaw 18; 90s' \
  --cmds 'wait world; wait 4s; ui nav down; ui nav activate; wait 6s;
          ui nav down; ui nav down; ui nav activate; wait 1s; screenshot; quit'
```

## To clients
- **What everyone sees**: `Snapshot::talk` (`dialogue::Told`): each
  conversation (its DLG, owner, player, speaker, entry, the replies
  offered, when the line was shown and ends, whether it is paused; in
  co-op its listener, the bodies joined, whether it holds every seat and
  is locked, who asks the host in, whom its line waits on and the
  player of the pazaak match its line dealt, R37; KJKS 56),
  the barks, the custom tokens. Views resolve text from the level's own
  DLGs. The authority's scripts write it; a module install keeps only the
  tokens.
- **Answers** ride the command (`Command::reply`): the low six bits what
  it asks (1–30 take the reply at that place from 1, a place past the
  last the last; 31–60 point at the reply at that place less 30; 62
  leaves the conversation, 63 skips the line), the high two a count of
  answers; a client holds the byte until its next answer, and the
  authority answers when the count moves. A repeated (substituted) or
  late command so answers once; the first byte heard from a player only
  starts the count. It travels in the command's selection group with the
  weapon and the power (KJKR 31).
- **Held**: a body standing in a conversation (its listener, or one
  joined to it; every seat's while a party conversation is up) stands
  still and does nothing but answer, its yaw kept where it faces: only a
  line's start turns it (Lines, above). `hold_talkers`, the step's first
  system; clients predict it from the snapshot, so what the host's
  scripts decide is corrected: its start and end, and the turns a line's
  start gives a held player's yaw (a guest sees its body turn then).
- **Co-op**. Two players can be in two conversations at once,
  and a player can leave one until it has locked them into the story.
  The scripts still see one PC, the host's character (I30 §0). One player
  plays exactly as before: every conversation the player starts is theirs
  and their AI companions come to it.
  - **A player's own conversation** (private): a use started it
    (`Start::asker` a seat's acting body, `View::seats`: the companion
    the host leads through too) and its DLG marks no cutscene. Its
    listener (`Conversation::listener`) is that body; its PC
    (`GetPCSpeaker`) stays the host's character, also where a script names
    a guest's companion (`View::speakers`), so free replies that face or
    move toward the PC act toward the host's character. Only its listener
    and the players who joined it are held and see it (`Told::holding`,
    `Snapshot::talk_for`); its seat's companions no seat drives come to it
    (`Gathered::Companions`), so a guest's brings nobody; the others play
    on. Its lines are said to its listener, a guest's companion, where no
    other listens, as the camera frames it (the PC where the host leads
    through a companion, as before). A start is refused ("already
    talking") while its owner or its listener talks, the host's two
    bodies (its character and the companion it leads) one. The asker rides the use: a dialogue hook's
    run, a placeable's `OnUsed`, the scripts they execute and the
    `ActionStartConversation` they queue (`Pending::asker`,
    `Event::asker`, `Action::Converse`; KJKV 40), so Taris's sewer
    console starts its user's own. Code run by `AssignCommand` or
    `DelayCommand` carries none.
  - **A party conversation** holds every seat (`party`): the game started
    it (a trigger, a heartbeat, a script), its DLG marks a cutscene
    (`AnimatedCut`, a `CameraModel` or `Skippable` 0, however it began),
    or the host accepted a request (below). Every seat's body is held,
    every screen shows it (a watching seat the conversation holding the
    member it follows, else this one), and, while more than one seat plays
    (`View::together`), party members farther than 10 m [verify] come to
    free places in sight (`Gathered::Everyone`), but not a seat's body
    standing in a player's own conversation; one player's party stays where
    it stands, as in KOTOR (scene mode's S9). Starting
    it cuts every conversation a seat's body stands in short ("a cutscene
    began"), locked or not; a start refused (already talking, nothing to
    say) cuts nothing. A bark holds nobody.
  - **Joining**: a player who uses the owner or the listener of another
    player's own conversation joins it (`joined`): held, shown it, marking
    and answering its free replies. Two seats using one NPC in one step:
    the lower seat starts it and the other joins.
  - **Markers**: any seat points (`Pick::Point`, the reply its screen
    highlights): `Conversation::marks` (seat → reply place), shown to
    everyone in it beside the reply with that seat's character's name;
    cleared when the line changes, taken before answers; a place past
    those offered marks nothing. `Talk.heard` is keyed by seat.
  - **Who answers.** A seat's answer is for the conversation its body
    stands in, else the party one (`answers`); a watching seat's only for
    the party one, and only as marks. Its listener answers at once, anyone
    else once the replies are up, so one sent for the line before takes
    nothing; the first answer in a step takes the line, in seat order.
    The story's replies are a PC's (a player's own character, or the body
    the host acts through): another's pick marks one, and where no PC stands
    (a guest's own conversation) it asks the host in (below). A paused
    conversation, or one whose line waits, takes no answer and no mark.
  - **The story's replies** (`dialogue::story`): picking one
    may reach, before the next replies are offered, a node with a quest or
    plot XP, or a script that may call one of `STORY_ROUTINES` (globals,
    the journal, alignment and influence, the party, the module and the
    galaxy map, factions, items and credits, the character), its own, an
    entry's it leads to, through replies taken by themselves, or the end
    script where it ends. XP alone and locals are anyone's, and a script
    that deals pazaak is the minigame's, not the story's. The survey it
    rests on (`context/artifacts/2026-10-13-coop-dialogue/SURVEY.md`): K1
    69 % of spoken replies anyone's, TSL 73 %. A guest's screen draws the
    story's replies faded.
  - **Asking the host in** (R5): a guest's pick of a story reply in its
    own conversation, where no PC stands, marks it and asks
    (`Conversation::asking`, `Line::Asked`; one asker a conversation).
    The guest keeps answering free replies meanwhile, or leaves, the
    request going with it; a PC joining clears it. The host answers with
    `Action::Request { talk, accept }` (wire code 36). Accepted, the host's
    own conversations are cut short ("the host joined another"), it steps
    out of one it joined, and the asked one becomes a party conversation
    with the host's character its listener, the guest's body joined,
    everyone gathered to its owner; the host answers its story replies.
    Ended, it is cut short, locked or not. While the party is in combat or
    the host stands in another locked conversation the answer does
    nothing and the request waits; the host's keys wait meanwhile, and
    while an exit's ready check is up or the host is down, and the card
    says why (`Snapshot::request_wait`; `UI.md`, "The host's request
    card"). Several guests ask each in their own
    conversation; the host's screen lists them and its keys answer the
    first.
  - **Locks and leaving** (R6). A conversation locks for good
    (`locked`): a party one the game started, and a clicked cutscene,
    from its first line; one whose `EndConverAbort` may move the story
    (`story_abort`; in both games it is the end script too) from its first
    line; any other once a story reply is taken, a silent one too, or a
    line shown moves the story (`story_line`: a script of its own, its
    quest or its plot XP). A party conversation the host accepted locks the
    same way. `Pick::Leave` (byte 62): the listener's ends it cut short
    (`EndConverAbort`), a joined player steps out, its mark and request
    going with it. Refused, noted `Line::Refused`, while it is locked
    ("locked", a joined player's too: so in a game-started party
    conversation, locked from its first line, everyone is told
    "locked"), and in a party conversation the host accepted that is
    still unlocked, from anyone but its listener ("the host's"); one
    paused or waiting takes no Leave.
  - **Interjections** (R4): in a player's own or a party conversation, a
    line said by a seat's body that does not stand in it is said where
    that body stands, never walked or moved, and the body joins. Its own
    unlocked conversation is cut short first ("an interjection"); one it
    only joined, it steps out of. While it stands in a locked or a party
    one, the line waits (`waits_for`; the conversation on hold) until that
    one ends, for `INTERJECTION_WAIT_MS` (10 s) at most, then plays
    without it, its replies offered as they stand then.
  - **Damage** (R7): while more than one seat plays a body
    (`View::together`), a listener hurt (the `Damaged` raise), a seat's
    body, has its own conversation cut short, locked or not ("its
    listener was hurt"; two others talking go on), unless
    its owner is not interruptable (the UTC's `Interruptable` 0: 6 of K1's
    and 4 of TSL's scripted captives; `ObjectData::interruptable`, the
    templates pack KJKT 18, `templates-v14`, so each install imports its
    templates again once); a joined body hurt steps
    out; a party conversation goes on. One player alone talks on when hurt,
    as the originals do: Dantooine's ruins computer is answered under the
    droids' fire.
  - **A seat letting its body go** (R12, `world::drivers`,
    `dialogue::Release`): died, its own conversation is cut short, locked
    or not; its player dropped or it left the party, an unlocked one is cut
    short and a locked one goes to the host (a party conversation, the
    host's character its listener, everyone gathered), so the scene still
    finishes. While the host stands in another locked conversation it waits
    on the host's (`waits_for` the host's character) and goes over once
    that ends, or is its player's again if the seat drives the body back
    first; its request goes either way, the host taking it over. A joined
    body steps out. Its trade ends too.
  - **A module a script asks for** (`StartNewModule`): each guest's own
    conversation, and a dropped guest's waiting on the host, is cut short
    ("the module changed") while its module is still loaded; the host's
    own, the party's, and a guest's own that a host's body has joined end
    with the module without their scripts, as one player's always have.
  - **Elsewhere**: a store or a pazaak match a conversation opens (its
    nodes' scripts or its end script) opens for who asked
    (`Conversation::asker`: the use's body, then whoever's pick took the
    last reply), on that seat's screen alone (`WORLD-STATE.md`, "Stores";
    `PAZAAK.md`). The host's party choice is refused while it would send
    away a companion a guest drives that is talking, trading or playing
    pazaak ("Carth is busy.", the selection kept open). Saving names the
    first guest busy (`UI.md`); an exit's ready check leaves an unlocked
    conversation or a store and waits on a locked one or a match
    (`PARTY.md`).
  - `kjk sim probes` runs the cases (`sim::coop_talk`, the approved
    `dialogue_coop`).
- `kjk play`: the conversation screen's answer (below) goes into the
  command; outside a conversation 1 and 2 still pick the hand.

## The conversation screen (`kjk play`)
KOTOR's `dialog.gui` on the UI framework (`ui::screens::dialog`, KOTOR 2's
`dialog_p` with the skin; `docs/UI.md`), fed by `dialogue_view`: black
bars above and below across the whole window, the words in the layout's
font (`fnt_d16x16`) and K1's blue, crisp at any size as every screen's.
Developer notes (`{…}`) are left out and `<CUSTOMn>` filled in. As reone
places it: while the line plays it is in the bottom bar, centred from its
top, and no reply shows; once it has played (the player's turn) it moves
to the top bar, centred down to its bottom, and the replies show below,
numbered, the highlighted one yellow; a reply that says nothing is
"[CONTINUE]" (xoreos's word). A bar grows to a long line or many replies,
up to half the window, rather than cutting or scrolling them. The screen is
drawn at half the other screens' size (`dialog::SCALE`, matched to
KOTOR 2 at 1280 × 1024), its bars kept to the window. The replies run
across the window (48 units in from each edge, as the games draw them
wide); the line wraps at about 80 characters, centred.
While the player is in a conversation the pointer is free. While the line
plays, a left click anywhere, Enter or Space ends it (where the DLG lets
it be skipped). On the player's turn, hovering or Up and Down highlight a
reply, a click on one, Enter or Space takes the highlighted one, 1 to 9
take theirs (`dialogue_view::ReplyInput`). A screen open over the
conversation (the menu) takes the input instead. Not yet: the GUI's hover
and click sounds (`gui_actscroll`, `gui_actuse`).
In a session of more than one seat (`docs/UI.md`, "the dialog's Leave
row"): the names of the seats pointing at each reply beside it; the
story's replies faded on a screen whose player does not take them
(`Snapshot::takes_story`), a click on one asking the host in where the
sim would (`Snapshot::asks_host`), else pointing; a Leave row under the
replies while more than one seat plays a body, faded while locked, and
its key (key mapping "Leave a
conversation", L by default, which is saber style's default too: it
leaves only in a conversation); on the host's screen, while a guest asks
it in, the request card (key mappings "Accept a join request" and "End
a join request", Y and N by default, which are also the ready and follow
keys: they answer only while the card is up and nothing keeps it
waiting, the card saying what; Y lets the first in, N ends that
conversation).
- **Voice**: the line's `Sound`, else its `VO_ResRef` (TSL's alien speech
  in their place, "Alien speech" above), as the install has
  them (`streamwaves`, `streamvoice`; KOTOR's MP3 behind a RIFF header is
  unwrapped, `fmt_odyssey::wav`), read when first needed and cued from the
  speaker at the moment the line was shown, in its voice slot; a line cut
  short (a reply picked early, a skip, the end) silences it
  (`sound::Hush`, `audio::Mixer::stop`).
- **Lips**: the speaker's mouth, its head (the model hooked to its body)
  or, with none, its body made with its face (`dialogue_view::mouth_clip`),
  plays its `talk` clip as sixteen mouth shapes evenly spaced over the clip; the
  line's LIP (the voice's name, else the other's) picks the two around
  the voice's time and the blend (reone's reading), held at the ends; the
  head rests when the line ends or someone else speaks.
- **Camera** (per frame): a DLG with a `CameraModel` (the cutscenes)
  films its lines from that model's `camerahook` (`play_view::animated_shot`):
  the model stands at the area's origin, as reone puts it, and plays the
  line's `CameraAnimation` clip (`cut%03dw` from 1200, `camera_clip_name`)
  from when the line showed, held at its end; the camera looks down the
  hook's −z; the line's `CamFieldOfView`, else 55 (Q-0141: reone keeps 55).
  A line that waits on its clip (`WaitFlags` 1) lasts its own timer (its
  voice or text, `Delay` and other waits) raised to the clip (the library's
  `camera_clips`, read from the models' clip lengths), as the K1 and TSL
  executables end a line once its timer and every flagged wait are done
  (reone takes the clip alone). The camera keeps the hook's roll.
  A line with no clip (no `CameraAnimation`, or one its model lacks) has
  an ordinary shot, as KOTOR's does (reone keeps the camera where the
  last clip left it), the placed camera or the dialog camera below.
  Else a placed camera a script cut to
  (`SetDialogPlaceableCamera`, `Conversation::camera`, KJKS 38, KJKV 25:
  the conversation whose script runs it, else the one the caller is in,
  else the first under way; held until
  the line changes [verify]), else the line's own `CameraID` where its
  angle is 6 (KOTOR keeps a `CameraID` on no other node), a DLG with a
  camera model too. A number the area's GIT has is that
  placed camera (`play_view::placed_shot`): at its `Position` raised by
  its `Height`, turned by its `Orientation` after a tip about its own x
  axis by `Pitch` (0 looks straight down, 90 level), its `FieldOfView` the
  vertical one. A line's number the area lacks films it as KOTOR's camera
  does (`play_view::MISSING_CAMERA`, Q-0230): from the area's origin at 55
  degrees [likely], looking straight down, unturned and with no pitch
  [verify]; a script's cut to one it lacks leaves the line to the dialog
  camera [verify].
  Otherwise KOTOR's dialog camera (`play_view::talk_camera`,
  read from the K1 executable; design S6):
  - *Which shot*: the line's `CameraAngle` as the sim resolved it
    (`Conversation::angle`): 1 the close-up of the speaker, 2 over the
    listener's shoulder, 3 the wide two-shot (3 after 3 keeps the shot
    and its pair), 5 keeps the shot and its pair, and 4, which has no shot
    of its own, the same [verify]. After an animated or a placed line, 5
    and 4 keep the last dialog shot before it [verify: the exe likely has
    the previous line's pair face each other again]. A default (0) never reaches the view:
    the sim takes it from the sequence `1,3,1,3,2,2,1,3,1,2,1,2,1,3,2,3,1,1,1`
    (`angle_walk`), walked from the hash of the DLG's name, lowercased
    [verify] (the sum of
    c·(1 + int(10^i)) over its characters, wrapping, a power past the
    range `i32::MIN`; read unsigned, it starts at the hash mod 19, steps
    by the hash mod 5, plus 1, forward when odd, past 18 back to 0 and
    below 0 to 18), moved once by every entry shown and once by every
    reply set (a silent one too [verify]), each taking the angle before
    its move. The first entry and the first reply set's defaults take 2.
    A node shown again keeps its angle (`Conversation::angles`). The
    line's camera fields are its entry's; while the replies are offered,
    the first reply's; once one is said, its own.
  - *The pair*: the speaker (A) and its listener (B,
    `Conversation::listener_now`); while the replies are offered, the
    player and the one who spoke. The player is filmed like any speaker.
  - *Geometry* (`dialog_shot`, the exe's constants, L the heads'
    distance): the close-up aims 0.2 m from A toward B levelled and
    0.04 m down, the eye 0.5 m behind that, 30° off the line, pulled back
    further by the speaker's talk clip's `CU_PB_Range`
    (`dialoganimations.2da`, the `AnimList` value less 10000; read into
    `LoadedScene::pull_backs`), which carries on until a talk clip with
    none; over the shoulder aims 30% of the way from B to A (A lowered
    0.1·L, B 0.05·L), the eye 0.8·L + 0.15 m behind, 30° off; the wide
    shot aims at the midpoint 0.2·L down, the eye 1.5·L square to the
    pair and 0.3·L up. `CamHeightOffset` and `TarHeightOffset` raise the
    eye and the aim. 55 degrees. The shot, its side and its wall
    fall-back are fixed as the line starts; the heads are read every
    frame, so the camera follows the pair. Each line cuts.
  - *Heads*: a body's `camerahook` as last drawn (its model's, else its
    head's, `Cast::fit`), else its soles raised by its model's height, the
    top of its meshes and its head's at rest (`Cast::model_height`;
    KOTOR raises the body's position by 0.1 m and a height of the model's
    own [verify]), else 1.7 m.
  - *Side* (`Sides::side`): chosen as a pair first appears, and remembered
    for four pairs a conversation, the first slot overwritten. The pair
    reversed takes the other side, which keeps the camera on the same
    side of the line between them (the 180° rule). A new pair's side is
    the one whose shots meet fewer walls: the wide shot's sight lines to
    the midpoint and both heads, and the over-the-shoulder shots' of
    each. Side 2 (−30°, −90°) wins a tie. Sight lines are traced through
    the level, its doors as drawn; it holds no bodies.
  - *Walls*: a wall between the eye and the speaker's head (for the wide
    shot, the midpoint or either head) films the line as a close-up
    without the pull-back. The eye is never pulled in.
  - The DLG's `OldHitCheck` (an older variant of the camera) is not read
    [verify]. The pair turns to each other at each line's start (Lines,
    above).
- **Cutscene participants** (an `AnimatedCut` DLG's `StuntList`, reone's
  stunt mode, `play_view::stage_stunts`): while its line shows, each
  participant (by tag; `owner`, `player`) stands where its stunt model's
  `cutscenedummy` stands in the line's clip for it (its `AnimList` entry,
  else the camera's clip) at the time since the line showed, played at
  scale 1 and held at the end: a body whose nodes the clip names has its
  root there and the clip poses its limbs (reone plays the stunt model's
  clip on the participant's model at the origin); JA's own body is turned
  and moved there, facing the dummy's +Y. The sim keeps everyone where they
  were. A placeable participant (a cutscene's battle screen and its holo
  ships) is its scene model posed the same way
  (`LoadedScene::instance_models`). A character's stunt clip's own events
  (swings, parries, clashes, contact sparks) are heard as a script's
  clip's are (`clip_events`), from where it was last heard.
- **Back faces are culled** (`OdysseyMaterial::culled`): no model's back
  faces are drawn, a decal texture's excepted, as reone and xoreos draw a
  KOTOR mesh, so a cutscene camera behind a wall or below a floor sees
  through it (the Republic flagship's holo table shots), and a close
  dialog camera through a head it grazes. A JA surface whose shader says
  `cull none` (`disable`, `twosided`, or `back`) is drawn from both sides
  (`TextureFacts::two_sided`), as are effects' cards and particles.
`--moves` answers with `reply N` and `skip`:

```bash
kjk play --stage end_m01aa --headless --moves '7s; 10s reply 1; 3s reply 1' \
  --cmds 'wait world; anim 9; screenshot; quit'
```

## End to end
- Talking to a creature: target it and press E (Dialog; `docs/UI.md`,
  "Targeting and use"), walking there when it is further than 3.2 m (four
  fifths of the conversation's 4 m). The
  use key's old trace meets bodies too, for scripted `--moves … use`
  runs.
- `kjk play` writes `transcript.txt` in its run folder as the lines come
  (the host's own, each resolved against the level it was said in).
- `--global NAME=NUMBER` (`kjk play`, `kjk dialogue run`) sets a script
  global before the first step: a story's state by hand.

```bash
# Trask, from his walk-up to "Okay." (answers just after each line ends)
kjk play --stage end_m01aa --headless \
  --moves '7.2s; 13.6s reply 1; 36.8s reply 1; 4.7s reply 1; 15s reply 1' \
  --cmds 'wait world; record trask 0.1 66 15; quit'
# Carth at the escape pods: used, "Who are you?", to his last line
kjk play --stage end_m01ab --arrive end_wp_carth01 --global END_CARTH_DLG=5 --headless \
  --moves '300ms yaw 148; 250ms w yaw 148; 600ms use yaw 148 pitch 8; 6s; 20s reply 1' \
  --cmds 'wait world; record carth 0.1 26 20; quit'
```
