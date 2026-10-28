//! A snapshot is the sim's state after a tick, as plain values. It is the
//! only thing that leaves the sim: render, UI and audio read snapshots, a
//! save is one, a client will receive them. What is absent from a snapshot
//! cannot reach any of them.
//!
//! It is complete: `SimWorld::from_snapshot` rebuilds a world that steps
//! exactly as the original would have.

use std::fmt;
use std::sync::{Arc, Mutex, Weak};

use script_ncs::vm::ObjectId;

use crate::combat::fighter::Life;
use crate::combat::{Fighter, Missile, Sheet};
use crate::input::Seat;
use crate::inventory::Goods;
use crate::module::ObjectKind;
use crate::motion::{Anim, Ground, MotionState, Part};
use crate::rng::SimRng;
use crate::script::dialogue::{Conversation, Told};
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};

const MAGIC: &[u8; 4] = b"KJKS";
/// Bump when the encoding changes: old snapshots then refuse to decode
/// instead of decoding wrong. 2: avatars carry their movement state (I13).
/// 3: the module, its objects, the transition asked for, the modules left
/// (I14). 4: characters fight: avatars and creatures carry a fighter
/// (and creatures their movement), avatars their sheet (I23). 5: fighters
/// carry their saber and Force states. 6: and their mind (I27) and their
/// blaster special (`combat::specials`). 7: sheets carry their race and
/// guards by damage type, weapons a bonus against a race. 8: fighters carry
/// their skeleton's pose, blade trails and swing, the saber's lock, throw,
/// kick and bolt cooldown (I24). 9: grenades: carried on sheets, thrown
/// and suffered on fighters, in flight on missiles. 10: weapons carry their
/// reach. 11: sheets their skills, minds when they become aware. 12:
/// sheets carry four kinds of items, fighters their pack (`combat::items`).
/// 13: fighters carry KOTOR's effects on them (`combat::effects`). 14: and
/// the blade hits held for them (I25, C19's defender grace). 15: sheets an
/// attack modifier, packs a stim's vitality and a poison its poisoner.
/// 16: events carry the weapon that struck (I22 C20's sparks). 17: and
/// where a block happened (its flare). 18: a bolt held for its target's
/// grace (C19 for bolts). 19: a pose's blend carries the blend it began in
/// (CHARACTER-LOOK). 20: effects carry the lasting effects scripts apply
/// (`combat::effects::Lasting`). 21: events a script's visual effect
/// (`Outcome::Visual`); objects a sound's volume, the area its sound. 23:
/// the conversations, barks and custom tokens everyone sees (I18). 24: the
/// objects made in play, here and stashed (I19, `Spawned`). 25:
/// containers: objects carry the slots taken from them, the tail what each
/// seat carries (I21). 26: the goods: the party's gold and items, trades and
/// their stores' stock (I19, `inventory::Goods`); what a seat took from
/// containers is the party's items, no longer a list of its own. 27: the
/// story's progress everyone sees: the journal and the party's XP (I19).
/// 28: objects carry their lock as scripts, keys and Security leave it
/// (`ObjectState::lock`). 29: an object made in play carries its facing
/// (`Spawned::yaw`: a made placeable's, which its collision and its model
/// turn by). 30: sheets carry their alignment (`Sheet::good_evil`). 31:
/// co-op (I30, I31): minds carry the seat that
/// drives them (P2, `Mind::driver`) and whether a driven one is steered
/// (`Mind::steered`, so a client predicts it as the authority moves it); a
/// module's place carries the players it was planned for (P21,
/// `Place::players`), and the head the seats the session holds
/// (`seats_held`); conversations carry who asked and the other seats'
/// markers (P6, P8); fighters fall: `Life::Downed` and its record
/// (`Fighter::fallen`, who revives it); the tail carries when the whole
/// party went down (`lost`; P11–P13) and the party's calls: in combat, a
/// break's caller, a ready check (`world::party`). 32: the tail carries the
/// modules left by their BLAKE3 (`Stash::digest`), their bytes come last, and
/// the hash leaves those bytes out (the digest stands for them). 33: the
/// tail carries the minigame in play (`minigame::Course`); objects a
/// sound's speed scripts fixed, how often they started it and how long
/// their stop fades it (`ObjectState::pitch`, `plays`, `fade`). 34: the
/// room clip a script plays (`AreaSound::room`). 35: the head carries the
/// screen's fade as scripts, the engine and conversations' lines set it
/// (`Fade`, with its lock and whether a line set it) and the engine's fade
/// in at a module's beginning (`WorldFadeIn`). 36: the tail carries the
/// companion leading the party in the first seat's hands (`leading`). 37: a
/// minigame's co-op: a race's run per seat with its globals and finish,
/// the turret's gunners, the module a race holds, each bolt's gunner; a
/// fighter's lasting effects may be a visual (`Modifies::Visual`). 38:
/// conversations carry the placed camera a script cut to
/// (`SetDialogPlaceableCamera`). 39: a shield up carries the damage types it
/// takes (`combat::items::Shielded`); a poison carries its course
/// (`combat::items::Course`), and the abilities poisons drained
/// (`Pack::drained`). 40: both together (main's cameras with the second
/// engine-smells batch).
/// 41: a poison carries the script effect it came linked in
/// (`Poisoned::link`). 42: the tail carries the pazaak match a script
/// started (`pazaak::Match`); a fighter's pack its last item used, a heal
/// under way its row and the use Use Item asked for (`combat::items::Pack`);
/// the goods' items their upgrades fitted (`inventory::Item::upgrades`); a
/// sheet's weapons how they are held (`Weapon::build`).
/// 43: sheets carry what worn items add to skills (`Sheet::item_skills`);
/// objects their trap's state (`ObjectState::trap`); fighters their
/// stealth and skill buttons (`Fighter::covert`).
/// 44: the tail carries who each seat's player is (`persona`), the progress
/// the careers (`Progress::careers`).
/// 45: an object's state the hit points it lost (`ObjectState::damage`:
/// bashing); the tail carries what the party explored of each area's
/// map (`fog`).
/// 46: an object's state a placeable's animation state (`ObjectState::state`,
/// `look`); the tail carries the scripts' visual effects on doors and
/// placeables (`visuals`).
/// 47: sheets carry what worn items give in place of their skills
/// (`Sheet::gear`); weapons their typed extras, massive criticals, on-hit
/// effects, Mighty, enhancement and traits, a bonus against a side
/// (`gear::Versus`); fighters' effects their worn immunities, traits and
/// regeneration (`Effects::worn_immune`); the goods' items TSL's upgrades
/// fitted (`inventory::Item::fitted`). 47: what play made carries what a
/// container made with its own goods holds (`Spawned::holds`: a dead
/// creature's remains).
/// 49: an entity with a lifetime carries how it goes (`Going`: the part of
/// it gone to scripts, only drawn, and the part drawn fading out), as
/// `DestroyObject`'s `bNoFade` and `fDelayUntilFade` ask.
/// 50: an entity a destroy took away is out of the world at once; the tail
/// carries it as it went while it is drawn going (`departing`), and a
/// lifetime how it goes (`EntityState::going`: fading in the world, the
/// dead, or departing after, what a destroy takes).
/// 51: the progress carries the XP the party's latest kills gave
/// (`Progress::kills`). 52: and the party's latest statuses
/// (`Progress::statuses`).
/// 53: a bolt says whether a script fired it (`combat::Missile::scripted`).
/// 54: a fighter says who has its weapon out or away (`combat::Sheath`).
/// 55: the pazaak match says whose it is (`pazaak::Match::player`).
/// 56: a conversation's listener, joined bodies, party, lock, request, wait
/// and the match its line dealt (private conversations); the pazaak
/// matches, each player's, and a match's pause, its player's seat dropped
/// and its hurt (`pazaak::Match::paused`, `away`, `hurt`), and who watches
/// it (`watchers`).
/// 57: a creature's mind carries its pacing (`combat::pace`, a blade
/// wielder's; `combat::troop`, a gunner's); a swing carries when its
/// holder's last attack began (`saber::sweep::Swing::struck_ms`).
/// 58: a conversation carries its camera angle, the default-angle table's
/// place, step and direction, the angles its nodes were given, the line's
/// listener, the previous line's pair and the turn and head-follow locks;
/// a walker carries its bumps and the body it last bumped into (scene
/// mode, `walk::Walker::bumps`).
/// 59: the head carries the video effect scripts enabled
/// (`EnableVideoEffect`: a `videoeffects.2da` row).
/// 60: a status may be TSL's influence a companion gained or lost
/// (`StatusKind::Influence`).
/// 61: a lasting effect carries when it took hold (`Lasting::at`) and may
/// be a KOTOR state or TSL's effects (`Modifies::State`, `Blind`, `Fury`,
/// …); a fighter's effects carry Fury's kills (`Effects::fury`), its Force
/// the state holding it (`ForceState::state`, `state_until`).
/// 62: a sheet carries TSL's bonus Force points, base save bonuses and Form
/// in use (`Sheet::bonus_fp`, `save_bonus`, `form`); a fighter its fake
/// combat and its flourish's wait (`Fighter::fake_until`,
/// `flourish_ready`), its Force TSL's regeneration's carry
/// (`ForceState::fp_carry`); a status may be the most Force points raised
/// or lowered (`StatusKind::MaxForcePoints`).
/// 63: a fighter carries the body scripts made it for good (`combat::Look`:
/// `ChangeObjectAppearance`, `DuplicateHeadAppearance`); the progress the
/// feedback floating over objects (`Progress::feedback`,
/// `DisplayFeedBackText`); the tail the in-game screens as scripts leave
/// them (`Screens`: the map jammed, the map page's return button).
/// 64: a minigame's course carries the jump key held, the swoop's jump
/// speed and the bump that holds its speed (TSL's races), and each
/// obstacle whether a script took it off the course.
/// 65: the screens whether TSL's Bao-Dur has been met
/// (`Screens::bao_dur_met`). 66: the items lying on the area's floor
/// (`Goods::floor`). 67: a player carries its last command's crosshair
/// line (`MotionState::command_aim`, `Command::aim`), and so does a
/// fighter's last command.
/// 68: a conversation carries its walk through TSL's alien speech's
/// variants and the variant each entry took (`Conversation::voice_cursor`,
/// `voices`).
/// 69: an item's flag that no one may put it on (`Item::non_equippable`).
/// 70: a pistol in each hand: the left gun's timer, its turn, the hands of the
/// last shot and a Rapid Shot's left shot still to fire (`Shooting::off_ms`,
/// `next_off`, `hands`, `off_burst`).
/// 71: a container no longer carries the slots taken from it
/// (`ObjectState::taken`); the goods carry what each container held open holds.
/// 72: a swing's contact keeps whether its chance to be turned was drawn
/// (`Contact::drawn`).
pub const VERSION: u32 = 72 + crate::pending::SNAPSHOT;

/// An entity's identity in the sim, stable across save, load and replay.
/// Handed out in order and never reused, so ordering by it is canonical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimId(pub u32);

/// What an entity is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A seat's player, moved by that seat's commands through JA's
    /// movement (`move_body`).
    Avatar(Seat),
    /// Walks in random directions; the authority spawns and expires them.
    Wanderer,
    /// An object of the area: its kind and its number among the area's
    /// instances (`crate::instance`), where its facts are.
    Object(ObjectKind, u32),
    /// A bolt in flight (`combat::Missile`).
    Missile,
}

