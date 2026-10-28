//! The cinematic conversation in `kjk play` (render side only: it reads the
//! presented snapshots' conversations, `Snapshot::talk`):
//!
//! - KOTOR's conversation screen (`ui::screens::dialog`, drawn by
//!   `ui_view`): the line, developer notes (`{…}`) left out and `<CUSTOMn>`
//!   tokens filled in, in the bottom bar while it plays, in the top bar
//!   with the replies below once it has played;
//! - the player's way through it (`ReplyInput`, sent by
//!   `sim_host::Controls`), as reone's: while the line plays, a left
//!   click anywhere, Enter or Space ends it (where the conversation allows
//!   it); on the player's turn, hovering or Up and Down highlight a reply,
//!   a click on one, Enter or Space takes the highlighted one, 1 to 9 take
//!   theirs. The pointer is free while the player is in a conversation; a
//!   screen open over it (the menu) takes the input instead;
//! - in a session of more than one seat, the reply highlighted pointed at
//!   (`Pick::Point`) and the names of the seats pointing at each reply
//!   beside it; any player driving a body answers, the first answer
//!   taking the line, but a guest only points at the replies the host
//!   takes (the story's, `dialogue::story`), drawn faded (I30 P6), and
//!   picking one in its own conversation asks the host in (R5);
//! - in such a session, the Leave row under the replies and its key (L
//!   unless rebound) while the player may leave (`leave`, R6): its own
//!   conversation ends, one it joined goes on without it;
//! - the host's request card (R5, `ui::screens::dialog::Dialog::card`)
//!   while a guest asks it in, talking or not: Y lets the first in, N ends
//!   its conversation (unless rebound; `Action::Request`);
//! - a watching seat sees the conversation holding the party member its
//!   camera follows, else a party one (`shown`);
//! - the line's voice from its speaker (`Sound`, else `VO_ResRef`, as the
//!   install has them; KOTOR's MP3s behind a RIFF header are unwrapped),
//!   cued at the moment the line was shown; a line cut short silences it;
//! - the speaker's lips: its head (else its body, one made with its face,
//!   `mouth_clip`) plays its `talk` clip as sixteen mouth
//!   shapes spaced evenly over the clip, the line's LIP keys picking two
//!   and the blend between them as the voice goes (reone's reading),
//!   held at the first and last keys; the head rests when the line ends.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use asset_odyssey::anim::AnimationIr;
use asset_odyssey::model::ModelIr;
use audio::{Channel, Clip};
use bevy::prelude::*;
use fmt_odyssey::ResType;
use fmt_odyssey::dlg::Node;
use fmt_odyssey::lip::{self, Lip};
use fmt_odyssey::tlk::Tlk;
use script_ncs::vm::ObjectId;
use sim::action::Action;
use sim::script::dialogue::{Conversation, Pick, Told};
use sim::{Kind, SimId, Snapshot};
use ui::vm::{DialogView, RequestsView};
use vfs::resolver::Resolver;
use vfs::{Discovery, Game};

use crate::lifecycle::Installer;
use crate::scene::{Animator, SceneContent, SceneModel};
use crate::settings::{Action as KeyAction, KeyMap};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap, view_ms};
use crate::sound::{Added, Cue, Cues, Forget, Hush};
use crate::ui_view::{Ui, UiOpen};

const DIGITS: [(KeyCode, KeyCode); 9] = [
    (KeyCode::Digit1, KeyCode::Numpad1),
    (KeyCode::Digit2, KeyCode::Numpad2),
    (KeyCode::Digit3, KeyCode::Numpad3),
    (KeyCode::Digit4, KeyCode::Numpad4),
    (KeyCode::Digit5, KeyCode::Numpad5),
    (KeyCode::Digit6, KeyCode::Numpad6),
    (KeyCode::Digit7, KeyCode::Numpad7),
    (KeyCode::Digit8, KeyCode::Numpad8),
    (KeyCode::Digit9, KeyCode::Numpad9),
];
pub(crate) const CONFIRM: [KeyCode; 3] = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space];

/// The player's way through the screen: whether it is in a conversation,
/// the reply highlighted, and an answer made and not yet sent
/// (`sim_host::Controls` takes it); in a session of more than one seat,
/// the reply last pointed at, the line last answered (a line is its
/// conversation and when it was shown), which takes no more, and the
/// line's replies this player only points at (`host_only`).
#[derive(Resource, Default)]
pub(crate) struct ReplyInput {
    pub active: bool,
    pub focus: u8,
    pub pick: Option<Pick>,
    pointed: Option<(Line, u8)>,
    answered: Option<Line>,
    host_only: Option<(Line, Vec<bool>)>,
}

/// A line: its conversation, and when it was shown.
type Line = (u32, u64);

impl ReplyInput {
    /// The replies of the line `talk` shows that this player only points
    /// at, as last worked out for it (`answer` works it out once a line).
    fn host_only(&self, talk: &Conversation) -> &[bool] {
        match &self.host_only {
            Some((line, only)) if *line == (talk.id, talk.shown_ms) => only,
            _ => &[],
        }
    }
}

/// The talk table, read once; the install, for each module's voices and
/// lips.
pub struct Setup {
    tlk: Option<Arc<Tlk>>,
    game: Game,
    install: Option<Arc<Installer>>,
}

