//! The weather an area has, as the executable decides it: from the ARE's
//! chances, rolled once for each 300 s epoch of the sim's clock, alike on
//! every peer.

use crate::module::{AreaWeather, check};
use crate::msvc_rand::MsvcRand;

/// How often the weather is rolled again (the executable's server countdown).
pub const ROLL_MS: u64 = 300_000;

/// What falls from the sky.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fall {
    #[default]
    None,
    Rain,
    Snow,
}

/// The weather at a moment: what falls, the lightning's chance at each
/// check (0 when the weather is not running), and the area's wind power.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Weather {
    pub fall: Fall,
    pub lightning: i32,
    pub wind: i32,
}

/// The weather the roll `draw` (0..100) gives `area`. The chance is the
/// rain's if it has one, else the snow's, else the lightning's; the weather
/// runs while the roll is under it, so rain wins over snow and an area with
/// only lightning (Malachor) has lightning without a fall.
pub fn weather(area: &AreaWeather, draw: i32) -> Weather {
    let chance = [area.rain, area.snow, area.lightning]
        .into_iter()
        .find(|&chance| chance != 0)
        .unwrap_or(0);
    if draw >= chance {
        return Weather {
            wind: area.wind,
            ..Weather::default()
        };
    }
    let fall = match (area.rain, area.snow) {
        (0, 0) => Fall::None,
        (0, _) => Fall::Snow,
        _ => Fall::Rain,
    };
    Weather {
        fall,
        lightning: area.lightning,
        wind: area.wind,
    }
}

/// The roll of the `epoch`-th 300 s of the sim clock, alike on every peer:
/// the C library's `rand` seeded with FNV-1a 32 of the lowercase area
/// resref followed by the whole epoch's eight little-endian bytes,
/// `next() % 100`. Hashing the epoch in makes each epoch's roll a fresh
/// one; neighbouring seeds would give the LCG's first outputs as a ramp.
pub fn draw(area_resref: &str, epoch: u64) -> i32 {
    let seed = fnv1a(area_key(area_resref), epoch.to_le_bytes());
    (MsvcRand(seed).next() % 100) as i32
}

/// An area's own seed: FNV-1a 32 of its lowercase resref (the weather's
/// rolls and the storm's go by it).
pub fn area_key(area_resref: &str) -> u32 {
    fnv1a(
        0x811c_9dc5,
        area_resref.bytes().map(|b| b.to_ascii_lowercase()),
    )
}

/// FNV-1a 32 of `bytes`, carried on from `hash`.
fn fnv1a(hash: u32, bytes: impl IntoIterator<Item = u8>) -> u32 {
    bytes.into_iter().fold(hash, |hash, b| {
        (hash ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// The checks of an area's weather from its chances.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let dxun = AreaWeather {
        rain: 100,
        snow: 0,
        lightning: 100,
        wind: 1,
    };
    let polar = AreaWeather {
        rain: 0,
        snow: 100,
        lightning: 0,
        wind: 0,
    };
    let malachor = AreaWeather {
        rain: 0,
        snow: 0,
        lightning: 100,
        wind: 2,
    };
    let half = AreaWeather {
        rain: 50,
        snow: 30,
        lightning: 20,
        wind: 1,
    };
    let calm = |wind| Weather {
        fall: Fall::None,
        lightning: 0,
        wind,
    };
    let cases = [
        (
            "weather_dxun",
            dxun,
            [0, 50, 99],
            Weather {
                fall: Fall::Rain,
                lightning: 100,
                wind: 1,
            },
        ),
        (
            "weather_polar",
            polar,
            [0, 50, 99],
            Weather {
                fall: Fall::Snow,
                lightning: 0,
                wind: 0,
            },
        ),
        (
            "weather_malachor",
            malachor,
            [0, 50, 99],
            Weather {
                fall: Fall::None,
                lightning: 100,
                wind: 2,
            },
        ),
        (
            "weather_none",
            AreaWeather::default(),
            [0, 50, 99],
            Weather::default(),
        ),
        (
            "weather_rain_wins",
            half,
            [0, 25, 49],
            Weather {
                fall: Fall::Rain,
                lightning: 20,
                wind: 1,
            },
        ),
        ("weather_half_dry", half, [50, 75, 99], calm(1)),
    ];
    let mut checks: Vec<_> = cases
        .into_iter()
        .map(|(name, area, draws, expected)| {
            let got: Vec<Weather> = draws.iter().map(|&d| weather(&area, d)).collect();
            (
                name,
                check(got.iter().all(|w| *w == expected), format!("{got:?}")),
            )
        })
        .collect();
    let draws: Vec<i32> = (0..1000).map(|epoch| draw("401dxn", epoch)).collect();
    let wet: Vec<bool> = draws.iter().map(|&d| d < 50).collect();
    let share = wet.iter().filter(|&&w| w).count() as f32 / wet.len() as f32;
    let flips = wet.windows(2).filter(|w| w[0] != w[1]).count() as f32 / (wet.len() - 1) as f32;
    checks.push((
        "weather_draws_alike",
        check(
            draws == (0..1000).map(|e| draw("401DXN", e)).collect::<Vec<_>>()
                && draws.iter().all(|d| (0..100).contains(d))
                && (0.4..0.6).contains(&share)
                && flips > 0.25,
            format!(
                "first {:?}, under 50 {share:.2}, changed {flips:.2}",
                &draws[..8]
            ),
        ),
    ));
    checks
}
