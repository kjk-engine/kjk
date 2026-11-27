//! CS-style `net_graph` HUD overlay (`F8` or `--cmd net_graph`): live RTT,
//! jitter, packet loss, throughput, snapshot rate, clock lead/delay, rewind
//! rate, transport path, and degradation warning badge.

use std::time::Instant;

use bevy::prelude::*;
use console::NetGraphRequests;

/// User-toggled overlay state and rate-calculation baseline (`Down/Up KB/s`,
/// `Snaps/s`, `Rewinds/s` sampled at 4 Hz).
#[derive(Resource, Default)]
pub struct NetGraph {
    pub enabled: bool,
    last_sample: Option<Instant>,
    prev: Option<Counters>,
    text: String,
    warn: bool,
}

#[derive(Clone, Copy)]
struct Counters {
    bytes_down: u64,
    bytes_up: u64,
    snaps: u64,
    rewinds: u64,
}

/// Snapshot of current network metrics supplied by either `net_join::Joined`
/// or `solo::Solo` via `sim_host`.
#[derive(Clone, Copy, Default)]
pub(crate) struct NetGraphSample {
    pub rtt_ms: f64,
    pub jitter_ms: f64,
    pub loss_pct: f64,
    pub bytes_down: u64,
    pub bytes_up: u64,
    pub snaps: u64,
    pub rewinds: u64,
    pub lead_ticks: f64,
    pub delay_ticks: f64,
    pub path: &'static str,
}

/// Latest host-side network metrics published by `sim_host` when hosting.
#[derive(Resource, Default)]
pub(crate) struct HostGraph(pub Option<NetGraphSample>);

#[derive(Component)]
struct NetGraphBox;

#[derive(Component)]
struct NetGraphText;

pub(crate) struct NetGraphPlugin;

impl Plugin for NetGraphPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetGraph>()
            .init_resource::<HostGraph>()
            .add_systems(Update, (toggle, render).chain());
    }
}

fn toggle(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut requests: Option<ResMut<NetGraphRequests>>,
    mut graph: ResMut<NetGraph>,
) {
    if keys.is_some_and(|keys| keys.just_pressed(KeyCode::F8)) {
        graph.enabled = !graph.enabled;
        graph.last_sample = None;
    }
    if let Some(requests) = requests.as_deref_mut() {
        for next in requests.0.drain(..) {
            graph.enabled = next.unwrap_or(!graph.enabled);
            graph.last_sample = None;
        }
    }
}

fn format_graph(sample: &NetGraphSample, prev: Option<Counters>, dt_s: f64) -> (String, bool) {
    let dt = dt_s.max(0.05);
    let (down_kbps, up_kbps, snaps_s, rewinds_s) = match prev {
        Some(prev) => (
            sample.bytes_down.saturating_sub(prev.bytes_down) as f64 / 1000.0 / dt,
            sample.bytes_up.saturating_sub(prev.bytes_up) as f64 / 1000.0 / dt,
            sample.snaps.saturating_sub(prev.snaps) as f64 / dt,
            sample.rewinds.saturating_sub(prev.rewinds) as f64 / dt,
        ),
        None => (0.0, 0.0, 0.0, 0.0),
    };
    let warn = sample.rtt_ms > 120.0 || sample.loss_pct > 5.0;
    let badge = if warn { "[WARN] " } else { "" };
    let text = format!(
        "{badge}Path: {}  RTT: {:.1} ms  Jitter: {:.1} ms  Loss: {:.2}%\nDown: {:.1} KB/s  Up: {:.1} KB/s  Snaps: {:.1}/s  Lead: {:+.2} t  D: {:.2} t  Rewinds: {:.1}/s",
        sample.path,
        sample.rtt_ms,
        sample.jitter_ms,
        sample.loss_pct,
        down_kbps,
        up_kbps,
        snaps_s,
        sample.lead_ticks,
        sample.delay_ticks,
        rewinds_s,
    );
    (text, warn)
}

#[allow(clippy::too_many_arguments)]
fn render(
    mut commands: Commands,
    mut graph: ResMut<NetGraph>,
    joined: Option<Res<crate::net_join::Joined>>,
    host: Res<HostGraph>,
    mut boxes: Query<&mut Visibility, With<NetGraphBox>>,
    mut labels: Query<(&mut Text, &mut TextColor), With<NetGraphText>>,
) {
    if !graph.enabled {
        for mut vis in &mut boxes {
            *vis = Visibility::Hidden;
        }
        return;
    }
    if boxes.is_empty() {
        commands
            .spawn((
                NetGraphBox,
                Node {
                    position_type: PositionType::Absolute,
                    right: px(12),
                    bottom: px(12),
                    padding: UiRect::axes(px(10), px(6)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.07, 0.1, 0.78)),
                GlobalZIndex(1000),
                Visibility::Inherited,
            ))
            .with_child((
                NetGraphText,
                Text::new(String::new()),
                TextFont::from_font_size(13.0),
                TextColor(Color::srgb(0.85, 0.96, 0.85)),
            ));
    } else {
        for mut vis in &mut boxes {
            *vis = Visibility::Inherited;
        }
    }

    let now = Instant::now();
    let due = graph
        .last_sample
        .is_none_or(|last| now.duration_since(last).as_secs_f64() >= 0.25);
    if due {
        let dt_s = graph
            .last_sample
            .map_or(0.25, |last| now.duration_since(last).as_secs_f64());
        let sample = joined
            .as_deref()
            .and_then(crate::net_join::Joined::graph_sample)
            .or(host.0);
        if let Some(sample) = sample {
            let (text, warn) = format_graph(&sample, graph.prev, dt_s);
            graph.text = text;
            graph.warn = warn;
            graph.prev = Some(Counters {
                bytes_down: sample.bytes_down,
                bytes_up: sample.bytes_up,
                snaps: sample.snaps,
                rewinds: sample.rewinds,
            });
            graph.last_sample = Some(now);
        } else if graph.text.is_empty() {
            graph.text = "Path: Offline  RTT: --  Jitter: --  Loss: --".to_string();
            graph.warn = false;
            graph.last_sample = Some(now);
        }
    }

    let color = if graph.warn {
        Color::srgb(1.0, 0.45, 0.35)
    } else {
        Color::srgb(0.85, 0.96, 0.85)
    };
    for (mut text, mut text_color) in &mut labels {
        text.0 = graph.text.clone();
        text_color.0 = color;
    }
}