/// `game`'s talk table (it may be missing: the screen then shows no
/// words), from `installer` (the stage's, its talk table already read by
/// the plan), else from an installer opened here.
pub fn load(discovery: &Discovery, game: Game, installer: Option<Arc<Installer>>) -> Setup {
    let install = installer.or_else(|| {
        let anims = Arc::new(sim::motion::AnimTable::stand_in());
        Installer::new(discovery, game, anims).ok().map(Arc::new)
    });
    let tlk = install
        .as_ref()
        .and_then(|install| install.base_resolver().ok())
        .and_then(|resolver| resolver.talk.get());
    Setup { tlk, game, install }
}

impl Setup {
    /// The install's base resolver, if it opened.
    pub fn base(&self) -> Option<&Resolver> {
        self.install.as_ref()?.base_resolver().ok()
    }
}

/// The conversation screen's systems; its setup comes with a session's
/// HUD (`install`).
pub struct DialogueViewPlugin;

/// A session's conversation screen, put into `app`.
pub fn install(app: &mut World, setup: Setup) {
    app.insert_resource(Screen {
        tlk: setup.tlk,
        showing: None,
        marks: None,
        game: setup.game,
        install: setup.install,
        module: None,
        voiced: VecDeque::new(),
        speaking: None,
        lip: None,
        face: None,
        models: BTreeMap::new(),
    });
}

