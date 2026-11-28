//! Zero-GPU headless host and multi-bot UDP runner for 64-player profiling and benchmarks.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::process::ExitCode;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kjk_game::stage::{self, Stage};
use kjk_game::{settings::Settings, solo, world_data};
use net::handshake::{GameFingerprint, JoinKey, Refusal};
use net::io::{SocketConfig, Udp};
use net::netem::{Conditions, Netem};
use net::session::client::{Client, ClientConfig, ClientEvent, REBASE_EVERY_US, RebaseOn};
use net::session::clock::Margins;
use net::session::msg::{InstallRequest, LevelRequest};
use net::session::server::{EncodeJob, EncodedJob, Server, ServerConfig};
use net::{NetTime, Peer, Transport};
use sim::combat::Sheet;
use sim::{Command, Guests, Level, LevelId, StepReason, TickInput, TickRate, buttons};
use vfs::{Discovery, Game};

/// Options for [`run_host`].
pub(crate) struct HostRun<'a> {
    pub(crate) stage: &'a str,
    pub(crate) arrive: Option<&'a str>,
    pub(crate) seats: u8,
    pub(crate) guests: Guests,
    pub(crate) address: SocketAddr,
    pub(crate) sheet: Sheet,
    pub(crate) conditions: Conditions,
    pub(crate) seconds: u64,
    pub(crate) quorum: Option<u8>,
    pub(crate) aim: Option<&'a str>,
    pub(crate) password: Option<&'a str>,
}

/// Options for [`run_bots`].
pub(crate) struct BotsRun<'a> {
    pub(crate) host: &'a str,
    pub(crate) name: &'a str,
    pub(crate) password: Option<&'a str>,
    pub(crate) conditions: Conditions,
    pub(crate) bots: u8,
    pub(crate) seconds: u64,
}

fn host_world_data(
    discovery: &Discovery,
    stage_name: &str,
) -> Result<Vec<GameFingerprint>, String> {
    let mut games = vec![Game::JediAcademy];
    if !matches!(stage_name, "course" | "duel" | "ring" | "arena") {
        games.push(Game::Kotor1);
        games.push(Game::Kotor2);
    }
    world_data(discovery, &games, &|_| {})
}

fn percentile_ms(samples_ns: &mut [u64], pct: usize) -> f64 {
    if samples_ns.is_empty() {
        return 0.0;
    }
    samples_ns.sort_unstable();
    let idx = (samples_ns.len() - 1) * pct / 100;
    samples_ns[idx] as f64 / 1_000_000.0
}

