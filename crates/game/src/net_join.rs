//! `kjk join`: a player of another process's world (D3's client side).
//! The host is an address, or a Steam ID (`steam:<id>`, or the name of a
//! Steam friend hosting now: `peer`), reached through Steam's relays.
//! Before the app starts, `connect` does the handshake and waits
//! for the host to name its level, so the run loads that stage (a module's
//! plan and scene, or the course); from then a net thread (`net_thread`)
//! pumps the socket and the client's link, so the connection is answered
//! while the stage loads and acks never wait for a frame. In the app, each
//! frame: the client's events (an install: the module planned on a thread
//! with its loading screen, handed to the client, the scene swapped when
//! it loads), the player's command (keyboard and mouse, or a `--moves`
//! script by predicted tick), the client's frame (its clocks, prediction),
//! and what it shows: the host's snapshots, rebuilt, in
//! `PresentedSnapshot`, and its own predictions in `OwnView`. Every
//! snapshot it rebuilt goes into `rebuilt.txt` (epoch, tick, hash), to be
//! compared with the host's `hashes.txt`.
//!
//! A connection that times out joins again by itself with its seat's
//! token for `REJOIN` (the host keeps the seat a minute): the player is
//! told, the host names its level again (the one played: handed over at
//! once; another: planned as an install is) and sends a keyframe.

use std::io::Write as _;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use console::{DumpFacts, RunFolder, States};
use net::io::{Socket, Udp};
use net::netem::{Conditions, Netem};
use net::session::{Cause, Client, ClientConfig, ClientEvent, InstallRequest, LevelRequest};
use net::{NetTime, Peer, Transport};
use sim::{Level, Presented, Seat, Snapshot, TickRate};

use crate::net_thread::{NetThread, Shared, lock};
use crate::scene::SceneContent;
use crate::sim_host::{
    Controls, Driver, Job, Lifecycle, Loading, Look, OwnSeat, OwnView, PlayerInput,
    PresentedSnapshot, Read, SceneSwap, SelectedPower, player_command, power_choices,
};

/// The port a host listens on unless told otherwise (not JA's).
pub const PORT: u16 = ui::intent::DEFAULT_PORT;
/// How long joining waits for the host to accept and name its level.
const JOIN_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a connection that timed out tries to join again: well inside
/// the host's `RECONNECT_US`, which starts at about the same moment.
const REJOIN: Duration = Duration::from_secs(45);

/// A connection that has joined: the client, shared with the net thread
/// that pumps it.
pub struct Joining {
    client: Shared<Client>,
    net: Option<NetThread>,
    /// The host's level (the stage the run loaded), and the host's seat
    /// for this player.
    pub request: LevelRequest,
    pub seat: Seat,
    /// How many seats the host's world has.
    pub seats: u8,
    /// How much longer each module's plan takes here (`--plan-delay`).
    pub plan_delay: Duration,
    /// The socket's receive buffer, as the OS gave it.
    recv_buffer: Option<usize>,
}

impl Joining {
    fn now(&self) -> NetTime {
        crate::solo::now()
    }

    /// The client, locked from the net thread.
    fn client(&self) -> MutexGuard<'_, Client> {
        lock(&self.client)
    }
}

/// What a joined player plans of a module its host installs, or was in
/// when it joined: the same module for as many players (I30 P21), or the
/// digests differ and the host's world is refused.
pub fn planned(install: &InstallRequest) -> session::Request {
    session::Request::new(
        &install.game,
        &install.module,
        install.arrival(),
        install.seats,
    )
    .with_players(install.players)
}

/// Moves the datagrams both ways on this thread (the handshake's, before
/// the net thread starts).
fn pump(client: &mut Client, socket: &mut Netem<Socket>) {
    let now = crate::solo::now();
    socket.poll(now);
    while let Some(datagram) = socket.recv() {
        client.receive(datagram.at, datagram.from, &datagram.bytes);
    }
    while let Some((to, bytes)) = client.poll_transmit(now) {
        socket.send(to, &bytes);
    }
    socket.poll(now);
}