/// What changes about an object of the area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ObjectState {
    /// A door's state: 0 closed, 1 and 2 open one way or the other (its
    /// collision follows it). A placeable's look (`look`). A sound object's: `sound::AS_PLACED` (its
    /// template's `Active`), `sound::PLAYING` or `sound::STOPPED` (a
    /// script's). A waypoint's map note: `pin::AS_PLACED` (its
    /// `MapNoteEnabled`), `pin::ON` or `pin::OFF` (`SetMapPinEnabled`). 0
    /// for the rest.
    pub state: u8,
    /// A sound object's volume a script set (1 + 0..=127); 0 its
    /// template's.
    pub volume: u8,
    /// A sound object's speed a script fixed (`SoundObjectSetFixedVariance`:
    /// 1 + hundredths); 0 its template's.
    pub pitch: u16,
    /// How many times scripts started a sound object (`SoundObjectPlay`),
    /// counting round: each start plays it from its first sound at once.
    pub plays: u8,
    /// How long a script's stop fades a sound object out (ms,
    /// `SoundObjectFadeAndStop`); 0 at once.
    pub fade: u16,
    /// Who stands in a trigger, or in an open exit door's doorway, or has
    /// a container open: bit n for seat n (so seats stop at `MAX_SEATS`),
    /// so entering fires once.
    pub tenants: u64,
    /// A door's or placeable's lock as scripts (`SetLocked`, the lock
    /// actions), keys and Security (`world::locks`) left it: `lock::PLACED`
    /// (its `Locked`), `lock::LOCKED` or `lock::UNLOCKED`.
    pub lock: u8,
    /// The hit points a door or placeable lost to blows and scripts'
    /// damage (`world::objects`); what it had is its data's.
    pub damage: u16,
    /// A trap's state the views need (`trap::DISABLED`,
    /// `trap::PARTY_SEES`); 0 for the rest.
    pub trap: u8,
}

/// `ObjectState::trap`'s bits.
pub mod trap {
    /// Disabled (`SetTrapDisabled`): it springs no more and is not drawn.
    pub const DISABLED: u8 = 1;
    /// The party detected it, or laid it: drawn for the players, and
    /// theirs to disarm or recover.
    pub const PARTY_SEES: u8 = 2;
    /// Every bit there is.
    pub(crate) const ALL: u8 = DISABLED | PARTY_SEES;
}

/// `ObjectState::lock`'s values.
pub mod lock {
    pub const PLACED: u8 = 0;
    pub const LOCKED: u8 = 1;
    pub const UNLOCKED: u8 = 2;
}

impl ObjectState {
    /// A placed object no longer in play (destroyed): its collision solid in
    /// no state (`collision::GONE`), a way through where it stood (the
    /// Unknown World's temple shield the ritual's script destroys).
    pub const GONE: ObjectState = ObjectState {
        state: collision::GONE,
        volume: 0,
        pitch: 0,
        plays: 0,
        fade: 0,
        tenants: 0,
        lock: 0,
        damage: 0,
        trap: 0,
    };

    /// Whether the door or placeable is locked: as it was last locked or
    /// unlocked, else as placed.
    pub fn locked(&self, placed: crate::module::Opens) -> bool {
        match self.lock {
            lock::LOCKED => true,
            lock::UNLOCKED => false,
            _ => placed == crate::module::Opens::Locked,
        }
    }

    /// Whether the door opens when used or run into: one that is not
    /// scenery and not locked.
    pub fn opens(&self, placed: crate::module::Opens) -> bool {
        placed != crate::module::Opens::Never && !self.locked(placed)
    }

    /// A door's or placeable's hit points now and at most: its data's,
    /// less what it lost; `None` where its data gives none.
    pub fn hit_points(&self, facts: &crate::module::ObjectData) -> Option<(i32, i32)> {
        facts
            .hit_points
            .map(|(now, most)| (now - i32::from(self.damage), most))
    }

    /// Broken by blows (`world::objects`): its hit points gone. A broken
    /// door stands open for good.
    pub fn broken(&self, facts: &crate::module::ObjectData) -> bool {
        self.damage > 0 && self.hit_points(facts).is_some_and(|(now, _)| now <= 0)
    }

    /// A waypoint's note on the map while it shows: as placed, else as a
    /// script last set it.
    pub fn map_note<'f>(
        &self,
        facts: &'f crate::module::ObjectData,
    ) -> Option<&'f crate::module::MapNote> {
        let note = facts.map_note.as_ref()?;
        match self.state {
            pin::ON => Some(note),
            pin::OFF => None,
            _ => note.enabled.then_some(note),
        }
    }

    /// Whether blows may break it open (`module::bashable`).
    pub fn bashable(&self, facts: &crate::module::ObjectData) -> bool {
        crate::module::bashable(facts, self.locked(facts.opens), self.hit_points(facts))
    }
}

/// A waypoint's map note as placed, or shown or hidden by a script
/// (`ObjectState::state`).
pub mod pin {
    pub const AS_PLACED: u8 = 0;
    pub const ON: u8 = 1;
    pub const OFF: u8 = 2;
}

/// A placeable's look (`ObjectState::state`): its `AnimationState` as
/// placed, then as the scripts' animations leave it (`look::of_animation`).
/// Each loops its model's clip of that name (`default`, `open`, `close`,
/// `dead`, `on`, `off`, `animloop01` on), a change playing its way there
/// first (`off2on`, `on2off`, `close2open`, `open2close`): the views'
/// (`launcher::own_fx`).
pub mod look {
    pub const DEFAULT: u8 = 0;
    pub const OPEN: u8 = 1;
    pub const CLOSED: u8 = 2;
    pub const DEAD: u8 = 3;
    pub const ON: u8 = 4;
    pub const OFF: u8 = 5;
    /// `animloop01` looping; `ANIMLOOP + n - 1` the `n`th, to the tenth.
    pub const ANIMLOOP: u8 = 16;
    /// The scripts' `ANIMATION_PLACEABLE_*` (200 on).
    const ACTIVATE: u16 = 200;
    const ANIMLOOPS: u16 = 10;

    /// The look `ANIMATION_PLACEABLE_*` constant `animation` leaves a
    /// placeable in: activate, deactivate, open, close, then its ten loops.
    pub fn of_animation(animation: u16) -> Option<u8> {
        match animation.checked_sub(ACTIVATE)? {
            0 => Some(ON),
            1 => Some(OFF),
            2 => Some(OPEN),
            3 => Some(CLOSED),
            n if n < 4 + ANIMLOOPS => Some(ANIMLOOP + (n - 4) as u8),
            _ => None,
        }
    }

    /// The name of the clip look `state` loops (`animations.2da`'s
    /// placeable rows).
    pub fn clip_name(state: u8) -> String {
        match state {
            OPEN => "open".into(),
            CLOSED => "close".into(),
            DEAD => "dead".into(),
            ON => "on".into(),
            OFF => "off".into(),
            n if n >= ANIMLOOP => format!("animloop{:02}", n - ANIMLOOP + 1),
            _ => "default".into(),
        }
    }

    /// A look a placeable may be placed in (its `AnimationState`).
    pub fn placed(state: u32) -> u8 {
        u8::try_from(state)
            .ok()
            .filter(|state| *state <= OFF)
            .unwrap_or(DEFAULT)
    }
}

/// What scripts set of the world's sound (the ambience's hook,
/// `docs/SOUND.md`): sound objects' states (`ObjectState`) and
/// the area's tracks (`AreaSound`). A view plays the area's own (its ARE,
/// its UTS) where these say nothing.
pub mod sound {
    /// As placed (the template's), started or stopped by a script.
    pub const AS_PLACED: u8 = 0;
    pub const PLAYING: u8 = 1;
    pub const STOPPED: u8 = 2;
}

/// A track of the area's (music, battle music, ambience) as scripts set
/// it: started or stopped (`sound::*`), the day's and night's track
/// (`ambientmusic.2da` / `ambientsound.2da` rows; `None` the ARE's), and
/// the volume (1 + 0..=127; 0 the ARE's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Track {
    pub state: u8,
    pub day: Option<u16>,
    pub night: Option<u16>,
    pub volume: u8,
}

/// A room's clip a script plays (`PlayRoomAnimation`: the turret's
/// skybox): the room by its name's hash (`room_key`), the clip's number
/// (`scriptloop0<n>`), when it began (world ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RoomAnim {
    pub room: u32,
    pub anim: u8,
    pub since_ms: u64,
}

/// A room's key in `RoomAnim`: FNV-1a over its lowercase name.
pub fn room_key(name: &str) -> u32 {
    name.trim()
        .bytes()
        .map(|b| b.to_ascii_lowercase())
        .fold(0x811c_9dc5u32, |hash, b| {
            (hash ^ u32::from(b)).wrapping_mul(0x0100_0193)
        })
}

/// The area's music, battle music and ambience as scripts set them, and
/// the room clip one last played. Reset on each install (KOTOR keeps it
/// per area: later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AreaSound {
    pub music: Track,
    pub battle: Track,
    pub ambient: Track,
    pub room: Option<RoomAnim>,
}

impl AreaSound {
    fn encode(&self, out: &mut Writer) {
        for track in [self.music, self.battle, self.ambient] {
            out.u8(track.state);
            out.u16(track.day.unwrap_or(u16::MAX));
            out.u16(track.night.unwrap_or(u16::MAX));
            out.u8(track.volume);
        }
        out.u8(u8::from(self.room.is_some()));
        if let Some(room) = self.room {
            out.u32(room.room);
            out.u8(room.anim);
            out.u64(room.since_ms);
        }
    }

    fn decode(input: &mut Reader) -> Result<AreaSound, DecodeError> {
        let mut track = || -> Result<Track, DecodeError> {
            Ok(Track {
                state: input.u8()?,
                day: Some(input.u16()?).filter(|t| *t != u16::MAX),
                night: Some(input.u16()?).filter(|t| *t != u16::MAX),
                volume: input.u8()?,
            })
        };
        let (music, battle, ambient) = (track()?, track()?, track()?);
        let room = match input.u8()? {
            0 => None,
            _ => Some(RoomAnim {
                room: input.u32()?,
                anim: input.u8()?,
                since_ms: input.u64()?,
            }),
        };
        Ok(AreaSound {
            music,
            battle,
            ambient,
            room,
        })
    }
}

/// The screen's fade as scripts last set it (`SetGlobalFadeOut`,
/// `SetGlobalFadeIn`), the same for everyone: from `start_ms` (sim ms) the
/// screen holds as covered as it was (`from`) for `wait_ms`, then ramps
/// from there to the colour over `length_ms` (out) or from there to clear
/// (in), and stays so: a fade in over one already lifting goes on from
/// where the screen is, never back to the colour (xoreos restarts it from
/// the colour; the owner saw that flash in the Dantooine council's end,
/// where its last script's fade in follows the montage's first line's).
/// The engine lifts a covering fade of its own accord (`lifted`): as a
/// module begins and as a conversation starts (`WorldFadeIn`), unless a
/// script locked it (`SetFadeUntilScript`, TSL).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fade {
    pub start_ms: u64,
    pub wait_ms: u32,
    pub length_ms: u32,
    /// Red, green and blue, 0 to 1.
    pub colour: [f32; 3],
    /// Covering (out) or uncovering (in).
    pub out: bool,
    /// How covered the screen was when it was set (0 clear, 1 the colour).
    pub from: f32,
    /// Only `SetGlobalFadeIn` lifts it (`SetFadeUntilScript`).
    pub locked: bool,
    /// A conversation's node set it (`NodeFade`): lifted as the
    /// conversation ends.
    pub conversation: bool,
}

/// How long the engine takes to lift a fade (ms) [verify in the game].
pub const LIFT_MS: u32 = rules::engine::shown::FADE_LIFT_MS;