/// Runs a headless authoritative server at 125 Hz with zero GPU/window/audio init.
pub(crate) fn run_host(spec: &HostRun<'_>) -> ExitCode {
    let discovery = crate::discover();
    let (anims, _) = stage::anims(&discovery);
    let anims = Arc::new(anims);
    let stage = match Stage::named(
        &discovery,
        spec.stage,
        spec.arrive,
        anims,
        false,
        spec.seats,
        1,
    ) {
        Ok(stage) => stage.with_sheet(spec.sheet).with_guests(spec.guests),
        Err(error) => {
            eprintln!("stage {}: {error}", spec.stage);
            return ExitCode::from(2);
        }
    };
    let (mut world, announcement) = match stage.world(1) {
        Ok(built) => built,
        Err(error) => {
            eprintln!("world {}: {error}", spec.stage);
            return ExitCode::from(2);
        }
    };
    let data = match host_world_data(&discovery, spec.stage) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("world data: {error}");
            return ExitCode::from(2);
        }
    };
    let mut credentials = solo::credentials("host", data);
    let saber_duel = Settings::load().saber_rules().duel();
    let stances = world
        .level()
        .anims()
        .stances()
        .filter(|s| saber_duel && !s.is_empty());
    if let Some(sets) = stances {
        credentials.games.push(GameFingerprint {
            game: net::wire::STANCE_GAME.to_string(),
            digest: sets.digest(),
            groups: Vec::new(),
        });
    }
    let request = match announcement {
        Some(ann) => LevelRequest::Module(ann),
        None => LevelRequest::Stage(world.level().id()),
    };
    let udp = match Udp::bind_with(spec.address, solo::clock(), SocketConfig::host(spec.seats)) {
        Ok(udp) => udp,
        Err(error) => {
            eprintln!("listen on {}: {error}", spec.address);
            return ExitCode::from(1);
        }
    };
    let bound = udp.address().unwrap_or(spec.address);
    let mut transport = Netem::new(udp, spec.conditions);
    let rate = TickRate::default();
    let quorum = spec
        .quorum
        .unwrap_or(if spec.seats > 4 { 80 } else { 100 })
        .clamp(1, 100);
    let mut server = Server::new(
        ServerConfig {
            seats: spec.seats,
            rate,
            every: if spec.seats > 4 { 4 } else { 2 },
            link: net::link::Config::default(),
            load_wait_us: 30_000_000,
            secret: solo::random_key(),
            join_key: spec.password.map(JoinKey::new),
            credentials,
            stances: stances.map_or_else(Vec::new, |sets| sets.ids()),
            collision: ::session::collision().code(),
            quorum_percent: quorum,
        },
        request,
    );
    let init_snap = world.snapshot();
    server.stepped(NetTime(0), &TickInput::default(), &init_snap);
    let _ = spec.aim;
    outln!(
        "[headless-host] listening={} stage={} seats={} quorum={}% duration={}s",
        bound,
        spec.stage,
        spec.seats,
        quorum,
        spec.seconds
    );

    let tick_us = 1_000_000 / u64::from(rate.hz());
    let end_us = spec.seconds.saturating_mul(1_000_000);
    let start = Instant::now();
    let mut next_tick_us = tick_us;
    let mut next_report_us = 2_000_000u64;
    let mut sim_tick = init_snap.tick;
    let parallel_encode = spec.seats > 4;
    let mut step_ns_samples = Vec::with_capacity(spec.seconds as usize * 125);
    let mut net_ns_samples = Vec::with_capacity(spec.seconds as usize * 125);
    let mut peak_stats = (0.001f64, server.stats());

    std::thread::scope(|scope| {
        let encode_pool = if parallel_encode {
            let (tx_job, rx_job) = mpsc::channel::<EncodeJob>();
            let rx_job = Arc::new(Mutex::new(rx_job));
            let (tx_done, rx_done) = mpsc::channel::<EncodedJob>();
            for _ in 0..4 {
                let rx_job = Arc::clone(&rx_job);
                let tx_done = tx_done.clone();
                scope.spawn(move || {
                    loop {
                        let job = {
                            let Ok(guard) = rx_job.lock() else {
                                break;
                            };
                            guard.recv()
                        };
                        let Ok(job) = job else {
                            break;
                        };
                        if tx_done.send(job.run()).is_err() {
                            break;
                        }
                    }
                });
            }
            Some((tx_job, rx_done))
        } else {
            None
        };

        loop {
            let now_us = start.elapsed().as_micros() as u64;
            if now_us >= end_us {
                break;
            }
            let now = NetTime(now_us);
            transport.poll(now);
            while let Some(datagram) = Transport::recv(&mut transport) {
                server.receive(now, datagram.from, &datagram.bytes);
            }
            while server.poll_event().is_some() {}
            if let Some((_, rx_done)) = &encode_pool {
                while let Ok(done) = rx_done.try_recv() {
                    server.finish_encode(now, done);
                }
            }

            while now_us >= next_tick_us {
                let tick_now = NetTime(next_tick_us);
                next_tick_us += tick_us;
                if server.can_step(tick_now) {
                    sim_tick += 1;
                    let t_step = Instant::now();
                    let input = server.commands_for(sim_tick);
                    let snapshot = sim::step(
                        &mut world,
                        sim_tick,
                        &input,
                        rate.msec(sim_tick),
                        StepReason::Authority,
                    );
                    step_ns_samples.push(t_step.elapsed().as_nanos() as u64);

                    let t_net = Instant::now();
                    server.stepped(tick_now, &input, &snapshot);
                    if let Some((tx_job, rx_done)) = &encode_pool {
                        while let Some(job) = server.poll_encode_job(tick_now) {
                            let _ = tx_job.send(job);
                        }
                        while let Ok(done) = rx_done.try_recv() {
                            server.finish_encode(tick_now, done);
                        }
                    }
                    while let Some((to, bytes)) = server.poll_transmit(tick_now) {
                        Transport::send(&mut transport, to, &bytes);
                    }
                    transport.poll(tick_now);
                    net_ns_samples.push(t_net.elapsed().as_nanos() as u64);
                } else {
                    while let Some((to, bytes)) = server.poll_transmit(tick_now) {
                        Transport::send(&mut transport, to, &bytes);
                    }
                    transport.poll(tick_now);
                }
            }
            if let Some((_, rx_done)) = &encode_pool {
                while let Ok(done) = rx_done.try_recv() {
                    server.finish_encode(now, done);
                }
            }
            while let Some((to, bytes)) = server.poll_transmit(now) {
                Transport::send(&mut transport, to, &bytes);
            }
            transport.poll(now);

            if now_us >= next_report_us {
                let stats = server.stats();
                let elapsed_s = (now_us as f64 / 1_000_000.0).max(0.001);
                if stats.connections.len() >= peak_stats.1.connections.len() {
                    peak_stats = (elapsed_s, stats.clone());
                }
                let max_down = stats
                    .connections
                    .iter()
                    .map(|c| c.link.bytes_sent as f64 / elapsed_s / 1024.0)
                    .fold(0.0f64, f64::max);
                outln!(
                    "[headless-host] t={:.1}s connected={}/{} max_down={:.1}KB/s",
                    elapsed_s,
                    stats.connections.len(),
                    spec.seats,
                    max_down
                );
                next_report_us += 2_000_000;
            }

            let until_tick = next_tick_us.saturating_sub(start.elapsed().as_micros() as u64);
            let wait_us = until_tick.min(400);
            if wait_us > 100 {
                transport.inner().wait(Duration::from_micros(wait_us));
            }
        }
    });

    let end_now = NetTime(start.elapsed().as_micros() as u64);
    let live_stats = server.stats();
    let (total_s, final_stats) = if live_stats.connections.len() >= peak_stats.1.connections.len() {
        ((end_now.0 as f64 / 1_000_000.0).max(0.001), live_stats)
    } else {
        peak_stats
    };
    server.shutdown(end_now);
    while let Some((to, bytes)) = server.poll_transmit(end_now) {
        Transport::send(&mut transport, to, &bytes);
    }
    transport.poll(end_now);
    let conns = final_stats.connections.len();
    let sum_down: f64 = final_stats
        .connections
        .iter()
        .map(|c| c.link.bytes_sent as f64 / total_s / 1024.0)
        .sum();
    let max_down: f64 = final_stats
        .connections
        .iter()
        .map(|c| c.link.bytes_sent as f64 / total_s / 1024.0)
        .fold(0.0f64, f64::max);
    let total_snaps: u64 = final_stats
        .connections
        .iter()
        .map(|c| c.stats.snapshots)
        .sum();
    let total_frags: u64 = final_stats
        .connections
        .iter()
        .map(|c| c.stats.fragments)
        .sum();
    let single_mtu = if total_snaps > 0 {
        total_snaps.saturating_sub(total_frags) as f64 / total_snaps as f64
    } else {
        1.0
    };
    let total_kfs: u64 = final_stats
        .connections
        .iter()
        .map(|c| c.stats.keyframes)
        .sum();
    let total_rel: u64 = final_stats
        .connections
        .iter()
        .map(|c| c.stats.reliable_deltas)
        .sum();
    let entities = world.snapshot().entities.len();
    let step_p50 = percentile_ms(&mut step_ns_samples, 50);
    let step_p95 = percentile_ms(&mut step_ns_samples, 95);
    let step_max = percentile_ms(&mut step_ns_samples, 100);
    let net_p50 = percentile_ms(&mut net_ns_samples, 50);
    let net_p95 = percentile_ms(&mut net_ns_samples, 95);
    let net_max = percentile_ms(&mut net_ns_samples, 100);
    outln!(
        "[headless-host-summary] connected={}/{} ticks={} entities={} snaps={} frags={} kfs={} rel={} sim_step_ms(p50={:.3},p95={:.3},max={:.3}) net_tx_ms(p50={:.3},p95={:.3},max={:.3}) down_kbps(mean={:.2},max={:.2}) single_mtu={:.4}",
        conns,
        spec.seats,
        step_ns_samples.len(),
        entities,
        total_snaps,
        total_frags,
        total_kfs,
        total_rel,
        step_p50,
        step_p95,
        step_max,
        net_p50,
        net_p95,
        net_max,
        if conns > 0 {
            sum_down / conns as f64
        } else {
            0.0
        },
        max_down,
        single_mtu,
    );
    ExitCode::SUCCESS
}