/// The address `host` (with `:port`, or the default port) names.
pub fn address(host: &str) -> Result<SocketAddr, String> {
    let with_port = if host
        .rsplit_once(':')
        .is_some_and(|(_, port)| port.parse::<u16>().is_ok())
        && !host.ends_with(']')
    {
        host.to_string()
    } else {
        format!("{host}:{PORT}")
    };
    with_port
        .to_socket_addrs()
        .map_err(|error| format!("{host}: {error}"))?
        .next()
        .ok_or(format!("{host}: no address"))
}

/// The host `host` names: `steam:<id>`, an address, or else a Steam friend
/// hosting now by name (any case). Steam is opened only for a name that is
/// no address.
pub fn peer(host: &str) -> Result<Peer, String> {
    if let Some(id) = net::parse_steam(host) {
        return Ok(Peer::Steam(id));
    }
    let by_address = address(host);
    #[cfg(feature = "steam")]
    if by_address.is_err()
        && let Some((_, id)) = net::io::steam::hosting_friends()
            .into_iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(host.trim()))
    {
        return Ok(Peer::Steam(id));
    }
    by_address.map(Peer::Addr)
}

/// The socket a player joins `host` from: UDP on any free port for an
/// address, Steam for a Steam ID.
fn socket_for(host: Peer, clock: net::io::Clock) -> Result<Socket, String> {
    match host {
        #[cfg(feature = "steam")]
        Peer::Steam(_) => net::io::Steam::open(clock).map(Socket::steam),
        #[cfg(not(feature = "steam"))]
        Peer::Steam(_) => Err("this build joins by address only (no Steam)".into()),
        Peer::Addr(address) => {
            let local: SocketAddr = if address.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            }
            .parse()
            .map_err(|_| "an unspecified address".to_string())?;
            Udp::bind(local, clock)
                .map(Socket::udp)
                .map_err(|error| format!("bind {local}: {error}"))
        }
        Peer::Local(_) => Err("not a host".into()),
    }
}

/// Joins the host at `host` with `credentials`, proving `key` if the host
/// asks for a password: the handshake, then the host's welcome and its
/// level. `conditions` shape what this side sends (`KJK_NETEM`).
pub fn connect(
    host: Peer,
    credentials: net::handshake::Credentials,
    key: Option<net::handshake::JoinKey>,
    conditions: Conditions,
) -> Result<Joining, String> {
    let started = Instant::now();
    let clock = crate::solo::clock();
    let socket = socket_for(host, clock.clone())?;
    let nonce = u64::from_le_bytes(crate::solo::random_key()[..8].try_into().unwrap_or([7; 8]));
    let mut client = Client::connect(
        credentials,
        host,
        nonce,
        crate::solo::now(),
        ClientConfig {
            link: net::link::Config::default(),
            audit: true,
            keep: true,
            rejoin_us: REJOIN.as_micros() as u64,
            key,
            nonce_key: crate::solo::random_key(),
            ..ClientConfig::default()
        },
    )
    .map_err(|error| format!("the join request: {error}"))?;
    let mut socket = Netem::new(socket, conditions);
    let (mut seat, mut seats) = (Seat(0), 1);
    while started.elapsed() < JOIN_TIMEOUT {
        pump(&mut client, &mut socket);
        while let Some(event) = client.poll_event() {
            match event {
                ClientEvent::Welcome(welcome) => {
                    seat = welcome.seat;
                    seats = welcome.seats;
                }
                ClientEvent::NeedLevel(request) => {
                    let recv_buffer = socket.inner().recv_buffer();
                    let client = Arc::new(Mutex::new(client));
                    let net = NetThread::spawn(socket, client.clone(), clock)
                        .map_err(|error| format!("the net thread: {error}"))?;
                    return Ok(Joining {
                        client,
                        net: Some(net),
                        request,
                        seat,
                        seats,
                        plan_delay: Duration::ZERO,
                        recv_buffer,
                    });
                }
                ClientEvent::Ended(why) => return Err(format!("{host}: {why}")),
                _ => {}
            }
        }
        socket.inner().wait(Duration::from_millis(5));
    }
    Err(format!("{host}: no answer in {} s", JOIN_TIMEOUT.as_secs()))
}