impl Fade {
    const BLANK: Fade = Fade {
        start_ms: 0,
        wait_ms: 0,
        length_ms: 0,
        colour: [0.0; 3],
        out: false,
        from: 0.0,
        locked: false,
        conversation: false,
    };

    /// A fade set at `now_ms` from a screen covered `from`: after `wait`
    /// seconds over `length` (below 0 as 0), to (out) or from `colour`
    /// (each within 0 to 1).
    pub fn set(
        now_ms: u64,
        wait: f32,
        length: f32,
        colour: [f32; 3],
        out: bool,
        from: f32,
    ) -> Fade {
        let ms = |seconds: f32| (seconds.max(0.0) * 1000.0).round() as u32;
        Fade {
            start_ms: now_ms,
            wait_ms: ms(wait),
            length_ms: ms(length),
            colour: colour.map(|channel| channel.clamp(0.0, 1.0)),
            out,
            from,
            ..Fade::BLANK
        }
    }

    /// The fade a module loads under (`nwscript.nss`'s "LevelLoad fade"):
    /// black, covering from `now_ms`.
    pub fn level_load(now_ms: u64) -> Fade {
        Fade {
            start_ms: now_ms,
            out: true,
            from: 1.0,
            ..Fade::BLANK
        }
    }

    /// The fade as the engine leaves it lifting it at `now_ms`: a covering
    /// one not locked fades in from where it is over `LIFT_MS` (a clear
    /// screen stays clear); any other stands.
    pub fn lifted(self, now_ms: u64) -> Fade {
        if !self.out || self.locked {
            return self;
        }
        let alpha = self.alpha(now_ms);
        Fade {
            start_ms: now_ms,
            length_ms: LIFT_MS,
            colour: self.colour,
            from: alpha,
            ..Fade::BLANK
        }
    }

    /// How covered the screen is at `now_ms` (sim ms): 0 clear, 1 the
    /// colour.
    pub fn alpha(&self, now_ms: u64) -> f32 {
        let since = now_ms.saturating_sub(self.start_ms);
        let Some(run) = since.checked_sub(u64::from(self.wait_ms)) else {
            return self.from;
        };
        let done = match self.length_ms {
            0 => 1.0,
            length => (run as f32 / length as f32).min(1.0),
        };
        if self.out {
            self.from + (1.0 - self.from) * done
        } else {
            self.from * (1.0 - done)
        }
    }

    /// Whether the screen is clear at `now_ms` and stays so: a fade in run
    /// out. A covering one never is, waiting or done (the screen dark
    /// until a script lifts it).
    pub fn settled(&self, now_ms: u64) -> bool {
        !self.out && self.alpha(now_ms) <= 0.0
    }

    fn encode(&self, out: &mut Writer) {
        out.u64(self.start_ms);
        out.u32(self.wait_ms);
        out.u32(self.length_ms);
        for channel in self.colour {
            out.f32(channel);
        }
        out.u8(u8::from(self.out) | u8::from(self.locked) << 1 | u8::from(self.conversation) << 2);
        out.f32(self.from);
    }

    fn decode(input: &mut Reader) -> Result<Fade, DecodeError> {
        let (start_ms, wait_ms, length_ms) = (input.u64()?, input.u32()?, input.u32()?);
        let colour = [input.f32()?, input.f32()?, input.f32()?];
        let at = input.at();
        let flags = input.u8()?;
        if flags > 7 {
            return Err(input.invalid("fade flags", at));
        }
        Ok(Fade {
            start_ms,
            wait_ms,
            length_ms,
            colour,
            out: flags & 1 != 0,
            from: input.f32()?,
            locked: flags & 2 != 0,
            conversation: flags & 4 != 0,
        })
    }
}

/// Where the engine's fade in at a module's beginning stands
/// (`nwscript.nss`: `HoldWorldFadeInForDialog` holds "the fade in at the
/// begining of a module until a dialog starts").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorldFadeIn {
    /// Done, or no module began.
    #[default]
    Done,
    /// The module just began: once its arrival's scripts have run, the
    /// engine lifts the fade [verify in the game].
    AfterArrival,
    /// Held by `HoldWorldFadeInForDialog`: lifted when a conversation
    /// starts.
    AtDialog,
}

impl WorldFadeIn {
    const ALL: [WorldFadeIn; 3] = [
        WorldFadeIn::Done,
        WorldFadeIn::AfterArrival,
        WorldFadeIn::AtDialog,
    ];

    fn decode(input: &mut Reader) -> Result<WorldFadeIn, DecodeError> {
        let at = input.at();
        WorldFadeIn::ALL
            .get(usize::from(input.u8()?))
            .copied()
            .ok_or(input.invalid("world fade in", at))
    }
}

/// Which module a world is in: what a save or a client must install to
/// continue it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub game: String,
    pub module: String,
    /// `Level::digest`: the module's facts, collision and animation
    /// lengths together.
    pub digest: [u8; 32],
    /// The players the module was planned for (`ModuleData::players`, I30
    /// P21). Coming back to it, or loading a save made in it, plans it for
    /// as many again, so its digest and its creatures stay what they were.
    pub players: u8,
}

/// A module change a player asked for by walking into (or using) a door or
/// trigger that leads out, in the step that asked. The authority's host
/// installs it (the world waits for it meanwhile); an ask it cannot
/// install is dropped, and asked again while the player stays at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub seat: Seat,
    /// The door or trigger, or the object whose script started the module
    /// (`StartNewModule`).
    pub via: SimId,
    pub module: String,
    /// The door or waypoint to arrive at; empty for the module's entry.
    pub tag: String,
}

/// An exit a player walked into, waiting for the rest of the party (P16;
/// `world::party`): where it leads, the door or trigger, who asked, who is
/// ready (a bit per seat), and when it was asked (sim ms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyCheck {
    pub module: String,
    pub tag: String,
    pub via: SimId,
    pub asked_by: Seat,
    pub ready: u64,
    pub since_ms: u64,
}

impl ReadyCheck {
    fn encode(&self, out: &mut Writer) {
        out.string(&self.module);
        out.string(&self.tag);
        out.u32(self.via.0);
        out.u8(self.asked_by.0);
        out.u64(self.ready);
        out.u64(self.since_ms);
    }

    fn decode(input: &mut Reader) -> Result<ReadyCheck, DecodeError> {
        Ok(ReadyCheck {
            module: input.string()?,
            tag: input.string()?,
            via: SimId(input.u32()?),
            asked_by: Seat(input.u8()?),
            ready: input.u64()?,
            since_ms: input.u64()?,
        })
    }
}

/// What a player's body is in (R10), as its portrait's icon shows it:
/// asking the host in, playing pazaak, trading, talking, the first that
/// holds in that order (`Snapshot::busy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Talking,
    Shopping,
    Pazaak,
    Asking,
}

/// Why the host's keys on a join request's card wait (R5): an
/// exit's ready check is up (its ready key is the accept key by default),
/// the host does not stand (down, its follow key the end key), or the sim
/// keeps the request waiting: the party is in combat, or the host stands
/// in another locked conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestWait {
    ReadyCheck,
    Down,
    InCombat,
    Talking,
}

/// A module left, as it was: coming back finds it so (KOTOR keeps every
/// module's state in the save).
#[derive(Debug, Clone, PartialEq)]
pub struct Stashed {
    pub place: Place,
    /// Its objects, by id.
    pub entities: Vec<EntityState>,
    /// The objects made in it in play (`Snapshot::spawned`).
    pub spawned: Vec<Spawned>,
}

/// The modules left, by name, shared: they change only at an install, so a
/// snapshot's copy is a pointer, and their encoding and its BLAKE3 are made
/// once, when they are, not each tick. Equal when their encodings are. One
/// copy a process: a stash made or read that is one held already (a solo
/// player's client rebuilds its own host's) is that one (`LIVE`).
#[derive(Clone)]
pub struct Stash(Arc<Left>);

/// The stashes this process holds, by their digest.
static LIVE: Mutex<Vec<Weak<Left>>> = Mutex::new(Vec::new());

/// The live stash whose encoding's BLAKE3 is `digest`.
fn live(digest: &[u8; 32]) -> Option<Stash> {
    let mut live = LIVE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    live.retain(|left| left.strong_count() > 0);
    live.iter()
        .filter_map(Weak::upgrade)
        .find(|left| left.digest == *digest)
        .map(Stash)
}

struct Left {
    modules: Vec<Stashed>,
    /// Their encoding: what a snapshot's encoding ends with.
    bytes: Vec<u8>,
    digest: [u8; 32],
    /// One past the highest id stashed (0 for none), which a snapshot's
    /// next id must not be below.
    ids_end: u32,
}

impl Stash {
    pub fn new(modules: Vec<Stashed>) -> Stash {
        let mut out = Writer::default();
        out.u32(u32::try_from(modules.len()).expect("fewer than 2^32 modules"));
        for stashed in &modules {
            encode_place(Some(&stashed.place), &mut out);
            encode_entities(&stashed.entities, &mut out);
            encode_spawned(&stashed.spawned, &mut out);
        }
        let ids_end = modules
            .iter()
            .flat_map(|stashed| &stashed.entities)
            .map(|entity| entity.id.0.saturating_add(1))
            .max()
            .unwrap_or(0);
        let digest = *blake3::hash(&out.0).as_bytes();
        if let Some(held) = live(&digest) {
            return held;
        }
        let left = Arc::new(Left {
            digest,
            bytes: out.0,
            modules,
            ids_end,
        });
        LIVE.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(Arc::downgrade(&left));
        Stash(left)
    }

    /// The BLAKE3 of their encoding: what a snapshot's tail and hash carry
    /// for them.
    pub fn digest(&self) -> [u8; 32] {
        self.0.digest
    }

    /// Their encoding (`Snapshot::encode`'s last bytes).
    pub fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }

    /// The modules left that `bytes` encode (`bytes`), in name order: the
    /// stash held already when it is one (not read again).
    pub fn decode(bytes: &[u8]) -> Result<Stash, DecodeError> {
        if let Some(held) = live(blake3::hash(bytes).as_bytes()) {
            return Ok(held);
        }
        let mut input = Reader::new("stash", bytes);
        let stash = Stash::read(&mut input)?;
        input.finish()?;
        Ok(stash)
    }

    fn read(input: &mut Reader) -> Result<Stash, DecodeError> {
        let mut modules: Vec<Stashed> = Vec::new();
        for _ in 0..input.u32()? {
            let at = input.at();
            let place = decode_place(input)?.ok_or(input.invalid("stashed module", at))?;
            if modules
                .last()
                .is_some_and(|last| last.place.module >= place.module)
            {
                return Err(input.invalid("stashed module order", at));
            }
            // Below the snapshot's next id, which `Snapshot::decode` checks.
            let entities = decode_entities(input, u32::MAX)?;
            let spawned = decode_spawned(input)?;
            modules.push(Stashed {
                place,
                entities,
                spawned,
            });
        }
        Ok(Stash::new(modules))
    }
}

impl Default for Stash {
    fn default() -> Stash {
        Stash::new(Vec::new())
    }
}

impl From<Vec<Stashed>> for Stash {
    fn from(modules: Vec<Stashed>) -> Stash {
        Stash::new(modules)
    }
}

impl std::ops::Deref for Stash {
    type Target = [Stashed];

    fn deref(&self) -> &[Stashed] {
        &self.0.modules
    }
}

impl PartialEq for Stash {
    fn eq(&self, other: &Stash) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.digest == other.0.digest
    }
}

impl Eq for Stash {}