fn resolve_host(host: &str) -> Result<SocketAddr, String> {
    if let Ok(addr) = host.parse::<SocketAddr>() {
        return Ok(addr);
    }
    let with_port = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:{}", kjk_game::net_join::PORT)
    };
    with_port
        .to_socket_addrs()
        .map_err(|error| format!("resolve {host}: {error}"))?
        .next()
        .ok_or_else(|| format!("resolve {host}: no addresses"))
}

struct BotState {
    idx: u8,
    start_us: u64,
    next_frame_us: u64,
    client: Option<Client>,
    transport: Netem<Udp>,
    connected: bool,
    ended: bool,
    joined_at_us: Option<u64>,
    final_down_bytes: u64,
    final_rtt_us: u64,
    final_loss_ppm: u64,
}

fn load_module_cached(
    discovery: &Discovery,
    anims: &Arc<sim::motion::AnimTable>,
    cache: &mut HashMap<String, Level>,
    module: &str,
    seats: u8,
    players: u8,
) -> Result<Level, String> {
    if let Some(level) = cache.get(module) {
        return Ok(level.clone());
    }
    let level = Stage::named(
        discovery,
        module,
        None,
        anims.clone(),
        false,
        seats,
        players,
    )?
    .level;
    cache.insert(module.to_string(), level.clone());
    Ok(level)
}