/// The joined player's state in the app.
#[derive(Resource)]
pub(crate) struct Joined {
    joining: Joining,
    /// The level the client plays in now (the stage's, then each install's).
    level: Level,
    driver: Driver,
    look: Look,
    controls: Controls,
    lifecycle: Option<Lifecycle>,
    job: Option<Job>,
    generation: u32,
    /// The level is still to be handed over (the stage finished loading).
    provide: bool,
    rebuilt: Option<std::io::BufWriter<std::fs::File>>,
    ended: Option<String>,
    frames: u64,
    /// The last frame's report, written to `net.json` at the end with the
    /// net feel's samples.
    facts: serde_json::Value,
    feel: crate::net_feel::Feel,
}

/// A joined player's session is under way (its systems run).
pub(crate) fn joined(joined: Option<Res<Joined>>) -> bool {
    joined.is_some()
}

/// Puts the joined player's resources into `app` (`sim_host` hands over a
/// `Net::Join` run here; its systems are `sim_host`'s plugin's).
pub fn install(
    app: &mut World,
    joining: Joining,
    level: Level,
    start: Snapshot,
    driver: Driver,
    lifecycle: Option<Lifecycle>,
) {
    let folder = app.resource::<RunFolder>().0.clone();
    let rebuilt = std::fs::File::create(folder.join("rebuilt.txt"))
        .ok()
        .map(std::io::BufWriter::new);
    // Until the host's snapshots come, the stage's own start is shown.
    let first = Presented::new(start);
    let mut states = States::default();
    states.0.insert("world".into(), true);
    states.0.insert("loading".into(), false);
    let seat = joining.seat;
    app.insert_resource(PresentedSnapshot(
        first.clone(),
        vec![first.current_shared().clone()],
    ));
    app.insert_resource(OwnView {
        presented: first,
        fresh: Vec::new(),
        correction: [0.0; 3],
    });
    app.insert_resource(OwnSeat(seat));
    app.init_resource::<SelectedPower>();
    app.insert_resource(SceneSwap {
        generation: 0,
        body: None,
        level: level.clone(),
        scene: false,
    });
    app.get_resource_or_insert_with(States::default)
        .0
        .extend(states.0);
    app.init_resource::<Loading>();
    app.insert_resource(Joined {
        joining,
        level,
        driver,
        look: Look {
            pitch: 0.0,
            yaw: 0.0,
        },
        controls: Controls::default(),
        lifecycle,
        job: None,
        generation: 0,
        provide: true,
        rebuilt,
        ended: None,
        frames: 0,
        facts: serde_json::Value::Null,
        feel: crate::net_feel::Feel::default(),
    });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn advance(
    input: PlayerInput,
    mut joined: ResMut<Joined>,
    mut presented: ResMut<PresentedSnapshot>,
    mut own: ResMut<OwnView>,
    mut facts: ResMut<DumpFacts>,
    (mut loading, mut states, mut swap, mut commands, mut selected, mut replies): (
        ResMut<Loading>,
        ResMut<States>,
        ResMut<SceneSwap>,
        Commands,
        ResMut<SelectedPower>,
        Option<ResMut<crate::dialogue_view::ReplyInput>>,
    ),
    (map, mut autopilot, camera_aim): (
        Option<Res<crate::settings::KeyMap>>,
        Option<ResMut<crate::target_view::Autopilot>>,
        Res<crate::sim_host::CameraAim>,
    ),
    (mut outgoing, mut told, mut shown, mut heard, mut copies): (
        Option<ResMut<crate::loot::Outgoing>>,
        Option<ResMut<crate::loot::Told>>,
        Option<ResMut<crate::script_screens::Shown>>,
        Option<ResMut<crate::ambience::Heard>>,
        Option<ResMut<crate::save_view::Received>>,
    ),
    (time, waited, frozen): (
        Res<Time<Real>>,
        Option<ResMut<crate::sim_host::Waited>>,
        Option<ResMut<crate::sim_host::Frozen>>,
    ),
) {
    let map = map.map_or_else(crate::settings::KeyMap::default, |map| map.clone());
    let joined = &mut *joined;
    joined.frames += 1;
    let now = joined.joining.now();
    let events: Vec<ClientEvent> = {
        let mut client = joined.joining.client();
        if let Some(outgoing) = outgoing.as_deref_mut() {
            for action in outgoing.0.drain(..) {
                client.act(action);
            }
        }
        if joined.provide {
            joined.provide = false;
            client.provide_level(now, Ok(joined.level.clone()));
        }
        std::iter::from_fn(|| client.poll_event()).collect()
    };
    for event in events {
        match event {
            ClientEvent::Installing(request) => {
                info!(
                    target: "net",
                    module = %request.module,
                    seats = request.seats,
                    players = request.players,
                    "the host installs"
                );
                if let Some(lifecycle) = &joined.lifecycle {
                    joined.job = Some(Job::spawn(
                        lifecycle.installer.clone(),
                        planned(&request),
                        lifecycle.body.is_some(),
                        joined.joining.plan_delay,
                        &mut loading,
                    ));
                } else {
                    joined
                        .joining
                        .client()
                        .provide_level(now, Err("this run plans no modules".into()));
                }
            }
            ClientEvent::Notice(text) => {
                if let Some(told) = told.as_deref_mut() {
                    told.0.push(text);
                }
            }
            ClientEvent::Screen(screen) => {
                if let Some(shown) = shown.as_deref_mut() {
                    shown.0.push(screen);
                }
            }
            ClientEvent::Sound(sound) => {
                if let Some(heard) = heard.as_deref_mut() {
                    heard.0.push(sound);
                }
            }
            // The host saved (I30 P17): its copy kept among this player's
            // saves.
            ClientEvent::SaveCopy(copy) => {
                let kept = crate::save_view::folder()
                    .ok_or_else(|| "no folder for saves here".to_string())
                    .and_then(|root| saves::copies::receive(&root, &copy, env!("KJK_GIT_SHA")));
                match &kept {
                    Ok(id) => info!(target: "saves", id, bytes = copy.len(), "copy kept"),
                    Err(why) => warn!(target: "saves", "copy refused: {why}"),
                }
                if let Some(copies) = copies.as_deref_mut() {
                    copies.0.push(kept);
                }
            }
            ClientEvent::Installed { epoch, agreed } => {
                info!(target: "net", epoch, agreed, "installed");
            }
            ClientEvent::Playing { tick } if joined.generation == 0 => {
                // The host's world, from here: the view starts over in it,
                // in the same scene.
                info!(target: "net", tick, "playing");
                joined.generation = 1;
                *swap = SceneSwap {
                    generation: 1,
                    body: None,
                    level: joined.level.clone(),
                    scene: false,
                };
            }
            ClientEvent::Rejoining { .. } => {
                warn!(target: "net", "the connection to the host timed out: joining again");
                if let Some(told) = told.as_deref_mut() {
                    told.0
                        .push("Connection to the host lost. Joining again...".into());
                }
            }
            // Joined again. The token keeps the seat, so another one means
            // the host no longer knew it (it restarted, or the seat's wait
            // ran out): this side's predictions, open screens and every
            // view follow the old seat's body, so the session ends rather
            // than drive someone else's.
            ClientEvent::Welcome(welcome) if welcome.seat != joined.joining.seat => {
                let why = format!(
                    "the host gave this player seat {} on joining again, not its seat {}: join again",
                    welcome.seat.0, joined.joining.seat.0
                );
                error!(target: "net", "{why}");
                if let Some(told) = told.as_deref_mut() {
                    told.0.push(format!("Session ended: {why}."));
                }
                joined
                    .joining
                    .client()
                    .leave(now, net::handshake::Refusal::Left);
                joined.ended = Some(why);
            }
            ClientEvent::Welcome(welcome) => {
                info!(target: "net", seat = welcome.seat.0, "joined again");
                if let Some(told) = told.as_deref_mut() {
                    told.0.push("Joined the host again.".into());
                }
            }
            // Joined again: the host names its level. The one played here
            // is handed over; another (it installed meanwhile) is planned.
            ClientEvent::NeedLevel(request) => {
                let same = match &request {
                    LevelRequest::Module(announcement) => announcement.check(&joined.level).is_ok(),
                    LevelRequest::Stage(id) => id.check(&joined.level).is_ok(),
                };
                let mut client = joined.joining.client();
                match (&request, &joined.lifecycle) {
                    _ if same => client.provide_level(now, Ok(joined.level.clone())),
                    (LevelRequest::Module(announcement), Some(lifecycle)) => {
                        joined.job = Some(Job::spawn(
                            lifecycle.installer.clone(),
                            planned(&InstallRequest::of(announcement)),
                            lifecycle.body.is_some(),
                            joined.joining.plan_delay,
                            &mut loading,
                        ));
                    }
                    _ => {
                        client.provide_level(now, Err("the host's level is not this run's".into()))
                    }
                }
            }
            ClientEvent::Ended(why) => {
                error!(target: "net", "the session ended: {why}");
                joined.ended.get_or_insert_with(|| why.to_string());
            }
            other => debug!(target: "net", "{other:?}"),
        }
    }
    // A module planned: the client installs it; the new scene comes in.
    if let Some(job) = &joined.job
        && let Some(result) = job.poll(&mut loading)
    {
        joined.job = None;
        match result {
            Ok(prepared) => {
                joined.level = prepared.plan.level.clone();
                joined
                    .joining
                    .client()
                    .provide_level(now, Ok(prepared.plan.level.clone()));
                joined.generation += 1;
                let mut body = None;
                let scene = prepared.scene.is_some();
                if let Some(scene) = prepared.scene {
                    let scene = match joined.lifecycle.as_ref().and_then(|l| l.body.clone()) {
                        Some(parts) => {
                            let (scene, index) = crate::play::compose(Some(scene), &parts, None);
                            body = Some(index);
                            scene
                        }
                        None => scene,
                    };
                    commands.insert_resource(SceneContent(Box::new(scene)));
                }
                *swap = SceneSwap {
                    generation: joined.generation,
                    body,
                    level: joined.level.clone(),
                    scene,
                };
                loading.progress = 1.0;
                loading.committed = Some(Instant::now());
            }
            Err(error) => {
                error!(target: "net", "cannot plan the host's module: {error}");
                joined.joining.client().provide_level(now, Err(error));
                loading.active = false;
            }
        }
    }
    let (running, next_tick) = {
        let client = joined.joining.client();
        (client.running(), client.next_tick())
    };
    let shown_loading = loading.active && (joined.job.is_some() || !running);
    states.0.insert("loading".into(), shown_loading);
    states.0.insert("world".into(), !shown_loading);

    // The player's command for the ticks this frame predicts: held, as the
    // host's, while a movie the host plays for everyone is up.
    let Read {
        keys,
        triggers,
        turn,
        feel,
    } = input.read();
    if let (Some(delta), Driver::Player(None)) = (turn, &joined.driver) {
        let held = crate::sim_host::held_pitch(own.presented.current(), joined.joining.seat);
        joined.look.turn(delta, feel, held);
    }
    // A scripted run's screens answer into the script's command, as the
    // host's do (`Controls::scripted`).
    let talk = own.presented.current().talk_for(joined.joining.seat);
    let picked = replies.as_deref_mut().and_then(|input| input.pick.take());
    let scripted = matches!(joined.driver, Driver::Player(Some(_)));
    if let Some(pick) = picked.filter(|_| scripted && talk.is_some()) {
        joined.controls.reply = sim::script::dialogue::answer(joined.controls.reply, pick);
    }
    if let Some(keys) = &keys {
        let choices = power_choices(own.presented.current(), joined.joining.seat, &swap.level);
        let picked = picked.filter(|_| !scripted);
        joined.controls.update(keys, &map, &choices, talk, picked);
        selected.0 = joined.controls.power;
    }
    let rate = TickRate::default();
    let command = match (&joined.driver, &keys, next_tick) {
        (Driver::Player(Some(script)), _, Some(tick)) => {
            let mut command = script.command(rate.time_ms(tick.saturating_sub(1)));
            command.reply = joined.controls.scripted(command.reply);
            command
        }
        (_, Some(keys), _) => player_command(keys, &map, triggers, joined.look, joined.controls),
        _ => sim::Command::default(),
    };
    let command = crate::target_view::steer(autopilot.as_deref_mut(), command, &mut joined.look);
    // Its shots go where the middle of its screen looks.
    let command = sim::Command {
        aim: camera_aim.0,
        ..command
    };
    // The frame predicts with the client locked: the net thread waits.
    let mut client = lock(&joined.joining.client);
    let span = info_span!("net.frame").entered();
    let frame = client.frame(now, command);
    drop(span);
    if let Some(frame) = frame {
        let same = |a: &Presented, b: &Presented| {
            a.previous().tick == b.previous().tick && a.current().tick == b.current().tick
        };
        if !same(&presented.0, frame.remote) {
            presented.0 = frame.remote.clone();
        }
        presented.0.set_alpha(frame.remote.alpha());
        if !same(&own.presented, frame.predicted) {
            own.presented = frame.predicted.clone();
        }
        own.presented.set_alpha(frame.predicted.alpha());
        own.correction = frame.correction;
    }
    // A break holds the host's world: the sounds wait with it, the beds go
    // on (`Waited`).
    let on_break = presented.0.current().paused_by.is_some();
    if let Some(mut waited) = waited {
        waited.frame(on_break, None, time.delta_secs_f64() * 1000.0);
    }
    if let Some(mut frozen) = frozen {
        frozen.0 = on_break;
    }
    let kept = client.take_snapshots();
    presented.1 = kept.rebuilt.into_iter().map(Arc::new).collect();
    own.fresh = kept.predicted;
    if let Some(file) = &mut joined.rebuilt {
        for (epoch, tick, hash) in client.take_audit() {
            let _ = writeln!(file, "{epoch} {tick} {hash}");
        }
    }

    if joined.feel.frame() {
        crate::net_feel::sample_player(&mut joined.feel, &client);
    }
    let stats = client.stats();
    let link = client.link_stats();
    let clock = client.clock();
    let seat = client.seat();
    drop(client);
    joined.facts = serde_json::json!({
            "path": "udp",
            "recv_buffer": joined.joining.recv_buffer,
            "seat": seat.map(|seat| seat.0),
            "ended": joined.ended,
            "running": running,
            "frames": joined.frames,
            "rebuilt": stats.rebuilt,
            "keyframes": stats.keyframes,
            "check_failures": stats.check_failures,
            "no_baseline": stats.no_baseline,
            "other_epoch": stats.other_epoch,
            "commands": stats.commands,
            "host_late": stats.host_late,
            "host_substituted": stats.host_substituted,
            "compared": stats.compared,
            "mismatches": stats.mismatches,
            "by_cause": Cause::ALL.iter()
                .map(|cause| (cause.name().to_string(), serde_json::json!(stats.by_cause[*cause as usize])))
                .collect::<serde_json::Map<_, _>>(),
            "installs_agreed": stats.installs_agreed,
            "installs_differed": stats.installs_differed,
            "rewinds": stats.rewinds,
            "rewind_mean": stats.rewind_ticks as f64 / stats.rewinds.max(1) as f64,
            "corrections": stats.corrections,
            "starvations": stats.starvations,
            "lead_ticks": clock.map(|c| c.lead),
            "view_lag_ticks": clock.map(|c| c.delay),
            "rtt_ms": link.map(|l| l.rtt_us as f64 / 1000.0),
            "loss_percent": link.map(|l| l.loss_ppm as f64 / 10_000.0),
            "bytes_up": link.map(|l| l.bytes_sent),
            "bytes_down": link.map(|l| l.bytes_received),
    });
    facts.0.insert("net".into(), joined.facts.clone());
}

/// The run ends: the host is told this player left (straight through the
/// socket: the network's conditions would hold the packets past the
/// process's end), the rebuilt hashes are flushed, and the last report
/// goes to `net.json`.
pub(crate) fn flush(
    mut exits: MessageReader<AppExit>,
    mut joined: ResMut<Joined>,
    folder: Res<RunFolder>,
) {
    if exits.read().next().is_none() {
        return;
    }
    let joined = &mut *joined;
    let mut socket = joined.joining.net.take().and_then(NetThread::stop);
    let now = joined.joining.now();
    let mut client = joined.joining.client();
    client.leave(now, net::handshake::Refusal::Left);
    while let Some((to, bytes)) = client.poll_transmit(now) {
        if let Some(socket) = &mut socket {
            socket.inner_mut().send(to, &bytes);
        }
    }
    drop(client);
    if let Some(file) = &mut joined.rebuilt {
        let _ = file.flush();
    }
    let mut report = joined.facts.clone();
    report["feel"] = joined.feel.report();
    crate::solo::write_json(&folder.0.join("net.json"), &report);
}