impl fmt::Debug for Stash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// An object made in play (I19: a companion, `CreateObject`): its kind,
/// template and facing as made (degrees; a creature's own turns with it).
/// Those of an area are its instances after the module's placements, in the
/// order made (`crate::instance::Instances`); their facts are the
/// template's (the script library's creatures and placeables), a
/// placeable's facing its own; creatures' sheets ride in the snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Spawned {
    pub kind: ObjectKind,
    pub template: String,
    pub yaw: f32,
    /// What a container made holding its own goods holds as made, in
    /// place of its template's (a dead creature's remains,
    /// `world::remains`): each template and stack; empty for the rest.
    pub holds: Vec<(String, u16)>,
}

fn encode_spawned(spawned: &[Spawned], out: &mut Writer) {
    out.u32(spawned.len() as u32);
    for made in spawned {
        out.u8(made.kind.index());
        out.string(&made.template);
        out.f32(made.yaw);
        out.u32(made.holds.len() as u32);
        for (template, stack) in &made.holds {
            out.string(template);
            out.u16(*stack);
        }
    }
}

fn decode_spawned(input: &mut Reader) -> Result<Vec<Spawned>, DecodeError> {
    (0..input.u32()?)
        .map(|_| {
            let at = input.at();
            let kind = ObjectKind::ALL
                .get(usize::from(input.u8()?))
                .copied()
                .ok_or(input.invalid("spawned kind", at))?;
            Ok(Spawned {
                kind,
                template: input.string()?,
                yaw: input.f32()?,
                holds: (0..input.u32()?)
                    .map(|_| Ok((input.string()?, input.u16()?)))
                    .collect::<Result<_, DecodeError>>()?,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityState {
    pub id: SimId,
    pub kind: Kind,
    /// Game space (Z up), metres. A player's is its movement box's origin,
    /// 24 units (0.61 m) above its soles.
    pub position: [f32; 3],
    /// Metres per second.
    pub velocity: [f32; 3],
    /// A character's movement state, in JA units (the position and
    /// velocity above are it in metres).
    pub player: Option<MotionState>,
    /// Wanderers: milliseconds until the next change of heading.
    pub retarget_ms: u32,
    /// Milliseconds until the entity is removed; `None` stays.
    pub life_ms: Option<u32>,
    /// With a lifetime, how it is drawn going (`Going`; none: it is not).
    pub going: Option<Going>,
    /// An object's changing state.
    pub object: Option<ObjectState>,
    /// A character's fight: players and creatures.
    pub fighter: Option<Fighter>,
    /// A player's sheet; a creature's once its equipment changed (else its
    /// module's data).
    pub sheet: Option<Sheet>,
    /// A bolt's flight.
    pub missile: Option<Missile>,
}

/// How a thing a destroy takes out of the world is drawn going (ms): as
/// KOTOR's client fades the model it still holds once the server removed
/// the object (`DestroyObject`'s `fDelayUntilFade`, the dead's remains),
/// whole for `whole_ms`, then fading out over `fade_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Departure {
    pub whole_ms: u32,
    pub fade_ms: u32,
}

/// How an entity with a lifetime is drawn going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Going {
    /// Fading out over the last `ms` of its life, in the world until it
    /// ends: a dead body taken away (`world::remains`), as KOTOR's client
    /// fades a body from the fade delay its death set while the server
    /// still holds it.
    Fades(u32),
    /// Out of the world when its life ends, then drawn departing
    /// (`Departing`): what `DestroyObject` takes.
    Departs(Departure),
}

impl Going {
    /// How opaque one with `life_ms` left is drawn in the world.
    pub fn alpha(self, life_ms: u32) -> f32 {
        match self {
            Going::Fades(fade_ms) if fade_ms > 0 => (life_ms as f32 / fade_ms as f32).min(1.0),
            _ => 1.0,
        }
    }

    fn encode(going: Option<&Going>, out: &mut Writer) {
        let (tag, a, b) = match going {
            None => (0, 0, 0),
            Some(Going::Fades(ms)) => (1, *ms, 0),
            Some(Going::Departs(departure)) => (2, departure.whole_ms, departure.fade_ms),
        };
        out.u8(tag);
        out.u32(a);
        out.u32(b);
    }

    fn decode(input: &mut Reader) -> Result<Option<Going>, DecodeError> {
        let at = input.at();
        match (input.u8()?, input.u32()?, input.u32()?) {
            (0, 0, 0) => Ok(None),
            (1, ms, 0) => Ok(Some(Going::Fades(ms))),
            (2, whole_ms, fade_ms) => Ok(Some(Going::Departs(Departure { whole_ms, fade_ms }))),
            _ => Err(input.invalid("going", at)),
        }
    }
}

/// A thing out of the world, still drawn as it goes (`Departure`): as it
/// was when it went, and how long it is drawn yet (ms), over the last
/// `fade_ms` of them fading out. Nothing in the world meets it: it is only
/// the view's.
#[derive(Debug, Clone, PartialEq)]
pub struct Departing {
    pub entity: EntityState,
    pub left_ms: u32,
    pub fade_ms: u32,
}

impl Departing {
    /// One that went as `entity` is, going as `departure` says.
    pub fn new(mut entity: EntityState, departure: Departure) -> Self {
        (entity.life_ms, entity.going) = (None, None);
        Departing {
            entity,
            left_ms: departure.whole_ms + departure.fade_ms,
            fade_ms: departure.fade_ms,
        }
    }

    /// How opaque it is drawn: 1 whole, 0 gone.
    pub fn alpha(&self) -> f32 {
        if self.fade_ms == 0 {
            return 1.0;
        }
        (self.left_ms as f32 / self.fade_ms as f32).min(1.0)
    }
}

impl EntityState {
    /// Whether it is a character: a player or a creature, moving by move_body
    /// and fighting.
    pub fn is_actor(&self) -> bool {
        is_actor(self.kind)
    }
}

/// Players and creatures are characters.
pub(crate) fn is_actor(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Avatar(_) | Kind::Object(ObjectKind::Creature, _)
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// The tick this state is the result of; 0 before the first step.
    pub tick: u64,
    /// Sim time at the end of `tick`, the sum of the steps' milliseconds.
    pub time_ms: u64,
    /// The module the world is in; `None` for a level without one.
    pub place: Option<Place>,
    /// Ordered by id.
    pub entities: Vec<EntityState>,
    /// A module change asked for in this step.
    pub transition: Option<Transition>,
    /// The modules left, by name.
    pub stash: Stash,
    /// The area's sound as scripts set it.
    pub area_sound: AreaSound,
    /// The screen's fade as scripts and the engine last set it; `None`
    /// clear.
    pub fade: Option<Fade>,
    /// The engine's fade in at the module's beginning.
    pub world_fade_in: WorldFadeIn,
    /// The video effect scripts enabled (`EnableVideoEffect`), a
    /// `videoeffects.2da` row, until they disable it
    /// (`DisableVideoEffect`); kept across modules.
    pub video_effect: Option<u16>,
    /// Conversations under way, barks, custom tokens.
    pub talk: Told,
    /// The party's gold and items, the trades under way and their stock.
    pub goods: Goods,
    /// The objects made in play in this module.
    pub spawned: Vec<Spawned>,
    /// The journal and the party's XP.
    pub progress: Progress,
    /// Who each seat's player is, by seat (`persona`).
    pub personas: Vec<crate::persona::Persona>,
    /// The seats the session holds (`WorldConfig::seats`): its players and
    /// its guests, a body or not (I30 P1).
    pub seats_held: u8,
    /// When the whole party went down (sim ms): the loss (I30 P13).
    pub lost: Option<u64>,
    /// The party is in combat (`world::party`: KOTOR's "enemies nearby"),
    /// as the step left it.
    pub in_combat: bool,
    /// A break holds, called by this seat (P10): steps change nothing but
    /// their tick until it ends.
    pub paused_by: Option<Seat>,
    /// An exit a player walked into, waiting for the others (P16).
    pub ready: Option<ReadyCheck>,
    /// The companions kept for the seats that drove them, by seat
    /// (`world::drivers`).
    pub kept: Vec<Kept>,
    /// The companion leading the party (`world::lent`: the host's portrait
    /// click or a script's `SetPartyLeader`), which the first seat moves
    /// while its own character stands.
    pub leading: Option<SimId>,
    /// The minigame in play, in a minigame's module.
    pub minigame: Option<crate::minigame::Course>,
    /// The pazaak matches under way (`PlayPazaak`), each its player's
    /// (`pazaak_of`, I30 P20).
    pub pazaak: Vec<crate::pazaak::Match>,
    /// What the party explored of each area's map, by module, here and in
    /// the modules left (`fog`).
    pub explored: Vec<crate::fog::Explored>,
    /// The scripts' visual effects on the module's doors and placeables,
    /// until they end (`visuals`).
    pub visuals: Vec<crate::visuals::OnObject>,
    /// The things destroys took out of the world still drawn going, by id
    /// (`Departing`): the view's alone.
    pub departing: Vec<Departing>,
    /// The in-game screens as scripts leave them (`Screens`).
    pub screens: Screens,
    pub(crate) shared: SimRng,
    pub(crate) authority: SimRng,
    pub(crate) next_id: u32,
}

/// A companion kept for the seat that drove it (I31): its `NPC_*` number
/// and when the seat last drove it (sim ms). For `world::drivers::KEEP_MS`
/// from then the seat takes it again when it speaks, and no other seat
/// does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kept {
    pub seat: Seat,
    pub npc: u8,
    pub at_ms: u64,
}

/// BLAKE3 of a snapshot's encoding but the stash's bytes, which the
/// stash's digest in its tail stands for: equal hashes mean equal states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotHash(pub [u8; 32]);

impl SnapshotHash {
    /// The first 8 bytes in hex, enough to tell runs apart in a report.
    pub fn short(&self) -> String {
        self.0[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

impl fmt::Display for SnapshotHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Snapshot {
    /// BLAKE3 of the encoding up to the stash's bytes (their digest is in
    /// the tail), so it costs the live state, however many modules are left.
    pub fn hash(&self) -> SnapshotHash {
        SnapshotHash(*blake3::hash(&self.encode_state()).as_bytes())
    }

    pub fn get(&self, id: SimId) -> Option<&EntityState> {
        self.entities
            .binary_search_by_key(&id, |entity| entity.id)
            .ok()
            .map(|at| &self.entities[at])
    }

    /// Entity `id` as drawn: in the world, or departing (`departing`).
    pub fn drawn(&self, id: SimId) -> Option<&EntityState> {
        self.get(id).or_else(|| {
            self.departing
                .binary_search_by_key(&id, |going| going.entity.id)
                .ok()
                .map(|at| &self.departing[at].entity)
        })
    }

    /// Every entity drawn: those in the world, then those departing.
    pub fn all_drawn(&self) -> impl Iterator<Item = &EntityState> {
        self.entities
            .iter()
            .chain(self.departing.iter().map(|going| &going.entity))
    }

    /// The body `seat`'s player moves: the creature it drives (a
    /// companion, I30 P2; for the first seat, the one it is switched to,
    /// `world::switch`, its own character benched meanwhile), for the first
    /// seat the companion leading the party (`leading`), else its avatar;
    /// `None` while it watches (P3).
    pub fn body(&self, seat: Seat) -> Option<&EntityState> {
        self.driven(seat)
            .or_else(|| {
                self.leading
                    .filter(|_| seat == crate::world::LEADER_SEAT)
                    .and_then(|id| self.get(id))
            })
            .or_else(|| self.own(seat))
    }

    /// Whether a conversation holds the world (S1, `Told::holds_world` of
    /// `seat_bodies`), as the sim's step had it.
    pub fn holds_world(&self) -> bool {
        !self.talk.conversations.is_empty() && self.talk.holds_world(&self.seat_bodies())
    }

    /// Each seat's body as the sim collects them (`scene::seat_bodies`):
    /// its `body`, which is the one it acts through (`acting_body`, the
    /// companion leading the party for seat 0); none for a watching seat.
    /// The drivers' probe `seat_bodies_agree` holds the two together.
    pub fn seat_bodies(&self) -> Vec<ObjectId> {
        let seats =
            (0..self.seats()).filter_map(|seat| Some((Seat(seat), self.body(Seat(seat))?.id)));
        crate::world::seat_bodies(seats, None)
    }

    /// The creature `seat` drives (`Mind::driver`), if it drives one.
    fn driven(&self, seat: Seat) -> Option<&EntityState> {
        self.entities.iter().find(|entity| {
            matches!(entity.kind, Kind::Object(ObjectKind::Creature, _))
                && entity.fighter.is_some_and(|f| f.mind.driver == Some(seat))
        })
    }

    /// Whether `entity` is in the party as the sim has it
    /// (`world::party_member`): a seat's own character benched while the
    /// seat plays another (`world::switch`) is not; one standing while a
    /// companion leads is.
    pub fn in_party(&self, entity: &EntityState) -> bool {
        let benched = match entity.kind {
            Kind::Avatar(seat) => self.driven(seat).is_some(),
            _ => false,
        };
        crate::world::party_member(entity) && !benched
    }

    /// The template of what character `holder` wears in `slot`
    /// (`inventory::slots`), as the goods tell it: the party's members
    /// whole, the others in the module their weapons and body armour.
    pub fn worn(&self, holder: u32, slot: u8) -> Option<&str> {
        self.goods
            .items
            .iter()
            .find(|(_, item)| item.holder == holder && item.slot == Some(slot))
            .map(|(_, item)| item.template.as_str())
    }

    /// `seat`'s own character (its avatar), whatever body it moves.
    pub fn own(&self, seat: Seat) -> Option<&EntityState> {
        self.entities
            .iter()
            .find(|entity| entity.kind == Kind::Avatar(seat))
    }

    /// The conversation `seat`'s screen shows (R1): the one its body stands
    /// in, else a party conversation, which every seat sees, a watching one
    /// too.
    pub fn talk_for(&self, seat: Seat) -> Option<&Conversation> {
        self.stands_in(seat).or_else(|| self.talk.party())
    }

    /// The conversation `seat`'s body stands in (R3), if any.
    pub fn stands_in(&self, seat: Seat) -> Option<&Conversation> {
        self.body(seat)
            .and_then(|body| self.talk.holding(ObjectId(body.id.0)))
    }

    /// Whether `seat`'s player may leave `talk` (R6), the Leave row's state:
    /// none while one seat alone plays a body (one player plays as before:
    /// the sim's `together`), watching, or in a party conversation it is not
    /// the listener of (only its listener leaves one); else whether it is
    /// unlocked. The host's own character stands for its seat too while a
    /// companion it leads acts for it (an accepted request's listener).
    pub fn may_leave(&self, seat: Seat, talk: &Conversation) -> Option<bool> {
        let own = (seat == crate::world::LEADER_SEAT)
            .then(|| self.own(seat))
            .flatten();
        let stands = [self.body(seat), own].into_iter().flatten().any(|body| {
            let body = ObjectId(body.id.0);
            if talk.party {
                talk.listener == body
            } else {
                talk.holds(body)
            }
        });
        (self.together() && stands).then_some(!talk.locked)
    }

    /// More than one seat plays a body, as the sim's `View::together` has
    /// it: watching seats count nothing.
    pub fn together(&self) -> bool {
        (0..self.seats())
            .filter(|&seat| self.body(Seat(seat)).is_some())
            .count()
            > 1
    }

    /// The pazaak match `seat` plays (`pazaak::played_by`), paused or not.
    pub fn pazaak_of(&self, seat: Seat) -> Option<&crate::pazaak::Match> {
        let driven =
            (0..self.seats()).filter_map(|seat| Some((Seat(seat), self.body(Seat(seat))?.id.0)));
        crate::pazaak::played_by(&self.pazaak, seat, driven)
    }

    /// The video effect `seat`'s view takes (a `videoeffects.2da` row): its
    /// body's effects' (TSL's Force Sight, else Fury: `Effects::video_row`,
    /// as the executable's party update picks for the character the player
    /// controls), else the one scripts enabled.
    pub fn video_effect_for(&self, seat: Seat) -> Option<u16> {
        self.body(seat)
            .and_then(|body| body.fighter.as_ref())
            .and_then(|fighter| fighter.effects.video_row(self.time_ms))
            .or(self.video_effect)
    }

    /// The pazaak match `seat`'s body watches (R9a), if any: a body standing
    /// by a table, not a seat watching with no body of its own (I30 P3).
    pub fn watching(&self, seat: Seat) -> Option<&crate::pazaak::Match> {
        let body = self.body(seat)?.id.0;
        self.pazaak
            .iter()
            .find(|table| table.watchers.contains(&body))
    }

    /// What `seat`'s body is in (R10), if anything; none while the seat
    /// watches the game with no body (I30 P3) or its body is down. A body
    /// watching a match (`watching`) is not busy (R9a).
    pub fn busy(&self, seat: Seat) -> Option<Busy> {
        let body = self.standing(seat)?.id;
        let id = ObjectId(body.0);
        if self.talk.conversations.iter().any(|c| c.asking == id) {
            Some(Busy::Asking)
        } else if self.pazaak_of(seat).is_some() {
            Some(Busy::Pazaak)
        } else if self.goods.trade(body.0).is_some() {
            Some(Busy::Shopping)
        } else if self.talk_for(seat).is_some() {
            Some(Busy::Talking)
        } else {
            None
        }
    }

    /// Whether `seat`'s player takes `talk`'s story replies itself (I30
    /// P6): its body is the conversation's player character, or it is the
    /// host's, whose body the sim always counts as one (its own character,
    /// the body a script switched it to, the companion it leads through).
    /// Its screen draws them plain and sends them as replies; a seat
    /// watching takes none.
    pub fn takes_story(&self, seat: Seat, talk: &Conversation) -> bool {
        self.body(seat)
            .is_some_and(|body| seat == crate::world::LEADER_SEAT || body.id.0 == talk.pc.0)
    }

    /// Whether `seat`'s pick of a story reply in `talk` asks the host in
    /// (R5), as the sim decides it: its body stands in that private
    /// conversation and no player character does (the host's body, any
    /// player's own character). Elsewhere a screen only points at it (I30
    /// P6): a player character's pick would take it.
    pub fn asks_host(&self, seat: Seat, talk: &Conversation) -> bool {
        let Some(body) = self.body(seat) else {
            return false;
        };
        let host = self.body(crate::world::LEADER_SEAT).map(|host| host.id);
        let player = |id: &ObjectId| {
            Some(SimId(id.0)) == host
                || self
                    .get(SimId(id.0))
                    .is_some_and(|entity| matches!(entity.kind, Kind::Avatar(_)))
        };
        !talk.party
            && talk.holds(ObjectId(body.id.0))
            && !std::iter::once(&talk.listener)
                .chain(&talk.joined)
                .any(player)
    }

    /// Why the host's keys on a join request's card wait, if they do: a
    /// ready check is up, the host does not stand, or the sim would drop the
    /// answer to the first asking: the party is in combat, or one of the
    /// host's bodies (its character, the companion it leads) stands in
    /// another locked conversation.
    pub fn request_wait(&self) -> Option<RequestWait> {
        let host = crate::world::LEADER_SEAT;
        let first = self
            .talk
            .conversations
            .iter()
            .find(|c| c.asking != ObjectId::INVALID)
            .map(|c| c.id);
        let locked_in = |body: &EntityState| {
            self.talk
                .conversations
                .iter()
                .any(|c| Some(c.id) != first && c.locked && c.holds(ObjectId(body.id.0)))
        };
        if self.ready.is_some() {
            Some(RequestWait::ReadyCheck)
        } else if self.standing(host).is_none() {
            Some(RequestWait::Down)
        } else if self.in_combat {
            Some(RequestWait::InCombat)
        } else if [self.body(host), self.own(host)]
            .into_iter()
            .flatten()
            .any(locked_in)
        {
            Some(RequestWait::Talking)
        } else {
            None
        }
    }

    /// Why `seat` cannot ready for an exit (`party::kept`), if it cannot:
    /// only while more than one seat's body stands (R11), as the ready
    /// check itself keeps seats (`party::ready`).
    pub fn kept_from_exit(&self, seat: Seat) -> Option<&'static str> {
        let standing = (0..self.seats())
            .filter(|&seat| self.standing(Seat(seat)).is_some())
            .count();
        if standing < 2 {
            return None;
        }
        let body = self.body(seat)?.id;
        crate::world::party::kept(&self.talk.conversations, &self.pazaak, body)
    }

    /// `seat`'s body while it stands: none when it is down or dead, or the
    /// seat has no body (I30 P11: a downed player watches through the
    /// follow camera).
    pub fn standing(&self, seat: Seat) -> Option<&EntityState> {
        self.body(seat)
            .filter(|body| body.fighter.is_none_or(|f| f.alive()))
    }

    /// How many avatars are here (players with a body of their own).
    pub fn avatars(&self) -> u8 {
        let avatars = self
            .entities
            .iter()
            .filter(|entity| matches!(entity.kind, Kind::Avatar(_)))
            .count();
        u8::try_from(avatars).unwrap_or(u8::MAX)
    }

    /// The seats the session holds (`seats_held`): how many the server
    /// admits and an install makes room for.
    pub fn seats(&self) -> u8 {
        self.seats_held
    }

    /// The players driving a body (I30 P21): the host, always one, and
    /// each other seat held (`seats_held`) whose `body` is not dead, or
    /// whose companion is kept for it while it is away (`kept`). A watching
    /// seat counts nothing. Where every seat has an avatar, that is every
    /// seat.
    pub fn players_driving(&self) -> u8 {
        let guests = (1..self.seats_held)
            .filter(|&seat| {
                self.body(Seat(seat))
                    .is_some_and(|body| body.fighter.is_none_or(|f| f.life != Life::Dead))
                    || self.kept.iter().any(|kept| kept.seat == Seat(seat))
            })
            .count();
        u8::try_from(guests + 1).unwrap_or(u8::MAX)
    }

    /// The players `module` was planned for, if this world is in it or
    /// left it (`Place::players`). A module is counted once, when it is
    /// first planned: its stash, a save made in it and a reinstall from its
    /// own link plan it for as many again.
    pub fn planned_players(&self, module: &str) -> Option<u8> {
        self.place
            .iter()
            .chain(self.stash.iter().map(|stashed| &stashed.place))
            .find(|place| place.module.eq_ignore_ascii_case(module))
            .map(|place| place.players)
    }

    /// What a plan of `module` from this world is for: the count it was
    /// planned for before, else the players driving a body now.
    pub fn players_for(&self, module: &str) -> u8 {
        self.planned_players(module)
            .unwrap_or_else(|| self.players_driving())
    }

    /// Entities by kind (`avatar`, `wanderer`, or an object kind's name),
    /// here and in the modules left: what a lifecycle run counts.
    pub fn census(&self) -> Census {
        let mut census = Census::default();
        for entity in &self.entities {
            *census.live.entry(kind_name(entity.kind)).or_insert(0) += 1;
        }
        for stashed in self.stash.iter() {
            census
                .stashed
                .insert(stashed.place.module.clone(), stashed.entities.len());
        }
        census
    }

    /// The whole state: what `hash` covers, then the stash's bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.encode_state();
        out.extend_from_slice(self.stash.bytes());
        out
    }

    /// The encoding up to the stash's bytes.
    fn encode_state(&self) -> Vec<u8> {
        let mut out = Writer::default();
        self.encode_head(&mut out);
        encode_entities(&self.entities, &mut out);
        self.encode_tail(&mut out);
        out.0
    }

    /// The encoding cut where the network diffs it (`Parts`); `join` makes
    /// the same snapshot again.
    pub fn split(&self) -> Parts {
        let mut head = Writer::default();
        self.encode_head(&mut head);
        let mut tail = Writer::default();
        self.encode_tail(&mut tail);
        Parts {
            head: head.0,
            entities: self
                .entities
                .iter()
                .map(|entity| (entity.id, Snapshot::record(entity)))
                .collect(),
            tail: tail.0,
            stash: self.stash.clone(),
        }
    }

    /// One entity's record, as `split` cuts it.
    pub fn record(entity: &EntityState) -> Vec<u8> {
        let mut record = Writer::default();
        encode_entity(entity, &mut record);
        record.0
    }

    /// The snapshot `parts` cut from: their bytes joined and decoded, so a
    /// snapshot built from parts passes every check a decoded one does. The
    /// stash is the parts' own, shared, not decoded again.
    pub fn join(parts: &Parts) -> Result<Snapshot, DecodeError> {
        Snapshot::read(&parts.state(), Some(&parts.stash))
    }

    fn encode_head(&self, out: &mut Writer) {
        out.bytes(MAGIC);
        out.u32(VERSION);
        out.u64(self.tick);
        out.u64(self.time_ms);
        self.shared.encode(out);
        self.authority.encode(out);
        out.u32(self.next_id);
        out.u8(self.seats_held);
        encode_place(self.place.as_ref(), out);
        self.area_sound.encode(out);
        write_fixed(out, self.fade.as_ref(), &Fade::BLANK, Fade::encode);
        out.u8(self.world_fade_in as u8);
        write_fixed(out, self.video_effect.as_ref(), &0, |row, out| {
            out.u16(*row)
        });
    }

    fn encode_tail(&self, out: &mut Writer) {
        match &self.transition {
            Some(transition) => {
                out.u8(1);
                out.u8(transition.seat.0);
                out.u32(transition.via.0);
                out.string(&transition.module);
                out.string(&transition.tag);
            }
            None => out.u8(0),
        }
        out.bytes(&self.stash.digest());
        self.talk.encode(out);
        encode_spawned(&self.spawned, out);
        self.goods.encode(out);
        self.progress.encode(out);
        crate::persona::encode(&self.personas, out);
        write_fixed(out, self.lost.as_ref(), &0, |ms, out| out.u64(*ms));
        out.u8(u8::from(self.in_combat));
        match self.paused_by {
            Some(seat) => {
                out.u8(1);
                out.u8(seat.0);
            }
            None => out.u8(0),
        }
        match &self.ready {
            Some(check) => {
                out.u8(1);
                check.encode(out);
            }
            None => out.u8(0),
        }
        out.u8(u8::try_from(self.kept.len()).expect("fewer kept than seats"));
        for kept in &self.kept {
            out.u8(kept.seat.0);
            out.u8(kept.npc);
            out.u64(kept.at_ms);
        }
        write_fixed(out, self.leading.as_ref(), &SimId(0), |id, out| {
            out.u32(id.0);
        });
        match &self.minigame {
            Some(course) => {
                out.u8(1);
                course.encode(out);
            }
            None => out.u8(0),
        }
        crate::pazaak::encode_tables(&self.pazaak, out);
        crate::fog::encode(&self.explored, out);
        crate::visuals::encode(&self.visuals, out);
        out.u32(u32::try_from(self.departing.len()).expect("fewer than 2^32 departing"));
        for going in &self.departing {
            encode_entity(&going.entity, out);
            out.u32(going.left_ms);
            out.u32(going.fade_ms);
        }
        self.screens.encode(out);
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        Snapshot::read(bytes, None)
    }

    /// Decodes `bytes`: a whole encoding, or with `stash` given, the
    /// encoding up to the stash's bytes (`Parts::state`), whose digest must
    /// be the one the tail carries.
    fn read(bytes: &[u8], stash: Option<&Stash>) -> Result<Self, DecodeError> {
        let mut input = Reader::new("snapshot", bytes);
        input.header(MAGIC, VERSION)?;
        let tick = input.u64()?;
        let time_ms = input.u64()?;
        let shared = SimRng::decode(&mut input)?;
        let authority = SimRng::decode(&mut input)?;
        let next_id = input.u32()?;
        let at = input.at();
        let seats_held = input.u8()?;
        if seats_held > crate::MAX_SEATS {
            return Err(input.invalid("seats held", at));
        }
        let place = decode_place(&mut input)?;
        let area_sound = AreaSound::decode(&mut input)?;
        let fade = read_fixed(&mut input, &Fade::BLANK, Fade::decode, "fade flag")?;
        let world_fade_in = WorldFadeIn::decode(&mut input)?;
        let video_effect = read_fixed(&mut input, &0, |input| input.u16(), "video effect flag")?;
        let entities = decode_entities(&mut input, next_id)?;
        let at = input.at();
        let transition = match input.u8()? {
            0 => None,
            1 => Some(Transition {
                seat: Seat(input.u8()?),
                via: SimId(input.u32()?),
                module: input.string()?,
                tag: input.string()?,
            }),
            _ => return Err(input.invalid("transition flag", at)),
        };
        let digest_at = input.at();
        let digest: [u8; 32] = input.array()?;
        let talk = Told::decode(&mut input)?;
        let spawned = decode_spawned(&mut input)?;
        let goods = Goods::decode(&mut input)?;
        let progress = Progress::decode(&mut input)?;
        let personas = crate::persona::decode(&mut input)?;
        let lost = read_fixed(&mut input, &0, |input| input.u64(), "lost flag")?;
        let at = input.at();
        let in_combat = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err(input.invalid("in combat flag", at)),
        };
        let at = input.at();
        let paused_by = match input.u8()? {
            0 => None,
            1 => Some(Seat(input.u8()?)),
            _ => return Err(input.invalid("break flag", at)),
        };
        let at = input.at();
        let ready = match input.u8()? {
            0 => None,
            1 => Some(ReadyCheck::decode(&mut input)?),
            _ => return Err(input.invalid("ready check flag", at)),
        };
        // By seat, one each, below `MAX_SEATS`: one state, one encoding.
        let mut kept: Vec<Kept> = Vec::new();
        for _ in 0..input.u8()? {
            let at = input.at();
            let seat = Seat(input.u8()?);
            if seat.0 >= crate::MAX_SEATS || kept.last().is_some_and(|last| last.seat >= seat) {
                return Err(input.invalid("kept seat", at));
            }
            kept.push(Kept {
                seat,
                npc: input.u8()?,
                at_ms: input.u64()?,
            });
        }
        let leading = read_fixed(
            &mut input,
            &SimId(0),
            |input| Ok(SimId(input.u32()?)),
            "leading flag",
        )?;
        let at = input.at();
        let minigame = match input.u8()? {
            0 => None,
            1 => Some(crate::minigame::Course::decode(&mut input)?),
            _ => return Err(input.invalid("minigame flag", at)),
        };
        let pazaak = crate::pazaak::decode_tables(&mut input)?;
        let explored = crate::fog::decode(&mut input)?;
        let visuals = crate::visuals::decode(&mut input)?;
        let mut departing: Vec<Departing> = Vec::new();
        for _ in 0..input.u32()? {
            let at = input.at();
            let entity = decode_entity(&mut input, departing.last().map(|d| &d.entity), next_id)?;
            let (left_ms, fade_ms) = (input.u32()?, input.u32()?);
            if left_ms == 0 || entity.life_ms.is_some() {
                return Err(input.invalid("departing", at));
            }
            departing.push(Departing {
                entity,
                left_ms,
                fade_ms,
            });
        }
        let screens = Screens::decode(&mut input)?;
        let stash = match stash {
            Some(stash) => stash.clone(),
            None => Stash::read(&mut input)?,
        };
        if stash.digest() != digest {
            return Err(input.invalid("stash digest", digest_at));
        }
        if stash.0.ids_end > next_id {
            return Err(input.invalid("stashed entity id", digest_at));
        }
        input.finish()?;
        Ok(Snapshot {
            tick,
            time_ms,
            place,
            entities,
            transition,
            stash,
            area_sound,
            fade,
            world_fade_in,
            video_effect,
            talk,
            goods,
            spawned,
            progress,
            personas,
            lost,
            in_combat,
            paused_by,
            ready,
            kept,
            leading,
            minigame,
            pazaak,
            explored,
            visuals,
            departing,
            screens,
            shared,
            authority,
            next_id,
            seats_held,
        })
    }
}

/// The story's progress as everyone sees it (the scripts' journal and the
/// party's XP, `script::journal`, `script::xp`): what the journal page, the
/// character sheet and the HUD read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    /// The entry each quest has reached, the latest updated last.
    pub journal: Vec<(String, i32)>,
    pub xp: i32,
    /// The levels taken in play, by character id: its classes and levels
    /// in each (`State::careers`), what the level-up screen offers from.
    pub careers: std::collections::BTreeMap<u32, crate::levels::Career>,
    /// The XP the party's kills gave in the last `Kill::SHOWN_MS`, oldest
    /// first: KOTOR floats each over its dead.
    pub kills: Vec<Kill>,
    /// What the scripts told the party in the last `Status::KEPT_MS`,
    /// oldest first: KOTOR's status summary gathers them.
    pub statuses: Vec<Status>,
    /// The feedback floating over objects (`DisplayFeedBackText`) in the
    /// last `Feedback::SHOWN_MS`, oldest first.
    pub feedback: Vec<Feedback>,
}

/// A line of feedback floating over an object (`DisplayFeedBackText`: a
/// `feedbacktext.2da` row's talk-table line, "Resisted", "Immune", "Saved"):
/// over whom (its id), the strref (0: the row has none, which KOTOR shows
/// as "BAD STRREF"), and when (sim ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feedback {
    pub over: u32,
    pub strref: u32,
    pub at_ms: u64,
}