fn resolve_level(
    discovery: &Discovery,
    anims: &Arc<sim::motion::AnimTable>,
    presets: &[(LevelId, Level)],
    cache: &mut HashMap<String, Level>,
    request: &LevelRequest,
) -> Result<Level, String> {
    match request {
        LevelRequest::Stage(id) => presets
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, level)| level.clone())
            .ok_or_else(|| format!("unknown stage level {id:?}")),
        LevelRequest::Module(ann) => {
            load_module_cached(discovery, anims, cache, &ann.module, ann.seats, ann.players)
        }
    }
}

fn resolve_install(
    discovery: &Discovery,
    anims: &Arc<sim::motion::AnimTable>,
    cache: &mut HashMap<String, Level>,
    request: &InstallRequest,
) -> Result<Level, String> {
    load_module_cached(
        discovery,
        anims,
        cache,
        &request.module,
        request.seats,
        request.players,
    )
}

struct BotSummary {
    connected: bool,
    ended: bool,
    down_kbps: f64,
    rtt_ms: f64,
    loss_ppm: u64,
}

/// Runs `spec.bots` headless simulated clients over real UDP sockets against `spec.host`.
pub(crate) fn run_bots(spec: &BotsRun<'_>) -> ExitCode {
    let server_addr = match resolve_host(spec.host) {
        Ok(addr) => addr,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let discovery = Arc::new(crate::discover());
    let games: Vec<Game> = discovery.installs.iter().map(|i| i.game).collect();
    let data = match world_data(&discovery, &games, &|_| {}) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("world data: {error}");
            return ExitCode::from(2);
        }
    };
    let (anims, _) = stage::anims(&discovery);
    let anims = Arc::new(anims);
    let presets = Arc::new(vec![
        {
            let l = Stage::course(anims.clone(), 1).level;
            (l.id(), l)
        },
        {
            let l = Stage::duel(anims.clone(), 1).level;
            (l.id(), l)
        },
        {
            let l = Stage::ring(anims.clone(), 1).level;
            (l.id(), l)
        },
        {
            let l = Level::arena();
            (l.id(), l)
        },
    ]);

    let bots = spec.bots.max(1);
    let workers = usize::from(bots).min(8);
    let end_us = spec.seconds.saturating_mul(1_000_000);
    let key = spec.password.map(JoinKey::new);
    let start = Instant::now();
    let bind_addr: SocketAddr = if server_addr.is_ipv6() {
        SocketAddr::from(([0u16; 8], 0))
    } else {
        SocketAddr::from(([0u8, 0, 0, 0], 0))
    };

    let summaries: Vec<BotSummary> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for worker_id in 0..workers {
            let discovery = &discovery;
            let anims = &anims;
            let presets = &presets;
            let data = &data;
            let conditions = spec.conditions;
            handles.push(scope.spawn(move || {
                let mut module_cache: HashMap<String, Level> = HashMap::new();
                let mut states = Vec::new();
                for idx in (worker_id as u8..bots).step_by(workers) {
                    let udp = Udp::bind_with(bind_addr, solo::clock(), SocketConfig::default())
                        .expect("bind bot udp socket");
                    let start_us = u64::from(idx) * 35_000;
                    states.push(BotState {
                        idx,
                        start_us,
                        next_frame_us: start_us,
                        client: None,
                        transport: Netem::new(udp, conditions),
                        connected: false,
                        ended: false,
                        joined_at_us: None,
                        final_down_bytes: 0,
                        final_rtt_us: 0,
                        final_loss_ppm: 0,
                    });
                }

                let frame_us = 16_667u64;
                let bot_rebase_us = if bots > 4 { 250_000 } else { REBASE_EVERY_US };
                loop {
                    let now_us = start.elapsed().as_micros() as u64;
                    if now_us >= end_us {
                        break;
                    }
                    let now = NetTime(now_us);
                    let mut had_work = false;
                    // Fast I/O & event pass across all bots in this thread so ACKs stay low-latency.
                    for bot in &mut states {
                        if bot.ended {
                            continue;
                        }
                        if bot.client.is_none() && now_us >= bot.start_us {
                            let bot_name = if bots == 1 {
                                spec.name.to_string()
                            } else {
                                format!("{}-{}", spec.name, bot.idx)
                            };
                            let creds = solo::credentials(&bot_name, data.clone());
                            let nonce = 0x9e37_79b9_7f4a_7c15_u64
                                .wrapping_add(u64::from(bot.idx) * 0xbf58_476d_1ce4_e5b9);
                            let config = ClientConfig {
                                link: net::link::Config::default(),
                                audit: false,
                                keep: false,
                                margins: Margins::default(),
                                rejoin_us: 5_000_000,
                                rebase_on: RebaseOn::Caller,
                                rebase_every_us: bot_rebase_us,
                                key,
                                nonce_key: [0; 32],
                                rebuild_beside: false,
                            };
                            if let Ok(client) =
                                Client::connect(creds, Peer::Addr(server_addr), nonce, now, config)
                            {
                                bot.client = Some(client);
                                bot.next_frame_us = now_us;
                            }
                        }
                        let Some(client) = bot.client.as_mut() else {
                            continue;
                        };
                        bot.transport.poll(now);
                        while let Some(dgram) = Transport::recv(&mut bot.transport) {
                            had_work = true;
                            client.receive(now, dgram.from, &dgram.bytes);
                        }
                        while let Some(event) = client.poll_event() {
                            match event {
                                ClientEvent::Accepted { .. } | ClientEvent::Loaded { .. } => {
                                    bot.connected = true;
                                    bot.joined_at_us.get_or_insert(now_us);
                                }
                                ClientEvent::NeedLevel(req) => {
                                    let level = resolve_level(
                                        discovery,
                                        anims,
                                        presets,
                                        &mut module_cache,
                                        &req,
                                    );
                                    client.provide_level(now, level);
                                }
                                ClientEvent::Installing(req) => {
                                    let level =
                                        resolve_install(discovery, anims, &mut module_cache, &req);
                                    client.provide_level(now, level);
                                }
                                ClientEvent::Ended(reason) => {
                                    if bot.idx == 0 {
                                        eprintln!("[bot-0] ended: {reason:?}");
                                    }
                                    bot.ended = true;
                                }
                                _ => {}
                            }
                        }
                        if let Some(link) = client.link_stats() {
                            bot.final_down_bytes = link.bytes_received;
                            bot.final_rtt_us = link.rtt_us;
                            bot.final_loss_ppm = link.loss_ppm;
                        }
                        while let Some((to, bytes)) = client.poll_transmit(now) {
                            had_work = true;
                            Transport::send(&mut bot.transport, to, &bytes);
                        }
                        bot.transport.poll(now);
                    }
                    // Frame / command pass.
                    for bot in &mut states {
                        if bot.ended {
                            continue;
                        }
                        let frame_now_us = start.elapsed().as_micros() as u64;
                        if frame_now_us < bot.next_frame_us {
                            continue;
                        }
                        let Some(client) = bot.client.as_mut() else {
                            continue;
                        };
                        had_work = true;
                        bot.next_frame_us = (bot.next_frame_us + frame_us).max(frame_now_us);
                        let frame_now = NetTime(frame_now_us);
                        let tick = client.next_tick().unwrap_or(0);
                        let yaw = ((tick.wrapping_mul(320) + u64::from(bot.idx).wrapping_mul(1024))
                            & 0xffff) as i16;
                        let atk = if (tick + u64::from(bot.idx)).is_multiple_of(8) {
                            buttons::ATTACK
                        } else {
                            0
                        };
                        let cmd = Command {
                            angles: [0, yaw, 0],
                            forward: 127,
                            buttons: atk,
                            ..Command::default()
                        };
                        let _ = client.frame(frame_now, cmd);
                        while let Some((to, bytes)) = client.poll_transmit(frame_now) {
                            Transport::send(&mut bot.transport, to, &bytes);
                        }
                        bot.transport.poll(frame_now);
                    }
                    let _ = had_work;
                    std::thread::sleep(Duration::from_millis(1));
                }

                let end_now = NetTime(start.elapsed().as_micros() as u64);
                for bot in &mut states {
                    if let Some(client) = bot.client.as_mut()
                        && !bot.ended
                    {
                        client.leave(end_now, Refusal::Left);
                        while let Some((to, bytes)) = client.poll_transmit(end_now) {
                            Transport::send(&mut bot.transport, to, &bytes);
                        }
                        bot.transport.poll(end_now);
                    }
                }
                states
                    .into_iter()
                    .map(|bot| {
                        let active_s = bot
                            .joined_at_us
                            .map_or(1.0f64, |at| {
                                end_now.0.saturating_sub(at) as f64 / 1_000_000.0
                            })
                            .max(0.001);
                        BotSummary {
                            connected: bot.connected,
                            ended: bot.ended,
                            down_kbps: bot.final_down_bytes as f64 / active_s / 1024.0,
                            rtt_ms: bot.final_rtt_us as f64 / 1000.0,
                            loss_ppm: bot.final_loss_ppm,
                        }
                    })
                    .collect::<Vec<_>>()
            }));
        }
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("bot worker thread"))
            .collect()
    });

    let connected = summaries.iter().filter(|b| b.connected).count();
    let ended = summaries.iter().filter(|b| b.ended).count();
    let count = summaries.len().max(1) as f64;
    let down_mean = summaries.iter().map(|b| b.down_kbps).sum::<f64>() / count;
    let down_max = summaries.iter().map(|b| b.down_kbps).fold(0.0f64, f64::max);
    let rtt_mean = summaries.iter().map(|b| b.rtt_ms).sum::<f64>() / count;
    let rtt_max = summaries.iter().map(|b| b.rtt_ms).fold(0.0f64, f64::max);
    let loss_max = summaries.iter().map(|b| b.loss_ppm).max().unwrap_or(0);
    outln!(
        "[headless-bots-summary] connected={}/{} ended={} rtt_ms(mean={:.2},max={:.2}) down_kbps(mean={:.2},max={:.2}) max_loss_ppm={}",
        connected,
        bots,
        ended,
        rtt_mean,
        rtt_max,
        down_mean,
        down_max,
        loss_max,
    );
    if connected == usize::from(bots) && ended == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