impl Plugin for DialogueViewPlugin {
    fn build(&self, app: &mut App) {
        let in_play = resource_exists::<PresentedSnapshot>;
        app.init_resource::<ReplyInput>()
            .init_resource::<Watching>()
            .init_resource::<Called>()
            .add_systems(
                Update,
                (answer, requests_keys, show)
                    .chain()
                    .run_if(in_play)
                    .before(crate::ui_view::Stepped),
            )
            .add_systems(
                PostUpdate,
                // The mouth over the face its body's clips posed (`face`).
                lips.run_if(in_play)
                    .after(crate::play_view::Drawn)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

#[derive(Resource)]
pub(crate) struct Screen {
    pub(crate) tlk: Option<Arc<Tlk>>,
    /// The line shown (conversation, entry, when, whether its reply is
    /// taken by itself), so its voice is cued once.
    showing: Option<(u32, u32, u64, bool)>,
    /// The seats' marks (the conversation, the line shown, the marks) and
    /// their names by reply, as last shown.
    marks: Option<(Marked, Names)>,
    game: Game,
    install: Option<Arc<Installer>>,
    /// The module's resolver (its voices' lips), by scene swap.
    module: Option<(u32, Resolver)>,
    /// Voices read into the sound bank, the oldest first (`VOICES_KEPT`),
    /// and who speaks the one playing.
    voiced: VecDeque<String>,
    speaking: Option<SimId>,
    /// The line's mouth shapes, from when it was shown (ms).
    lip: Option<(Lip, u64)>,
    /// The speaker's mouth posed with its `talk` clip.
    face: Option<Face>,
    models: BTreeMap<String, Option<ModelIr>>,
}

/// A speaker's head and its `talk` clip bound to it.
struct Face {
    speaker: SimId,
    /// Its head's talk clip; looked for again while there is none and the
    /// scene gains models (a creature made in play spawns frames after it
    /// came, `made`).
    talk: Option<Animator>,
    models: usize,
    /// The head's pose is the clip's (not its rest's).
    posed: bool,
}

/// A node's text as the game's bytes: its own string, else its strref's
/// (`strref`'s words); developer notes (`{…}`) out, `<CUSTOMn>` filled in.
pub(crate) fn node_text(
    node: &Node,
    strref: &dyn Fn(u32) -> Option<Vec<u8>>,
    fill: Fill,
) -> Vec<u8> {
    let raw = match node.text.strings.first() {
        Some((_, own)) if !own.is_empty() => own.clone(),
        _ => strref(node.text.strref).unwrap_or_else(Vec::new),
    };
    said(&raw, fill)
}

/// What a line's tokens are filled with: the scripts' custom tokens, and
/// the player it is said to, if one (`player_token`).
#[derive(Clone, Copy)]
pub(crate) struct Fill<'a> {
    pub told: &'a Told,
    pub pc: Option<&'a sim::persona::Persona>,
}

impl<'a> Fill<'a> {
    /// For the player whose character is `pc` in `snapshot`.
    pub fn to(snapshot: &'a sim::Snapshot, pc: script_ncs::vm::ObjectId) -> Fill<'a> {
        Fill {
            told: &snapshot.talk,
            pc: crate::persona::of_character(snapshot, pc.0),
        }
    }
}

/// The game's own tokens of the player a line is said to: `<FullName>` and
/// `<FirstName>` its name (a player has a first name only), a pair such as
/// `<man/woman>` or `<sir/madam>` the first for a man, the second for a
/// woman.
fn player_token(token: &str, pc: &sim::persona::Persona) -> Option<String> {
    match token {
        "FullName" | "FirstName" => Some(pc.name.clone()),
        _ => {
            let (man, woman) = token.split_once('/')?;
            let pair = [man, woman];
            pair.iter()
                .all(|word| word.bytes().all(|b| b.is_ascii_alphabetic()))
                .then(|| pair[usize::from(pc.female())].to_string())
        }
    }
}

/// A line's words as shown: developer notes (`{…}`) out
/// (`fmt_odyssey::tlk::unbraced`), `<CUSTOMn>` and the player's tokens
/// (`player_token`) filled in, trimmed.
pub(crate) fn said(raw: &[u8], fill: Fill) -> Vec<u8> {
    let raw = fmt_odyssey::tlk::unbraced(raw);
    let mut out = Vec::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        let rest = &raw[at..];
        match rest[0] {
            b'<' if rest.contains(&b'>') => {
                let end = rest.iter().position(|&b| b == b'>').expect("checked");
                let token = std::str::from_utf8(&rest[1..end]).unwrap_or("");
                let value = match token.strip_prefix("CUSTOM") {
                    Some(digits) => digits
                        .parse::<i32>()
                        .ok()
                        .and_then(|n| fill.told.tokens.get(&n).cloned()),
                    None => fill.pc.and_then(|pc| player_token(token, pc)),
                };
                match value {
                    Some(value) => out.extend(value.chars().map(|c| c as u32 as u8)),
                    None => out.extend_from_slice(&rest[..=end]),
                }
                at += end + 1;
                continue;
            }
            byte => out.push(byte),
        }
        at += 1;
    }
    let trimmed = out.trim_ascii();
    trimmed.to_vec()
}

/// Whether `talk`'s replies are offered now: its line has played and it
/// waits on the player (a reply that says nothing is taken by itself).
fn answering(talk: &Conversation) -> bool {
    talk.until_ms == u64::MAX && !talk.auto && !talk.replies.is_empty()
}

/// The words of `talk`'s line and of the replies it offers, numbered, as
/// the game's bytes.
fn words(
    screen: &Screen,
    talk: &Conversation,
    swap: &SceneSwap,
    fill: Fill,
) -> (Vec<u8>, Vec<Vec<u8>>) {
    let Some(dlg) = swap
        .level
        .scripts()
        .and_then(|library| library.dialogue(&talk.dialog))
    else {
        return (Vec::new(), Vec::new());
    };
    let tlk = screen.tlk.as_deref();
    let tlk = |strref| tlk?.text(strref).map(<[u8]>::to_vec);
    let said = dlg
        .entries
        .get(talk.entry as usize)
        .map_or_else(Vec::new, |node| node_text(node, &tlk, fill));
    let replies = talk
        .replies
        .iter()
        .enumerate()
        .filter_map(|(n, reply)| {
            let node = dlg.replies.get(*reply as usize)?;
            Some(ui::strings::reply(n, &node_text(node, &tlk, fill)))
        })
        .collect();
    (said, replies)
}

/// By place, the replies of `talk` that `seat`'s player only points at
/// (I30 P6): none when it takes the story's replies itself
/// (`Snapshot::takes_story`: the conversation's player, or the host); else
/// the story's (`dialogue::story`), and every one for a seat with no body
/// (watching).
fn host_only(
    talk: &Conversation,
    snapshot: &Snapshot,
    seat: sim::Seat,
    swap: &SceneSwap,
) -> Vec<bool> {
    if snapshot.takes_story(seat, talk) {
        return Vec::new();
    }
    let body = snapshot.body(seat);
    let library = swap.level.scripts();
    let dlg = library.and_then(|library| Some((library, library.dialogue(&talk.dialog)?)));
    let story = |reply: u32| {
        dlg.is_some_and(|(library, dlg)| sim::script::dialogue::story(library, dlg, reply))
    };
    talk.replies
        .iter()
        .map(|reply| body.is_none() || story(*reply))
        .collect()
}

/// The body a watching seat's camera follows (I30 P3, P11; `play_view`),
/// none while the seat plays a standing body.
#[derive(Resource, Default)]
pub(crate) struct Watching(pub Option<SimId>);

/// The conversation `seat`'s screens show: the one its body stands in,
/// else a party one (`Snapshot::talk_for`); while it watches (`followed`,
/// the body its camera follows: `Watching`), the one holding that body,
/// else a party one.
pub(crate) fn shown(
    snapshot: &Snapshot,
    seat: sim::Seat,
    followed: Option<SimId>,
) -> Option<&Conversation> {
    match followed {
        Some(followed) => snapshot
            .talk
            .holding(ObjectId(followed.0))
            .or_else(|| snapshot.talk.party()),
        None => snapshot.talk_for(seat),
    }
}

/// What `id` is called on the screens: its template's name, a player's
/// persona's, else nothing.
pub(crate) fn name_of(snapshot: &Snapshot, objects: &sim::Instances, id: ObjectId) -> Vec<u8> {
    let Some(entity) = snapshot.get(SimId(id.0)) else {
        return Vec::new();
    };
    let named = objects
        .of_entity(entity)
        .map(|facts| facts.name.clone())
        .filter(|name| !name.is_empty())
        .or_else(|| match entity.kind {
            Kind::Avatar(seat) => {
                sim::persona::of(&snapshot.personas, seat).map(|persona| persona.name.clone())
            }
            _ => None,
        });
    named.map_or_else(Vec::new, |name| ui::strings::bytes(&name))
}

/// The conversations asking the host in (R5), in order, as the host's card
/// shows them: the asking guest's name and the NPC's, and the keys that
/// answer the first; none when none asks or this player is not the host.
fn requests(
    snapshot: &Snapshot,
    seat: sim::Seat,
    swap: &SceneSwap,
    map: &KeyMap,
) -> Option<RequestsView> {
    let asking = asking(snapshot, seat);
    if asking.is_empty() {
        return None;
    }
    let objects = sim::Instances::in_snapshot(&swap.level, snapshot);
    let name = |id| name_of(snapshot, &objects, id);
    Some(RequestsView {
        asks: asking
            .iter()
            .map(|talk| (name(talk.asking), name(talk.owner)))
            .collect(),
        accept: map.shown(KeyAction::AcceptCall),
        end: map.shown(KeyAction::EndCall),
        waiting: snapshot.request_wait().map(|wait| match wait {
            sim::RequestWait::ReadyCheck => ui::strings::ANSWER_AFTER_READY,
            sim::RequestWait::Down => ui::strings::ANSWER_WHEN_UP,
            sim::RequestWait::InCombat => ui::strings::ANSWER_AFTER_COMBAT,
            sim::RequestWait::Talking => ui::strings::ANSWER_AFTER_TALK,
        }),
    })
}

/// The conversations asking the host in, in order, when `seat` is the
/// host's.
fn asking(snapshot: &Snapshot, seat: sim::Seat) -> Vec<&Conversation> {
    if seat != sim::Seat(0) {
        return Vec::new();
    }
    snapshot
        .talk
        .conversations
        .iter()
        .filter(|talk| talk.asking != ObjectId::INVALID)
        .collect()
}

/// The console's `ui accept` and `ui end` this frame (`ui_view`): the
/// request card's keys.
#[derive(Resource, Default)]
pub(crate) struct Called(pub Option<bool>);

/// The host's keys on the request card (R5), or the console's: accept or
/// end the first asking, as `Action::Request`, while no screen is open and
/// nothing keeps the answer waiting (`Snapshot::request_wait`).
fn requests_keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    map: Option<Res<KeyMap>>,
    presented: Res<PresentedSnapshot>,
    (own, open): (Option<Res<OwnSeat>>, Option<Res<UiOpen>>),
    mut called: ResMut<Called>,
    outgoing: Option<ResMut<crate::loot::Outgoing>>,
) {
    let called = called.0.take();
    let (Some(own), Some(mut outgoing)) = (own, outgoing) else {
        return;
    };
    let snapshot = presented.0.current();
    // A ready check's key and the follow key are the request's by default,
    // and the sim drops an answer while the party fights or the host's own
    // conversation is locked: the keys wait meanwhile, as the card says.
    if snapshot.request_wait().is_some() {
        return;
    }
    let Some(first) = asking(snapshot, own.0).first().map(|talk| talk.id) else {
        return;
    };
    let map = map.as_deref().unwrap_or(KeyMap::defaults());
    let keys = keys.filter(|_| open.is_none_or(|open| !open.0));
    let pressed = |action| {
        keys.as_ref()
            .is_some_and(|keys| map.pressed_now(keys, action))
    };
    for (key, accept) in [(KeyAction::AcceptCall, true), (KeyAction::EndCall, false)] {
        if pressed(key) || called == Some(accept) {
            outgoing.0.push(Action::Request {
                talk: first,
                accept,
            });
        }
    }
}

/// Whether `talk` is a computer's (its DLG's `ConversationType` 1): the
/// computer screen shows it (`terminal`), this screen only its voice.
pub(crate) fn computer(swap: &SceneSwap, talk: &Conversation) -> bool {
    swap.level
        .scripts()
        .and_then(|library| library.dialogue(&talk.dialog))
        .is_some_and(|dlg| dlg.kind == fmt_odyssey::dlg::Kind::Computer)
}

#[allow(clippy::too_many_arguments)]
fn show(
    screen: Option<ResMut<Screen>>,
    presented: Res<PresentedSnapshot>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    ui: Option<ResMut<Ui>>,
    mut facts: ResMut<console::DumpFacts>,
    (mut cues, mut added, mut forget, mut hush): (
        ResMut<Cues>,
        ResMut<Added>,
        ResMut<Forget>,
        ResMut<Hush>,
    ),
    (input, watching, map): (Res<ReplyInput>, Res<Watching>, Option<Res<KeyMap>>),
    mut logged: Local<Option<(u32, u32)>>,
) {
    let (Some(mut screen), Some(own), Some(swap)) = (screen, own, swap) else {
        return;
    };
    let map = map.as_deref().unwrap_or(KeyMap::defaults());
    let snapshot = presented.0.current();
    let talk = shown(snapshot, own.0, watching.0);
    let leaves = talk.and_then(|talk| snapshot.may_leave(own.0, talk));
    let view = talk.filter(|talk| !computer(&swap, talk)).map(|talk| {
        let fill = Fill::to(snapshot, talk.pc);
        let (said, offered) = words(&screen, talk, &swap, fill);
        // Any second seat, not `together` as the Leave row: a watching seat
        // sees the players' marks too.
        let shared = snapshot.seats() > 1;
        let marks = shared.then(|| marks(&mut screen, talk, snapshot, &swap, &mut facts));
        DialogView {
            said,
            replies: if answering(talk) { offered } else { Vec::new() },
            focus: input.focus,
            marks,
            host_only: input.host_only(talk).to_vec(),
            leave: leaves,
            leave_key: map.shown(KeyAction::LeaveTalk),
        }
    });
    let requests = requests(snapshot, own.0, &swap, map);
    let text = |bytes: &[u8]| bytes.iter().map(|&b| char::from(b)).collect::<String>();
    match &requests {
        Some(card) => {
            let asks: Vec<[String; 2]> = card
                .asks
                .iter()
                .map(|(name, npc)| [text(name), text(npc)])
                .collect();
            facts.0.insert("requests".into(), serde_json::json!(asks));
        }
        None => {
            facts.0.remove("requests");
        }
    }
    if let Some(mut ui) = ui {
        // Each line said goes to the messages page's dialog log, once.
        let line = talk
            .zip(view.as_ref())
            .map(|(talk, view)| (talk, &view.said));
        match line {
            Some((talk, said)) if *logged != Some((talk.id, talk.entry)) => {
                *logged = Some((talk.id, talk.entry));
                if !said.is_empty() {
                    let objects = sim::Instances::in_snapshot(&swap.level, snapshot);
                    let who = name_of(snapshot, &objects, talk.speaker);
                    let (who, said) = (fmt_odyssey::code_page(&who), fmt_odyssey::code_page(said));
                    let line = if who.is_empty() {
                        said
                    } else {
                        format!("{who}: {said}")
                    };
                    crate::message_log::push(&mut ui.vm.dialog_log, line);
                }
            }
            None => *logged = None,
            Some(_) => {}
        }
        ui.vm.dialog = view;
        ui.vm.requests = requests;
    }
    let Some(talk) = talk else {
        if screen.showing.take().is_some() {
            facts.0.remove("dialogue");
        }
        if screen.marks.take().is_some() {
            facts.0.remove("dialogue_marks");
        }
        if let Some(speaker) = screen.speaking.take() {
            hush.0.push((speaker, Channel::Voice));
        }
        screen.lip = None;
        return;
    };
    // A line played or skipped: its voice and lips stop.
    if talk.until_ms == u64::MAX
        && let Some(was) = screen.speaking.take()
    {
        hush.0.push((was, Channel::Voice));
        screen.lip = None;
    }
    let key = (talk.id, talk.entry, talk.shown_ms, talk.auto);
    if screen.showing == Some(key) {
        if let Some(fact) = facts.0.get_mut("dialogue") {
            fact["answering"] = serde_json::json!(answering(talk));
            fact["host_only"] = serde_json::json!(input.host_only(talk));
            fact["leave"] = serde_json::json!(leaves);
        }
        return;
    }
    screen.showing = Some(key);
    // The line's voice and lips.
    let speaker = SimId(talk.speaker.0);
    if let Some(was) = screen.speaking.take() {
        hush.0.push((was, Channel::Voice));
    }
    screen.lip = None;
    let voice = swap.level.scripts().and_then(|library| {
        let dlg = library.dialogue(&talk.dialog)?;
        let node = dlg.entries.get(talk.entry as usize)?;
        let (name, _) = library.entry_voice(dlg, node, talk.voice_variant(talk.entry))?;
        let other = [node.sound.as_str(), node.vo.as_str()]
            .into_iter()
            .find(|n| *n != name && !n.is_empty());
        Some((name.into_owned(), other.map(str::to_string)))
    });
    if let (Some((name, other)), Some(resolver)) = (voice, module_resolver(&mut screen, &swap)) {
        if let Some(cue) = screen.voice_cue(&swap, &name, &mut added, &mut forget) {
            // Unplaced, as reone and xoreos play a conversation's voice (a
            // speaker far from the shot, Taris's duel announcer, is heard);
            // in its speaker's voice slot, so a line skipped stops it.
            cues.0.push(Cue {
                entity: Some(speaker),
                channel: Channel::Voice,
                follows: true,
                falloff: VOICE_FALLOFF,
                ..Cue::at(cue, speaker_at(snapshot, speaker), talk.shown_ms as f32)
            });
            screen.speaking = Some(speaker);
        }
        let lip_type = ResType::LIP;
        screen.lip = std::iter::once(name)
            .chain(other)
            .find_map(|name| {
                let bytes = resolver.find(&name, lip_type)?.location.read().ok()?;
                Lip::parse(&bytes).ok()
            })
            .map(|lip| (lip, talk.shown_ms));
    }
    let (said, replies) = words(&screen, talk, &swap, Fill::to(snapshot, talk.pc));
    facts.0.insert(
        "dialogue".into(),
        serde_json::json!({
            "dialog": talk.dialog, "entry": talk.entry,
            "said": text(&said),
            "replies": replies.iter().map(|reply| text(reply)).collect::<Vec<_>>(),
            "answering": answering(talk),
            "host_only": input.host_only(talk),
            "leave": leaves,
            "voiced": screen.speaking.is_some(),
            "lip_keys": screen.lip.as_ref().map(|(lip, _)| lip.keys.len()),
        }),
    );
}

/// What the seats' marks were drawn for: the conversation, the line
/// shown, the marks.
type Marked = (u32, u64, BTreeMap<u8, u8>);
/// The names pointing at each reply pointed at, by its place.
type Names = Vec<(u8, Vec<u8>)>;

/// The seats' marks (I30 P6): for each reply pointed at, the names of
/// those pointing at it, the character a seat drives, else "Player N";
/// worked out and reported again when the marks or the line change. The
/// portrait waits: a creature's facts carry none yet.
fn marks(
    screen: &mut Screen,
    talk: &Conversation,
    snapshot: &Snapshot,
    swap: &SceneSwap,
    facts: &mut console::DumpFacts,
) -> Names {
    let now = (talk.id, talk.shown_ms, talk.marks.clone());
    if let Some((drawn, names)) = &screen.marks
        && *drawn == now
    {
        return names.clone();
    }
    let objects = sim::Instances::in_snapshot(&swap.level, snapshot);
    let name = |seat: u8| {
        snapshot
            .body(sim::Seat(seat))
            .and_then(|body| objects.of_entity(body))
            .map(|facts| facts.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("Player {}", u16::from(seat) + 1))
    };
    let listed: Vec<serde_json::Value> = talk
        .marks
        .iter()
        .map(|(&seat, &nth)| {
            serde_json::json!({"seat": seat, "reply": u16::from(nth) + 1, "by": name(seat)})
        })
        .collect();
    facts
        .0
        .insert("dialogue_marks".into(), serde_json::json!(listed));
    let mut by_reply: BTreeMap<u8, Vec<String>> = BTreeMap::new();
    for (&seat, &nth) in &talk.marks {
        by_reply.entry(nth).or_default().push(name(seat));
    }
    let names: Names = by_reply
        .into_iter()
        .map(|(nth, names)| (nth, format!("< {}", names.join(", ")).into_bytes()))
        .collect();
    screen.marks = Some((now, names.clone()));
    names
}

/// The player's input to the screen (see the module's docs): the pointer
/// freed while it talks and held again after, the highlight, an answer.
#[allow(clippy::too_many_arguments)]
fn answer(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    mut windows: Query<
        (&Window, &mut bevy::window::CursorOptions),
        With<bevy::window::PrimaryWindow>,
    >,
    presented: Res<PresentedSnapshot>,
    (own, swap): (Option<Res<OwnSeat>>, Option<Res<SceneSwap>>),
    watching: Res<Watching>,
    map: Option<Res<KeyMap>>,
    (mut ui, open): (Option<ResMut<Ui>>, Option<Res<UiOpen>>),
    mut input: ResMut<ReplyInput>,
    showing: Option<Res<crate::movie_view::Showing>>,
    mut swallowed: ResMut<crate::swallow::Swallowed>,
    mut last: Local<Option<Vec2>>,
) {
    let Some(own) = own else {
        return;
    };
    let snapshot = presented.0.current();
    let talk = shown(snapshot, own.0, watching.0);
    let active = talk.is_some();
    // The line's replies this player only points at, once a line.
    let shared = snapshot.seats() > 1;
    if let (Some(talk), Some(swap), true) = (talk, swap.as_deref(), shared) {
        let line = (talk.id, talk.shown_ms);
        if input.host_only.as_ref().is_none_or(|(of, _)| *of != line) {
            input.host_only = Some((line, host_only(talk, snapshot, own.0, swap)));
        }
    }
    if active != input.active {
        input.active = active;
        input.focus = 0;
        for (_, mut cursor) in &mut windows {
            cursor.grab_mode = if active {
                bevy::window::CursorGrabMode::None
            } else {
                bevy::window::CursorGrabMode::Locked
            };
            cursor.visible = active;
        }
    }
    // A movie over the conversation, or a screen (the menu), takes the
    // keys and the pointer.
    let movie = showing.is_some_and(|showing| showing.0);
    let screen_open = open.is_some_and(|open| open.0);
    let (Some(talk), Some(keys), false, false) = (talk, keys, movie, screen_open) else {
        return;
    };
    if swap.as_deref().is_some_and(|swap| computer(swap, talk)) {
        return;
    }
    let pressed = |codes: &[KeyCode]| codes.iter().any(|code| keys.just_pressed(*code));
    let clicked = buttons
        .as_ref()
        .is_some_and(|b| b.just_pressed(MouseButton::Left));
    // What answers is not held into play when the conversation ends
    // (`swallow`).
    let take = |swallowed: &mut crate::swallow::Swallowed| {
        if let Some(buttons) = buttons.as_deref().filter(|_| clicked) {
            swallowed.button(MouseButton::Left, buttons);
        }
        for code in CONFIRM.iter().filter(|code| keys.just_pressed(**code)) {
            swallowed.key(*code, &keys);
        }
    };
    // The screen under the pointer, as it was last drawn.
    let pointer = windows
        .iter()
        .find_map(|(window, _)| Some((window.cursor_position()?, window.size())));
    let mut placed = pointer.and_then(|(at, size)| {
        let placed = ui.as_deref_mut()?.dialog_placed((size.x, size.y))?;
        Some((placed, (size.x, size.y), [at.x, at.y]))
    });
    // Leave (R6): its key at any time, or a click on its row, while the
    // conversation may be left.
    let leave_key = map
        .as_deref()
        .unwrap_or(KeyMap::defaults())
        .key(KeyAction::LeaveTalk);
    let on_leave = placed
        .as_ref()
        .is_some_and(|(placed, window, at)| placed.leave_at(*window, *at));
    if snapshot.may_leave(own.0, talk) == Some(true)
        && (keys.just_pressed(leave_key) || clicked && on_leave)
    {
        input.pick = Some(Pick::Leave);
        take(&mut swallowed);
        swallowed.key(leave_key, &keys);
        return;
    }
    if !answering(talk) {
        input.focus = 0;
        if clicked || pressed(&CONFIRM) {
            input.pick = Some(Pick::Skip);
            take(&mut swallowed);
        }
        return;
    }
    // Answered, the line takes nothing more from this player: a point sent
    // after it would hide the answer from an authority yet to hear it.
    let line = (talk.id, talk.shown_ms);
    if input.answered == Some(line) {
        return;
    }
    let offered = talk.replies.len() as u8;
    let step = i32::from(keys.just_pressed(KeyCode::ArrowDown))
        - i32::from(keys.just_pressed(KeyCode::ArrowUp));
    input.focus = (i32::from(input.focus) + step).rem_euclid(i32::from(offered)) as u8;
    // The reply under the pointer; it takes the highlight when the pointer
    // moves.
    let pointed = placed
        .take()
        .and_then(|(placed, window, at)| placed.row_at(window, at));
    let moved = pointer.map(|(at, _)| at) != *last;
    *last = pointer.map(|(at, _)| at);
    if let Some(nth) = pointed.filter(|nth| *nth < offered && moved) {
        input.focus = nth;
    }
    let digit = DIGITS
        .iter()
        .take(usize::from(offered))
        .position(|(top, pad)| keys.just_pressed(*top) || keys.just_pressed(*pad))
        .map(|nth| nth as u8);
    let nth = digit
        .or(pointed.filter(|_| clicked))
        .or_else(|| pressed(&CONFIRM).then_some(input.focus))
        .filter(|nth| *nth < offered);
    let only_points = |nth: u8| {
        let only = input.host_only(talk);
        only.get(usize::from(nth)).copied().unwrap_or(false)
    };
    if let Some(nth) = nth {
        take(&mut swallowed);
        if only_points(nth) {
            // A reply the host takes: this player suggests it; in its own
            // conversation, where no player character stands, it picks it,
            // which asks the host in (R5).
            input.pick = Some(if snapshot.asks_host(own.0, talk) {
                Pick::Reply(nth)
            } else {
                Pick::Point(nth)
            });
            input.pointed = Some((line, nth));
        } else {
            input.pick = Some(Pick::Reply(nth));
            input.answered = Some(line);
        }
    } else if shared
        && (step != 0 || pointed.is_some() && moved)
        && input.pointed != Some((line, input.focus))
    {
        // The highlight moved by hand: the others see where.
        input.pick = Some(Pick::Point(input.focus));
        input.pointed = Some((line, input.focus));
    }
}

impl Screen {
    /// The cue that plays voice `name` (a module's voiced line), its clip
    /// read into the sound bank when it is not there (`added`), the oldest
    /// past `VOICES_KEPT` taken out (`forget`; one playing plays on); none
    /// when the module has no such voice.
    pub(crate) fn voice_cue(
        &mut self,
        swap: &SceneSwap,
        name: &str,
        added: &mut Added,
        forget: &mut Forget,
    ) -> Option<String> {
        let cue = format!("vo/{name}");
        if !self.voiced.iter().any(|voiced| voiced == name) {
            let resolver = module_resolver(self, swap)?;
            added.0.push((cue.clone(), voice_clip(&resolver, name)?));
            self.voiced.push_back(name.to_string());
            while self.voiced.len() > VOICES_KEPT {
                let old = self.voiced.pop_front().expect("more than kept");
                forget.0.push(format!("vo/{old}"));
            }
        }
        Some(cue)
    }
}

/// The module's resolver for the scene swap shown, made once a swap.
fn module_resolver(screen: &mut Screen, swap: &SceneSwap) -> Option<Resolver> {
    if let Some((generation, resolver)) = &screen.module
        && *generation == swap.generation
    {
        return Some(resolver.clone());
    }
    let module = swap.level.module()?;
    let resolver = screen.install.as_ref()?.resolver(&module.name).ok()?;
    // What was read through another module's resources is not kept.
    screen.models.clear();
    screen.module = Some((swap.generation, resolver.clone()));
    Some(resolver)
}

/// Voiced lines kept read in the sound bank: a line is about a megabyte
/// decoded, and a game speaks thousands.
const VOICES_KEPT: usize = 16;

/// A voiced line's clip (`Clip::kotor`).
fn voice_clip(resolver: &Resolver, name: &str) -> Option<Clip> {
    Clip::kotor(&resolver.voice(name)?.read().ok()?).ok()
}

/// Where a character or object is (units, as cues take places).
pub(crate) fn speaker_at(snapshot: &Snapshot, id: SimId) -> Vec3 {
    snapshot
        .entities
        .iter()
        .find(|entity| entity.id == id)
        .map_or(Vec3::ZERO, |entity| {
            Vec3::from_array(entity.position) / collision::UNIT
        })
}

/// The speaker's head, lip-synced to its line (see the module's docs).
fn lips(
    screen: Option<ResMut<Screen>>,
    presented: Res<PresentedSnapshot>,
    (own, watching): (Option<Res<OwnSeat>>, Res<Watching>),
    (scene, swap): (Option<Res<SceneContent>>, Res<SceneSwap>),
    models: Query<(Entity, &SceneModel)>,
    mut transforms: Query<&mut Transform>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let (Some(mut screen), Some(own), Some(scene)) = (screen, own, scene) else {
        return;
    };
    // The speaker's mouth as last bound and posed, for dumps.
    if let (Some(fact), Some(face)) = (facts.0.get_mut("dialogue"), screen.face.as_ref()) {
        fact["mouth"] = serde_json::json!({
            "speaker": face.speaker.0, "bound": face.talk.is_some(), "posed": face.posed,
        });
    }
    let screen = &mut *screen;
    let snapshot = presented.0.current();
    let speaker = shown(snapshot, own.0, watching.0).map(|talk| SimId(talk.speaker.0));
    if screen.face.as_ref().map(|face| face.speaker) != speaker {
        if let Some(Face {
            talk: Some(talk), ..
        }) = screen.face.take()
        {
            talk.rest(&mut transforms);
        }
        screen.face = speaker.map(|speaker| Face {
            speaker,
            talk: None,
            models: 0,
            posed: false,
        });
    }
    if let Some(face) = screen.face.as_ref()
        && face.talk.is_none()
        && face.models != scene.0.models.len()
    {
        let (speaker, count) = (face.speaker, scene.0.models.len());
        let talk = head_talk(screen, (&scene.0, &swap), &models, snapshot, speaker);
        if let Some(face) = screen.face.as_mut() {
            face.talk = talk;
            face.models = count;
        }
    }
    let Some(face) = screen.face.as_mut() else {
        return;
    };
    let Some(talk) = &face.talk else {
        return;
    };
    let now = view_ms(&presented.0);
    let shapes = screen.lip.as_ref().and_then(|(lip, from)| {
        let at = ((now - *from as f64) / 1000.0) as f32;
        (at >= 0.0 && at <= lip.length).then(|| lip.shapes_at(at))
    });
    match shapes {
        Some((a, b, blend)) => {
            let pose = |shape: u8| f32::from(shape) / f32::from(lip::SHAPES) * talk.length();
            talk.pose_from(pose(b), (talk, pose(a)), blend, &mut transforms);
            face.posed = true;
        }
        None if face.posed => {
            talk.rest(&mut transforms);
            face.posed = false;
        }
        None => {}
    }
}

/// A creature's mouth (`mouth`: its hung head, else its body, whose own
/// face it is) with its `talk` clip bound, if it has one, read through the
/// module's resources.
fn head_talk(
    screen: &mut Screen,
    (scene, swap): (&crate::viewer::LoadedScene, &SceneSwap),
    models: &Query<(Entity, &SceneModel)>,
    snapshot: &Snapshot,
    speaker: SimId,
) -> Option<Animator> {
    let entity = snapshot.entities.iter().find(|e| e.id == speaker)?;
    let Kind::Object(sim::module::ObjectKind::Creature, object) = entity.kind else {
        return None;
    };
    let body = scene
        .creature_views
        .iter()
        .find(|view| view.object == object)?
        .body;
    let head = crate::face::hung_head(&scene.models, body).unwrap_or(body);
    let nodes = &models
        .iter()
        .find(|(_, model)| model.index == head)?
        .1
        .nodes;
    let placed = &scene.models[head];
    let resolver = module_resolver(screen, swap)?;
    let clip = mouth_clip(&mut screen.models, &resolver, screen.game, &placed.ir)?;
    crate::scene::animator(placed, &clip, nodes)
}

/// The clip a speaker's mouth plays its line's shapes on.
const MOUTH_CLIP: &str = "talk";

/// `mouth`'s `talk` clip (its model chain's), if it keys a node `mouth`
/// animates. The mouth is the speaker's hung head, else its body: a body
/// made with its face (`appearance.2da` `modeltype` F and no head, Calo
/// Nord's `n_calonord`) has the head's nodes itself, and plays the clip
/// its own chain has, as reone plays a speaker's `talk` on its body and
/// everything hung on it.
pub(crate) fn mouth_clip(
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    game: Game,
    mouth: &ModelIr,
) -> Option<Arc<AnimationIr>> {
    let clip = crate::viewer::clip(cache, resolver, game, mouth, MOUTH_CLIP).ok()??;
    clip.bind(mouth).iter().any(Option::is_some).then_some(clip)
}

/// Why a voiced line's speaker would not move its lips (`kjk dialogue
/// check`), its look's `head` (a model of its own) and `body` read, and
/// whether reone would move them: `Some(true)` when the view's mouth
/// (`mouth_clip`) binds no `talk` though the head or the body does (reone
/// plays `talk` on both); `Some(false)` when neither has one (a helmet,
/// a mask, a droid: nothing moves in reone either).
#[cfg(feature = "probes")]
pub fn lips_unmoved(
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    game: Game,
    (head, body): (Option<&ModelIr>, &ModelIr),
) -> Option<bool> {
    let mut binds = |model: &ModelIr| mouth_clip(cache, resolver, game, model).is_some();
    if binds(head.unwrap_or(body)) {
        return None;
    }
    Some(head.is_some_and(&mut binds) || binds(body))
}

/// How a line's voice falls off: unplaced, as reone and xoreos play a
/// conversation's voice (a speaker far from the shot, Taris's duel
/// announcer, is heard).
const VOICE_FALLOFF: audio::Falloff = audio::Falloff::Unplaced;

/// Why a line's voice `name` is not heard where it plays (`kjk dialogue
/// check`): its clip does not decode, or, cued as the view cues it at its
/// speaker `from` with the camera at `to` (game space, metres), it starts
/// out of earshot.
#[cfg(feature = "probes")]
pub fn voice_unheard(
    resolver: &Resolver,
    name: &str,
    (from, to): (Vec3, Vec3),
) -> Option<&'static str> {
    if voice_clip(resolver, name).is_none() {
        return Some("does not decode");
    }
    let listener = audio::Listener {
        origin: to / asset_ja::UNIT,
        right: Vec3::X,
        entity: None,
    };
    let emitter = audio::Emitter {
        at: from / asset_ja::UNIT,
        entity: Some(1),
        falloff: VOICE_FALLOFF,
    };
    let (left, right) = audio::spatial::gains(&listener, &emitter);
    (left == 0.0 && right == 0.0).then_some("unheard (out of earshot)")
}

/// The voice's probes (`kjk sim probes --only "conversation voice"`): a
/// line's voice is heard unplaced, wherever its speaker stands.
#[cfg(feature = "probes")]
pub fn voice_probes() -> sim::probe::Checks {
    use audio::{Emitter, Falloff, Listener};
    let listener = Listener {
        origin: Vec3::ZERO,
        right: Vec3::X,
        entity: None,
    };
    // Taris's duel announcer, 91 m to the side of the ring's cameras.
    let far = Vec3::X * 91.0 / asset_ja::UNIT;
    let heard = |falloff| {
        audio::spatial::gains(
            &listener,
            &Emitter {
                at: far,
                entity: Some(7),
                falloff,
            },
        )
    };
    let (left, right) = heard(VOICE_FALLOFF);
    let placed = heard(Falloff::Normal);
    vec![(
        "a conversation's voice heard unplaced 91 m off",
        sim::probe::check(
            left > 0.0 && left == right && placed == (0.0, 0.0),
            format!("unplaced ({left}, {right}); placed {placed:?}"),
        ),
    )]
}