impl Feedback {
    /// How long KOTOR floats it (`CSWCObject::SetFeedbackInfo`'s).
    pub const SHOWN_MS: u64 = rules::engine::shown::FEEDBACK_MS;
}

/// The in-game screens as scripts leave them (I35, `docs/UI.md`): TSL's
/// map jammed (`DisableMap`: the party table's `PT_DISABLEMAP`; the map page
/// shows static, the HUD no minimap), the map page's return button
/// (`Transit`), and whether TSL's Bao-Dur has been met (his influence set:
/// the items he cannot equip say so, `cannot_equip`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Screens {
    pub map_disabled: bool,
    pub transit: Transit,
    pub bao_dur_met: bool,
}

/// What TSL's executable adds to the description of an item Bao-Dur cannot
/// equip once he has been met (`CSWSItem::AddCNPCRequirements`): the base
/// items it names (`baseitems.2da`: the Jedi robes 35 to 37, 98 the armoured
/// flight suit, 99 the restrictive robe, 102 the Zeison Sha armour, and 97,
/// a sonic grenade, which its range takes too: QUIRKS.md Q-0266), and the
/// lines, "Cannot Equip" and "Bao-Dur" (talk-table strrefs).
pub mod cannot_equip {
    pub const BASE_ITEMS: [u16; 7] = [35, 36, 37, 97, 98, 99, 102];
    pub const CANNOT_EQUIP: u32 = 130590;
    pub const BAO_DUR: u32 = 102045;

