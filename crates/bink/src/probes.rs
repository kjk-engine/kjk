//! The decoder's parts checked without any movie (the approved test
//! `bink_decoder_parts`). `kjk movie compare` checks whole movies against
//! `ffmpeg`, but needs the games installed.

use crate::Bink;
use crate::audio::Dct;
use crate::bits::Bits;
use crate::tables::TREE_LENGTHS;
use crate::video::{LONGEST_CODE, idct_put, lookup};

type Probe = (String, Result<String, String>);

fn check(name: &str, passed: bool, detail: String) -> Probe {
    (
        name.to_string(),
        if passed { Ok(detail) } else { Err(detail) },
    )
}

/// Every probe, in order.
pub fn probes() -> Vec<Probe> {
    let mut probes = vec![trees(), bit_order(), block_idct()];
    probes.extend([512, 1024, 2048].map(audio_dct));
    probes.extend(headers());
    probes
}

/// Each of the 16 Huffman trees has a code for every pattern of 7 bits,
/// and its code lengths add up to a whole tree.
fn trees() -> Probe {
    let broken: Vec<usize> = (0..16)
        .filter(|&tree| {
            let full = lookup()[tree].iter().all(|&(_, length)| length > 0);
            let kraft: f64 = TREE_LENGTHS[tree]
                .iter()
                .map(|&length| 0.5f64.powi(i32::from(length)))
                .sum();
            !full || (kraft - 1.0).abs() > 1e-9
        })
        .collect();
    check(
        "huffman trees",
        broken.is_empty(),
        format!(
            "every one of the 16 trees covers all {} patterns of {LONGEST_CODE} bits; broken: {broken:?}",
            1 << LONGEST_CODE
        ),
    )
}

/// Bits are read least significant first, and reading past the end is told.
fn bit_order() -> Probe {
    let mut bits = Bits::new(&[0b1010_0110, 0xFF, 0x01]);
    let read = [bits.read(1), bits.read(2), bits.read(5), bits.read(9)];
    bits.align32();
    let passed = read == [0, 0b11, 0b10100, 0x1FF] && bits.left() == -8 && bits.overrun();
    check(
        "bit order",
        passed,
        format!(
            "read {read:?} from 0xA6 0xFF 0x01, then {} bits left after aligning",
            bits.left()
        ),
    )
}

/// The block inverse DCT turns a DC value alone into an even block of
/// `(DC + 127) / 256`.
fn block_idct() -> Probe {
    let mut block = [0i32; 64];
    block[0] = 800;
    let mut pixels = [0u8; 64];
    idct_put(&mut pixels, 8, &mut block);
    let passed = pixels.iter().all(|&pixel| pixel == 3);
    check(
        "block inverse DCT",
        passed,
        format!("a DC of 800 alone gives pixels {:?}", &pixels[..8]),
    )
}

/// The audio's fast inverse DCT gives what its definition gives.
fn audio_dct(size: usize) -> Probe {
    let input: Vec<f32> = (0..size)
        .map(|i| ((i * 37 % 11) as f32 - 5.0) / 3.0)
        .collect();
    let mut fast = input.clone();
    Dct::new(size).inverse(&mut fast);
    let largest = (0..size)
        .map(|n| {
            let sum: f64 = (1..size)
                .map(|k| {
                    f64::from(input[k])
                        * (std::f64::consts::PI * k as f64 * (2 * n + 1) as f64 / (2 * size) as f64)
                            .cos()
                })
                .sum();
            let slow = 2.0 / size as f64 * (f64::from(input[0]) / 2.0 + sum);
            (f64::from(fast[n]) - slow).abs()
        })
        .fold(0.0, f64::max);
    check(
        &format!("audio inverse DCT of {size}"),
        largest < 1e-4,
        format!("the largest difference from the definition is {largest:e}"),
    )
}

/// A header of one empty frame and no sound, with its tag and flags as given.
fn header(tag: &[u8; 4], flags: u32) -> Vec<u8> {
    let mut bytes = tag.to_vec();
    let words = [0, 1, 0, 1, 640, 272, 30, 1, flags, 0];
    for word in words {
        bytes.extend_from_slice(&u32::to_le_bytes(word));
    }
    // The frame index: one frame, starting right after it.
    let start = bytes.len() as u32 + 4;
    bytes.extend_from_slice(&(start | 1).to_le_bytes());
    bytes
}

/// A Bink 1 revision "i" header reads. Bink 2, an older revision and a grey
/// movie are refused, so that `ffmpeg` gets them.
fn headers() -> Vec<Probe> {
    let read = Bink::parse(header(b"BIKi", 0));
    let mut probes = vec![check(
        "header read",
        read.as_ref().is_ok_and(|bink| {
            (bink.width, bink.height, bink.rate, bink.frames()) == (640, 272, (30, 1), 1)
                && bink.tracks.is_empty()
        }),
        format!(
            "640×272 at 30 frames a second with one frame and no sound: {:?}",
            read.as_ref()
                .map(|bink| (bink.width, bink.height, bink.frames()))
        ),
    )];
    for (name, tag, flags) in [
        ("Bink 2 refused", b"KB2a", 0),
        ("older revision refused", b"BIKf", 0),
        ("grey movie refused", b"BIKi", 0x0002_0000),
    ] {
        let refused = Bink::parse(header(tag, flags)).err();
        probes.push(check(
            name,
            refused.is_some(),
            format!("refused with {refused:?}"),
        ));
    }
    probes
}