    /// The lines before the rest of `base_item`'s description: "Cannot
    /// Equip:", the companion's name, then a blank line, as the
    /// executable writes them (`line` reads a strref); none for an item of
    /// another base item, or before Bao-Dur is met.
    pub fn lines(base_item: u16, met: bool, line: impl Fn(u32) -> Vec<u8>) -> Vec<u8> {
        if !met || !BASE_ITEMS.contains(&base_item) {
            return Vec::new();
        }
        let mut text = line(CANNOT_EQUIP);
        text.extend_from_slice(b":\n");
        text.extend(line(BAO_DUR));
        text.extend_from_slice(b"\n\n");
        text
    }
}

/// The map page's return button as the scripts leave it: whether it may
/// show (`SetReturnStrref`'s `bShow`), its label and its confirm box's
/// question (strrefs; 0 none), and whether the area keeps the party from
/// leaving (`SetDisableTransit`, the ARE's `DisableTransit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Transit {
    pub show: bool,
    pub label: u32,
    pub query: u32,
    pub disabled: bool,
}

impl Screens {
    fn encode(&self, out: &mut Writer) {
        let Transit {
            show,
            label,
            query,
            disabled,
        } = self.transit;
        out.u8(u8::from(self.map_disabled)
            | u8::from(show) << 1
            | u8::from(disabled) << 2
            | u8::from(self.bao_dur_met) << 3);
        out.u32(label);
        out.u32(query);
    }

    fn decode(input: &mut Reader) -> Result<Screens, DecodeError> {
        let at = input.at();
        let flags = input.u8()?;
        if flags > 15 {
            return Err(input.invalid("screens flags", at));
        }
        Ok(Screens {
            map_disabled: flags & 1 != 0,
            bao_dur_met: flags & 8 != 0,
            transit: Transit {
                show: flags & 2 != 0,
                disabled: flags & 4 != 0,
                label: input.u32()?,
                query: input.u32()?,
            },
        })
    }
}

/// A death's XP for the party: who died (its id), what it gave, and when
/// (sim ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kill {
    pub dead: u32,
    pub xp: i32,
    pub at_ms: u64,
}

impl Kill {
    /// How long KOTOR shows a kill's XP over its dead.
    pub const SHOWN_MS: u64 = rules::engine::shown::KILL_XP_MS;
}

/// What KOTOR's status summary tells the party of (its engine raises one
/// where a script gives or takes from a player's character, not where the
/// party loots, trades or kills): a journal entry added, XP or credits
/// received (lost, if negative), light or dark side points, an item (its
/// template) received, items lost; TSL's influence a companion (`NPC_*`)
/// gained or lost; TSL's most Force points raised or lowered (bonus Force
/// points: the summary adds `by` to the one or the other, as its executable
/// does, and shows the line while that is not 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusKind {
    Journal,
    Xp(i32),
    Credits(i32),
    Alignment { light: bool },
    Received(String),
    Lost,
    Influence { npc: u8, gained: bool },
    MaxForcePoints { gained: bool, by: i32 },
}

/// A status, and when it was raised (sim ms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub kind: StatusKind,
    pub at_ms: u64,
}

impl Status {
    /// How long a status stays in the snapshot: long enough for every
    /// player's view to have seen it.
    pub const KEPT_MS: u64 = 3000;

    fn encode(&self, out: &mut Writer) {
        match &self.kind {
            StatusKind::Journal => out.u8(0),
            StatusKind::Xp(xp) => {
                out.u8(1);
                out.u32(*xp as u32);
            }
            StatusKind::Credits(credits) => {
                out.u8(2);
                out.u32(*credits as u32);
            }
            StatusKind::Alignment { light } => {
                out.u8(3);
                out.u8(u8::from(*light));
            }
            StatusKind::Received(template) => {
                out.u8(4);
                out.string(template);
            }
            StatusKind::Lost => out.u8(5),
            StatusKind::Influence { npc, gained } => {
                out.u8(6);
                out.bytes(&[*npc, u8::from(*gained)]);
            }
            StatusKind::MaxForcePoints { gained, by } => {
                out.u8(7);
                out.u8(u8::from(*gained));
                out.u32(*by as u32);
            }
        }
        out.u64(self.at_ms);
    }

    fn decode(input: &mut Reader) -> Result<Status, DecodeError> {
        let at = input.at();
        let kind = match input.u8()? {
            0 => StatusKind::Journal,
            1 => StatusKind::Xp(input.u32()? as i32),
            2 => StatusKind::Credits(input.u32()? as i32),
            3 => StatusKind::Alignment {
                light: input.u8()? != 0,
            },
            4 => StatusKind::Received(input.string()?),
            5 => StatusKind::Lost,
            6 => {
                let [npc, gained] = input.array()?;
                StatusKind::Influence {
                    npc,
                    gained: gained != 0,
                }
            }
            7 => StatusKind::MaxForcePoints {
                gained: input.u8()? != 0,
                by: input.u32()? as i32,
            },
            _ => return Err(input.invalid("status", at)),
        };
        Ok(Status {
            kind,
            at_ms: input.u64()?,
        })
    }
}

impl Progress {
    fn encode(&self, out: &mut Writer) {
        crate::script::journal::encode_journal(out, &self.journal);
        out.u32(self.xp as u32);
        out.u32(self.careers.len() as u32);
        for (id, career) in &self.careers {
            out.u32(*id);
            out.u8(u8::try_from(career.len()).expect("fewer classes than 256"));
            for (class, levels) in career {
                out.u8(*class);
                out.u8(*levels);
            }
        }
        out.u32(self.kills.len() as u32);
        for kill in &self.kills {
            out.u32(kill.dead);
            out.u32(kill.xp as u32);
            out.u64(kill.at_ms);
        }
        out.u32(self.statuses.len() as u32);
        for status in &self.statuses {
            status.encode(out);
        }
        out.u32(self.feedback.len() as u32);
        for feedback in &self.feedback {
            out.u32(feedback.over);
            out.u32(feedback.strref);
            out.u64(feedback.at_ms);
        }
    }

    fn decode(input: &mut Reader) -> Result<Progress, DecodeError> {
        let journal = crate::script::journal::decode_journal(input)?;
        let xp = input.u32()? as i32;
        let mut careers = std::collections::BTreeMap::new();
        for _ in 0..input.u32()? {
            let at = input.at();
            let id = input.u32()?;
            let career = (0..input.u8()?)
                .map(|_| Ok((input.u8()?, input.u8()?)))
                .collect::<Result<_, DecodeError>>()?;
            // By id, one each: one state, one encoding.
            if careers
                .last_key_value()
                .is_some_and(|(last, _)| *last >= id)
            {
                return Err(input.invalid("career id", at));
            }
            careers.insert(id, career);
        }
        let kills = (0..input.u32()?)
            .map(|_| {
                Ok(Kill {
                    dead: input.u32()?,
                    xp: input.u32()? as i32,
                    at_ms: input.u64()?,
                })
            })
            .collect::<Result<_, DecodeError>>()?;
        let statuses = (0..input.u32()?)
            .map(|_| Status::decode(input))
            .collect::<Result<_, DecodeError>>()?;
        let feedback = (0..input.u32()?)
            .map(|_| {
                Ok(Feedback {
                    over: input.u32()?,
                    strref: input.u32()?,
                    at_ms: input.u64()?,
                })
            })
            .collect::<Result<_, DecodeError>>()?;
        Ok(Progress {
            journal,
            xp,
            careers,
            kills,
            statuses,
            feedback,
        })
    }
}

/// A snapshot's encoding in four cuts: the head (signature, tick, time,
/// random streams, next id, module), each live entity's record by id (in
/// id order), the tail (the transition asked for, the modules left's
/// digest, what the party shares), and the modules left, shared with the
/// snapshot (`Stash`). Every record of a kind has one length, so two
/// snapshots' records line up byte for byte: what the network's deltas XOR.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Parts {
    pub head: Vec<u8>,
    pub entities: Vec<(SimId, Vec<u8>)>,
    pub tail: Vec<u8>,
    pub stash: Stash,
}

impl Parts {
    /// The whole encoding (`Snapshot::encode`'s bytes).
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = self.state();
        out.extend_from_slice(self.stash.bytes());
        out
    }

    /// The snapshot's hash (`Snapshot::hash`): of the head, the records
    /// and the tail, not the stash's bytes.
    pub fn hash(&self) -> SnapshotHash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.head);
        hasher.update(&self.count().to_le_bytes());
        for (_, record) in &self.entities {
            hasher.update(record);
        }
        hasher.update(&self.tail);
        SnapshotHash(*hasher.finalize().as_bytes())
    }

    /// The encoding up to the stash's bytes.
    fn state(&self) -> Vec<u8> {
        let records: usize = self.entities.iter().map(|(_, record)| record.len()).sum();
        let mut out = Vec::with_capacity(self.head.len() + 4 + records + self.tail.len());
        out.extend_from_slice(&self.head);
        out.extend_from_slice(&self.count().to_le_bytes());
        for (_, record) in &self.entities {
            out.extend_from_slice(record);
        }
        out.extend_from_slice(&self.tail);
        out
    }

    fn count(&self) -> u32 {
        u32::try_from(self.entities.len()).expect("fewer than 2^32 entities")
    }
}

/// Entity counts: live by kind, and per module left.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Census {
    pub live: std::collections::BTreeMap<&'static str, usize>,
    pub stashed: std::collections::BTreeMap<String, usize>,
}

impl Census {
    pub fn live_total(&self) -> usize {
        self.live.values().sum()
    }

    pub fn stashed_total(&self) -> usize {
        self.stashed.values().sum()
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Avatar(_) => "avatar",
        Kind::Wanderer => "wanderer",
        Kind::Missile => "bolt",
        Kind::Object(kind, _) => kind.name(),
    }
}

fn encode_place(place: Option<&Place>, out: &mut Writer) {
    match place {
        Some(place) => {
            out.u8(1);
            out.string(&place.game);
            out.string(&place.module);
            out.bytes(&place.digest);
            out.u8(place.players);
        }
        None => out.u8(0),
    }
}

fn decode_place(input: &mut Reader) -> Result<Option<Place>, DecodeError> {
    let at = input.at();
    Ok(match input.u8()? {
        0 => None,
        1 => {
            let (game, module, digest) = (input.string()?, input.string()?, input.array()?);
            let at = input.at();
            let players = input.u8()?;
            if players == 0 {
                return Err(input.invalid("players", at));
            }
            Some(Place {
                game,
                module,
                digest,
                players,
            })
        }
        _ => return Err(input.invalid("module flag", at)),
    })
}

fn encode_entities(entities: &[EntityState], out: &mut Writer) {
    out.u32(u32::try_from(entities.len()).expect("fewer than 2^32 entities"));
    for entity in entities {
        encode_entity(entity, out);
    }
}

fn encode_entity(entity: &EntityState, out: &mut Writer) {
    out.u32(entity.id.0);
    match entity.kind {
        Kind::Avatar(seat) => {
            out.u8(0);
            out.u8(seat.0);
        }
        Kind::Wanderer => {
            out.u8(1);
            out.u8(0);
        }
        Kind::Missile => {
            out.u8(3);
            out.u8(0);
        }
        Kind::Object(kind, index) => {
            out.u8(2);
            out.u8(kind.index());
            out.u32(index);
        }
    }
    for value in entity.position.iter().chain(&entity.velocity) {
        out.f32(*value);
    }
    out.u32(entity.retarget_ms);
    match &entity.player {
        Some(player) => {
            out.u8(1);
            encode_player(player, out);
        }
        None => out.u8(0),
    }
    match entity.life_ms {
        Some(life) => {
            out.u8(1);
            out.u32(life);
            Going::encode(entity.going.as_ref(), out);
        }
        None => {
            out.u8(0);
            out.u32(0);
        }
    }
    match entity.object {
        Some(object) => {
            out.u8(1);
            out.u8(object.state);
            out.u8(object.volume);
            out.u16(object.pitch);
            out.u8(object.plays);
            out.u16(object.fade);
            out.u64(object.tenants);
            out.u8(object.lock);
            out.u16(object.damage);
            out.u8(object.trap);
        }
        None => out.u8(0),
    }
    match &entity.fighter {
        Some(fighter) => {
            out.u8(1);
            fighter.encode(out);
        }
        None => out.u8(0),
    }
    match &entity.sheet {
        Some(sheet) => {
            out.u8(1);
            sheet.encode(out);
        }
        None => out.u8(0),
    }
    match &entity.missile {
        Some(missile) => {
            out.u8(1);
            missile.encode(out);
        }
        None => out.u8(0),
    }
}

fn decode_entities(input: &mut Reader, next_id: u32) -> Result<Vec<EntityState>, DecodeError> {
    let count = input.u32()?;
    let mut entities: Vec<EntityState> = Vec::new();
    for _ in 0..count {
        let entity = decode_entity(input, entities.last(), next_id)?;
        entities.push(entity);
    }
    Ok(entities)
}

/// One entity, its id after `last`'s and below `next_id`.
fn decode_entity(
    input: &mut Reader,
    last: Option<&EntityState>,
    next_id: u32,
) -> Result<EntityState, DecodeError> {
    let at = input.at();
    let id = SimId(input.u32()?);
    if last.is_some_and(|last| last.id >= id) || id.0 >= next_id {
        return Err(input.invalid("entity id order", at));
    }
    let at = input.at();
    let kind = match (input.u8()?, input.u8()?) {
        (0, seat) => Kind::Avatar(Seat(seat)),
        (1, 0) => Kind::Wanderer,
        (3, 0) => Kind::Missile,
        (2, kind) => Kind::Object(
            ObjectKind::from_index(kind).ok_or(input.invalid("object kind", at))?,
            input.u32()?,
        ),
        _ => return Err(input.invalid("entity kind", at)),
    };
    let mut values = [0.0; 6];
    for value in &mut values {
        *value = input.f32()?;
    }
    let retarget_ms = input.u32()?;
    let at = input.at();
    let player = match input.u8()? {
        0 => None,
        1 => Some(decode_player(input)?),
        _ => return Err(input.invalid("player flag", at)),
    };
    if player.is_some() != is_actor(kind) {
        return Err(input.invalid("player state", at));
    }
    let at = input.at();
    let (life_ms, going) = match (input.u8()?, input.u32()?) {
        (1, life) => (Some(life), Going::decode(input)?),
        (0, 0) => (None, None),
        _ => return Err(input.invalid("entity lifetime", at)),
    };
    // Wanderers expire, and objects a script destroys after a delay
    // (`DestroyObject`, the dead fading); a player never does.
    if life_ms.is_some() && matches!(kind, Kind::Avatar(_)) {
        return Err(input.invalid("entity lifetime", at));
    }
    let at = input.at();
    let object = match input.u8()? {
        0 => None,
        1 => Some(ObjectState {
            state: input.u8()?,
            volume: input.u8()?,
            pitch: input.u16()?,
            plays: input.u8()?,
            fade: input.u16()?,
            tenants: input.u64()?,
            lock: {
                let at = input.at();
                match input.u8()? {
                    lock @ (lock::PLACED | lock::LOCKED | lock::UNLOCKED) => lock,
                    _ => return Err(input.invalid("lock", at)),
                }
            },
            damage: input.u16()?,
            trap: {
                let at = input.at();
                match input.u8()? {
                    bits if bits & !trap::ALL == 0 => bits,
                    _ => return Err(input.invalid("trap", at)),
                }
            },
        }),
        _ => return Err(input.invalid("object flag", at)),
    };
    if object.is_some() != matches!(kind, Kind::Object(..)) {
        return Err(input.invalid("object state", at));
    }
    let at = input.at();
    let fighter = match input.u8()? {
        0 => None,
        1 => Some(Fighter::decode(input)?),
        _ => return Err(input.invalid("fighter flag", at)),
    };
    if fighter.is_some() != is_actor(kind) {
        return Err(input.invalid("fighter", at));
    }
    let at = input.at();
    let sheet = match input.u8()? {
        0 => None,
        1 => Some(Sheet::decode(input)?),
        _ => return Err(input.invalid("sheet flag", at)),
    };
    // A player always has one; a creature has one once its equipment
    // changed (I19); nothing else does.
    let required = matches!(kind, Kind::Avatar(_));
    if (required && sheet.is_none()) || (sheet.is_some() && !is_actor(kind)) {
        return Err(input.invalid("sheet", at));
    }
    let at = input.at();
    let missile = match input.u8()? {
        0 => None,
        1 => Some(Missile::decode(input)?),
        _ => return Err(input.invalid("missile flag", at)),
    };
    if missile.is_some() != (kind == Kind::Missile) {
        return Err(input.invalid("missile", at));
    }
    Ok(EntityState {
        id,
        kind,
        position: [values[0], values[1], values[2]],
        velocity: [values[3], values[4], values[5]],
        player,
        retarget_ms,
        life_ms,
        going,
        object,
        fighter,
        sheet,
        missile,
    })
}

fn encode_part(part: &Part, out: &mut Writer) {
    out.u16(part.anim.index());
    out.u32(part.timer as u32);
    out.u8(u8::from(part.flip));
}

fn decode_part(input: &mut Reader) -> Result<Part, DecodeError> {
    let at = input.at();
    let anim = Anim::from_index(input.u16()?).ok_or(input.invalid("animation", at))?;
    let timer = input.u32()? as i32;
    let at = input.at();
    let flip = match input.u8()? {
        0 => false,
        1 => true,
        _ => return Err(input.invalid("animation flip", at)),
    };
    Ok(Part { anim, timer, flip })
}

fn encode_player(player: &MotionState, out: &mut Writer) {
    for value in player
        .origin
        .iter()
        .chain(&player.velocity)
        .chain(&player.view)
    {
        out.f32(*value);
    }
    for angle in player.view_offset.iter().chain(&player.command_angles) {
        out.u16(*angle as u16);
    }
    for step in player.command_aim {
        out.i8(step);
    }
    match player.ground {
        None => {
            out.u8(0);
            out.u32(0);
        }
        Some(Ground::World) => {
            out.u8(1);
            out.u32(0);
        }
        Some(Ground::Entity(entity)) => {
            out.u8(2);
            out.u32(entity);
        }
    }
    out.u16(player.flags);
    out.u32(player.hold_ms as u32);
    encode_part(&player.legs, out);
    encode_part(&player.torso, out);
    out.u8(player.push_dir);
    out.u8(u8::from(player.in_air_anim));
    out.u8(player.footstep_cycle);
    for value in [
        player.maxs_z,
        player.eye_height,
        player.takeoff_z,
        player.run_speed,
        player.gravity,
    ] {
        out.f32(value);
    }
    out.u64(player.slope_next_ms);
    out.u8(player.events);
    out.u64(player.next_use_ms);
}

fn decode_player(input: &mut Reader) -> Result<MotionState, DecodeError> {
    let mut vectors = [0.0f32; 9];
    for value in &mut vectors {
        *value = input.f32()?;
    }
    let mut angles = [0i16; 6];
    for angle in &mut angles {
        *angle = input.u16()? as i16;
    }
    let mut command_aim = [0i8; 3];
    for step in &mut command_aim {
        *step = input.i8()?;
    }
    let at = input.at();
    let ground = match (input.u8()?, input.u32()?) {
        (0, 0) => None,
        (1, 0) => Some(Ground::World),
        (2, entity) => Some(Ground::Entity(entity)),
        _ => return Err(input.invalid("ground", at)),
    };
    let flags = input.u16()?;
    let hold_ms = input.u32()? as i32;
    let legs = decode_part(input)?;
    let torso = decode_part(input)?;
    let push_dir = input.u8()?;
    let at = input.at();
    let in_air_anim = match input.u8()? {
        0 => false,
        1 => true,
        _ => return Err(input.invalid("in-air flag", at)),
    };
    let footstep_cycle = input.u8()?;
    let mut scalars = [0.0f32; 5];
    for value in &mut scalars {
        *value = input.f32()?;
    }
    let slope_next_ms = input.u64()?;
    let events = input.u8()?;
    let next_use_ms = input.u64()?;
    Ok(MotionState {
        origin: [vectors[0], vectors[1], vectors[2]],
        velocity: [vectors[3], vectors[4], vectors[5]],
        view: [vectors[6], vectors[7], vectors[8]],
        view_offset: [angles[0], angles[1], angles[2]],
        command_angles: [angles[3], angles[4], angles[5]],
        command_aim,
        ground,
        flags,
        hold_ms,
        legs,
        torso,
        push_dir,
        in_air_anim,
        footstep_cycle,
        maxs_z: scalars[0],
        eye_height: scalars[1],
        takeoff_z: scalars[2],
        run_speed: scalars[3],
        gravity: scalars[4],
        slope_next_ms,
        events,
        next_use_ms,
    })
}

/// What render, UI and audio read: the last two snapshots and how far wall
/// time is between them. They never see a `SimWorld`. The snapshots are
/// shared (`Arc`): a snapshot is large (every entity's fight and sheet
/// inline), and copying one cost as much as making it.
#[derive(Debug, Clone)]
pub struct Presented {
    previous: Arc<Snapshot>,
    current: Arc<Snapshot>,
    alpha: f32,
}

/// An entity as drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentedEntity {
    pub id: SimId,
    pub kind: Kind,
    /// Blended between the last two snapshots.
    pub position: [f32; 3],
}

impl Presented {
    pub fn new(first: Snapshot) -> Self {
        Presented::shared(Arc::new(first))
    }

    /// `new` with a snapshot shared already.
    pub fn shared(first: Arc<Snapshot>) -> Self {
        Presented {
            previous: first.clone(),
            current: first,
            alpha: 1.0,
        }
    }

    /// A new tick's snapshot: the current one becomes the previous.
    pub fn push(&mut self, snapshot: Snapshot) {
        self.push_shared(Arc::new(snapshot));
    }

    /// `push` with a snapshot shared already.
    pub fn push_shared(&mut self, snapshot: Arc<Snapshot>) {
        self.previous = std::mem::replace(&mut self.current, snapshot);
    }

    /// The current snapshot, shared.
    pub fn current_shared(&self) -> &Arc<Snapshot> {
        &self.current
    }

    /// The previous snapshot, shared.
    pub fn previous_shared(&self) -> &Arc<Snapshot> {
        &self.previous
    }

    /// From 0 (show the previous snapshot) to 1 (show the current one).
    pub fn set_alpha(&mut self, alpha: f32) {
        self.alpha = alpha.clamp(0.0, 1.0);
    }

    pub fn current(&self) -> &Snapshot {
        &self.current
    }

    /// The snapshot before the current one.
    pub fn previous(&self) -> &Snapshot {
        &self.previous
    }

    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// The current snapshot's entities, each blended from where it was in
    /// the previous one; a new entity shows where it is now.
    pub fn entities(&self) -> impl Iterator<Item = PresentedEntity> + '_ {
        self.current.entities.iter().map(|entity| {
            let position = match self.previous.get(entity.id) {
                Some(before) => std::array::from_fn(|axis| {
                    before.position[axis]
                        + (entity.position[axis] - before.position[axis]) * self.alpha
                }),
                None => entity.position,
            };
            PresentedEntity {
                id: entity.id,
                kind: entity.kind,
                position,
            }
        })
    }
}
