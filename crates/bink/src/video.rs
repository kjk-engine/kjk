//! Bink 1 video. Each frame codes its planes one after another: the alpha
//! plane if the movie has one, then the luma plane, then the two chroma
//! planes at half size. A plane is coded as rows of 8×8 blocks. Each block
//! has one of ten kinds, from a copy of the last frame to a block of DCT
//! coefficients. The values the blocks use come in nine "bundles", and each
//! row of blocks refills the bundles it has used up.

use std::sync::OnceLock;

use crate::Bink;
use crate::bits::Bits;
use crate::tables::{INTER_QUANT, INTRA_QUANT, PATTERNS, SCAN, TREE_CODES, TREE_LENGTHS};

// The bundles.
const BLOCK_TYPES: usize = 0;
const SUB_BLOCK_TYPES: usize = 1;
const COLOURS: usize = 2;
const PATTERN: usize = 3;
const X_OFF: usize = 4;
const Y_OFF: usize = 5;
const INTRA_DC: usize = 6;
const INTER_DC: usize = 7;
const RUN: usize = 8;
const BUNDLES: usize = 9;

// The kinds of block.
const SKIP: i32 = 0;
const SCALED: i32 = 1;
const MOTION: i32 = 2;
const RUN_BLOCK: i32 = 3;
const RESIDUE: i32 = 4;
const INTRA: i32 = 5;
const FILL: i32 = 6;
const INTER: i32 = 7;
const PATTERN_BLOCK: i32 = 8;
const RAW: i32 = 9;

/// The lengths of the runs that block types 12 to 15 repeat the last type for.
const TYPE_RUNS: [usize; 4] = [4, 8, 12, 32];
/// How many bits a bundle's first DC value has.
const DC_START_BITS: u32 = 11;
/// The longest Huffman code, in bits.
const LONGEST_CODE: u32 = 7;

/// Each of the 16 Huffman trees as a table looked up by the next 7 bits:
/// the code's index in the tree and its length.
fn lookup() -> &'static [[(u8, u8); 1 << LONGEST_CODE]; 16] {
    static LOOKUP: OnceLock<[[(u8, u8); 1 << LONGEST_CODE]; 16]> = OnceLock::new();
    LOOKUP.get_or_init(|| {
        let mut lookup = [[(0, 0); 1 << LONGEST_CODE]; 16];
        for (tree, table) in lookup.iter_mut().enumerate() {
            for symbol in 0..16 {
                let length = TREE_LENGTHS[tree][symbol];
                let code = usize::from(TREE_CODES[tree][symbol]);
                for entry in (code..table.len()).step_by(1 << length) {
                    table[entry] = (symbol as u8, length);
                }
            }
        }
        lookup
    })
}

/// A Huffman tree as a plane uses it: one of the 16 fixed trees, and the
/// order its 16 symbols stand in.
#[derive(Clone, Copy, Default)]
struct Tree {
    table: usize,
    symbols: [u8; 16],
}

impl Tree {
    fn read(bits: &mut Bits) -> Tree {
        let table = bits.read(4) as usize;
        let mut symbols: [u8; 16] = std::array::from_fn(|i| i as u8);
        if table == 0 {
            return Tree { table, symbols };
        }
        if bits.bit() {
            // Some symbols listed first, then the rest in order.
            let mut last = bits.read(3) as usize;
            let mut listed = [false; 16];
            for symbol in symbols.iter_mut().take(last + 1) {
                *symbol = bits.read(4) as u8;
                listed[usize::from(*symbol)] = true;
            }
            for (value, _) in listed.iter().enumerate().filter(|(_, listed)| !**listed) {
                if last >= 15 {
                    break;
                }
                last += 1;
                symbols[last] = value as u8;
            }
        } else {
            // Runs of 1, 2, 4 and 8 symbols merged pairwise, as a merge sort
            // does, with one bit for each pick.
            let passes = bits.read(2);
            let mut merged = [0u8; 16];
            for pass in 0..=passes {
                let size = 1usize << pass;
                for start in (0..16).step_by(2 * size) {
                    merge(
                        bits,
                        &mut merged[start..start + 2 * size],
                        &symbols[start..start + 2 * size],
                        size,
                    );
                }
                std::mem::swap(&mut symbols, &mut merged);
            }
        }
        Tree { table, symbols }
    }

    fn get(&self, bits: &mut Bits) -> i32 {
        let (index, length) = lookup()[self.table][bits.peek(LONGEST_CODE) as usize];
        bits.skip(usize::from(length));
        i32::from(self.symbols[usize::from(index)])
    }
}

/// Merges the two runs of `size` symbols in `from` into `to`. Each bit
/// picks the next symbol from the first run (0) or the second (1).
fn merge(bits: &mut Bits, to: &mut [u8], from: &[u8], size: usize) {
    let (mut first, mut second) = (0, size);
    let mut out = 0;
    while first < size && second < 2 * size {
        if bits.bit() {
            to[out] = from[second];
            second += 1;
        } else {
            to[out] = from[first];
            first += 1;
        }
        out += 1;
    }
    for &symbol in from[first..size].iter().chain(&from[second..2 * size]) {
        to[out] = symbol;
        out += 1;
    }
}

/// One bundle of values. A row of blocks reads a new piece of it only once
/// every value read before is used, and a piece of no values ends it for
/// the plane.
#[derive(Default)]
struct Bundle {
    /// How many bits a piece's count of values has.
    count_bits: u32,
    tree: Tree,
    values: Vec<i32>,
    next: usize,
    ended: bool,
}

impl Bundle {
    fn start(&mut self) {
        self.values.clear();
        self.next = 0;
        self.ended = false;
    }

    /// How many values the next piece has, if this row reads one.
    fn piece(&mut self, bits: &mut Bits) -> Option<usize> {
        if self.ended || self.values.len() > self.next {
            return None;
        }
        let count = bits.read(self.count_bits) as usize;
        if count == 0 {
            self.ended = true;
            return None;
        }
        Some(count)
    }

    fn take(&mut self) -> i32 {
        let value = self.values.get(self.next).copied().unwrap_or(0);
        self.next += 1;
        value
    }

    fn fill(&mut self, value: i32, count: usize) {
        self.values.extend(std::iter::repeat_n(value, count));
    }
}

/// A value with a sign bit after it, if it is not 0.
fn signed(bits: &mut Bits, value: i32) -> i32 {
    if value != 0 && bits.bit() {
        -value
    } else {
        value
    }
}

/// One plane of a frame, padded to whole blocks (and one block more, which
/// a 16×16 block at the right or bottom edge may write into).
#[derive(Clone)]
struct Plane {
    stride: usize,
    pixels: Vec<u8>,
}

impl Plane {
    fn new(blocks_wide: usize, blocks_high: usize) -> Plane {
        let stride = (blocks_wide + 2) * 8;
        Plane {
            stride,
            pixels: vec![0; stride * (blocks_high + 2) * 8],
        }
    }
}

/// The decoder of a movie's pictures. It keeps the last frame, which the
/// next frame's blocks copy from.
pub struct Video {
    width: usize,
    height: usize,
    alpha: bool,
    /// The planes: luma, the two chromas, and alpha.
    current: Vec<Plane>,
    last: Vec<Plane>,
    bundles: [Bundle; BUNDLES],
    /// The 16 trees the colours' high nibbles are read with, one for each
    /// value of the colour before.
    colour_trees: [Tree; 16],
    last_high: i32,
}

/// The size of a plane in blocks.
fn blocks(width: usize, height: usize, chroma: bool) -> (usize, usize) {
    if chroma {
        (width.div_ceil(16), height.div_ceil(16))
    } else {
        (width.div_ceil(8), height.div_ceil(8))
    }
}

/// The base-2 logarithm of `value`, rounded down.
fn log2(value: usize) -> u32 {
    usize::BITS - 1 - value.leading_zeros()
}

impl Video {
    pub fn new(bink: &Bink) -> Video {
        let (width, height) = (bink.width as usize, bink.height as usize);
        let planes: Vec<Plane> = (0..4)
            .map(|plane| {
                let (wide, high) = blocks(width, height, plane == 1 || plane == 2);
                Plane::new(wide, high)
            })
            .collect();
        Video {
            width,
            height,
            alpha: bink.alpha,
            last: planes.clone(),
            current: planes,
            bundles: Default::default(),
            colour_trees: [Tree::default(); 16],
            last_high: 0,
        }
    }

    /// Decodes the next frame from its video packet.
    pub fn decode(&mut self, packet: &[u8]) -> Result<(), String> {
        std::mem::swap(&mut self.current, &mut self.last);
        let mut bits = Bits::new(packet);
        if self.alpha {
            bits.skip(32);
            self.plane(&mut bits, 3, false)?;
        }
        bits.skip(32);
        // The second plane coded is the second chroma (Cr), then the first (Cb).
        for plane in [0, 2, 1] {
            self.plane(&mut bits, plane, plane != 0)?;
            if bits.left() <= 0 {
                break;
            }
        }
        Ok(())
    }

    /// The last frame decoded, as RGBA, `width × height × 4` bytes. The
    /// colours are converted as BT.601 with video levels (16 to 235).
    pub fn rgba(&self, out: &mut Vec<u8>) {
        out.clear();
        out.reserve(self.width * self.height * 4);
        let [luma, cb, cr, _] = &self.current[..] else {
            unreachable!("four planes")
        };
        for y in 0..self.height {
            let row = &luma.pixels[y * luma.stride..];
            let cb = &cb.pixels[(y / 2) * cb.stride..];
            let cr = &cr.pixels[(y / 2) * cr.stride..];
            for x in 0..self.width {
                let l = (i32::from(row[x]) - 16) * 298;
                let u = i32::from(cb[x / 2]) - 128;
                let v = i32::from(cr[x / 2]) - 128;
                let channel = |value: i32| ((value + 128) >> 8).clamp(0, 255) as u8;
                out.extend_from_slice(&[
                    channel(l + 409 * v),
                    channel(l - 100 * u - 208 * v),
                    channel(l + 516 * u),
                    255,
                ]);
            }
        }
    }

    /// The last frame's planes, cropped to the picture: luma, Cb and Cr.
    /// The tests compare these with `ffmpeg`'s.
    pub fn planes(&self) -> [Vec<u8>; 3] {
        std::array::from_fn(|plane| {
            let (width, height) = if plane == 0 {
                (self.width, self.height)
            } else {
                (self.width.div_ceil(2), self.height.div_ceil(2))
            };
            let source = &self.current[plane];
            (0..height)
                .flat_map(|y| &source.pixels[y * source.stride..y * source.stride + width])
                .copied()
                .collect()
        })
    }

    fn plane(&mut self, bits: &mut Bits, plane: usize, chroma: bool) -> Result<(), String> {
        let (wide, high) = blocks(self.width, self.height, chroma);
        let width = (self.width >> usize::from(chroma))
            .max(8)
            .next_multiple_of(8);
        let lengths = [
            (BLOCK_TYPES, width >> 3),
            (SUB_BLOCK_TYPES, width >> 4),
            (COLOURS, wide * 64),
            (PATTERN, wide << 3),
            (X_OFF, width >> 3),
            (Y_OFF, width >> 3),
            (INTRA_DC, width >> 3),
            (INTER_DC, width >> 3),
            (RUN, wide * 48),
        ];
        for (bundle, count) in lengths {
            self.bundles[bundle].count_bits = log2(count + 511) + 1;
        }
        for bundle in 0..BUNDLES {
            if bundle == COLOURS {
                for tree in &mut self.colour_trees {
                    *tree = Tree::read(bits);
                }
                self.last_high = 0;
            }
            if bundle != INTRA_DC && bundle != INTER_DC {
                self.bundles[bundle].tree = Tree::read(bits);
            }
            self.bundles[bundle].start();
        }
        // The plane's pixels are taken out while its rows read the bundles.
        let stride = self.current[plane].stride;
        let mut current = std::mem::take(&mut self.current[plane].pixels);
        let last = std::mem::take(&mut self.last[plane].pixels);
        let decoded = self.rows(bits, (&mut current, &last), stride, (wide, high));
        self.current[plane].pixels = current;
        self.last[plane].pixels = last;
        decoded?;
        bits.align32();
        if bits.overrun() {
            return Err("a plane runs past its packet".into());
        }
        Ok(())
    }

    fn rows(
        &mut self,
        bits: &mut Bits,
        (current, last): (&mut [u8], &[u8]),
        stride: usize,
        (wide, high): (usize, usize),
    ) -> Result<(), String> {
        // The furthest a motion vector may point: the last block's corner.
        let furthest = (high - 1) * 8 * stride + (wide - 1) * 8;
        for row in 0..high {
            self.read_row(bits)?;
            let mut column = 0;
            while column < wide {
                let at = row * 8 * stride + column * 8;
                let kind = self.bundles[BLOCK_TYPES].take();
                // A 16×16 block covers this row's block and the next, which
                // the row below then skips.
                if kind == SCALED && row % 2 == 1 {
                    column += 2;
                    continue;
                }
                let mut block = Block {
                    bundles: &mut self.bundles,
                    bits,
                    current: &mut *current,
                    last,
                    stride,
                    at,
                    furthest,
                };
                match kind {
                    SKIP => block.copy(at),
                    SCALED => {
                        block.scaled()?;
                        column += 1;
                    }
                    MOTION => {
                        let from = block.motion()?;
                        block.copy(from);
                    }
                    RUN_BLOCK => {
                        let mut pixels = [0u8; 64];
                        block.run(&mut pixels)?;
                        block.put(&pixels);
                    }
                    RESIDUE => {
                        let from = block.motion()?;
                        block.copy(from);
                        let masks = block.bits.read(7) as i32;
                        let mut residue = [0i32; 64];
                        block.residue(&mut residue, masks);
                        block.add(&residue);
                    }
                    INTRA => {
                        let mut coefficients = [0i32; 64];
                        coefficients[0] = block.bundles[INTRA_DC].take();
                        block.coefficients(&mut coefficients, &INTRA_QUANT);
                        let mut pixels = [0u8; 64];
                        idct_put(&mut pixels, 8, &mut coefficients);
                        block.put(&pixels);
                    }
                    FILL => {
                        let colour = block.bundles[COLOURS].take() as u8;
                        block.fill(colour, 8);
                    }
                    INTER => {
                        let from = block.motion()?;
                        block.copy(from);
                        let mut coefficients = [0i32; 64];
                        coefficients[0] = block.bundles[INTER_DC].take();
                        block.coefficients(&mut coefficients, &INTER_QUANT);
                        idct(&mut coefficients);
                        block.add(&coefficients);
                    }
                    PATTERN_BLOCK => {
                        let mut pixels = [0u8; 64];
                        block.pattern(&mut pixels);
                        block.put(&pixels);
                    }
                    RAW => {
                        let mut pixels = [0u8; 64];
                        for pixel in &mut pixels {
                            *pixel = block.bundles[COLOURS].take() as u8;
                        }
                        block.put(&pixels);
                    }
                    _ => return Err(format!("a block of kind {kind}")),
                }
                column += 1;
            }
        }
        Ok(())
    }

    /// Reads a new piece of every bundle that this row needs one of.
    fn read_row(&mut self, bits: &mut Bits) -> Result<(), String> {
        self.read_types(bits, BLOCK_TYPES)?;
        self.read_types(bits, SUB_BLOCK_TYPES)?;
        self.read_colours(bits);
        let patterns = &mut self.bundles[PATTERN];
        if let Some(count) = patterns.piece(bits) {
            for _ in 0..count {
                let low = patterns.tree.get(bits);
                let value = low | (patterns.tree.get(bits) << 4);
                patterns.values.push(value);
            }
        }
        for offsets in [X_OFF, Y_OFF] {
            let bundle = &mut self.bundles[offsets];
            if let Some(count) = bundle.piece(bits) {
                if bits.bit() {
                    let value = bits.read(4) as i32;
                    let value = signed(bits, value);
                    bundle.fill(value, count);
                } else {
                    for _ in 0..count {
                        let value = bundle.tree.get(bits);
                        bundle.values.push(signed(bits, value));
                    }
                }
            }
        }
        read_dcs(&mut self.bundles[INTRA_DC], bits, false)?;
        read_dcs(&mut self.bundles[INTER_DC], bits, true)?;
        let runs = &mut self.bundles[RUN];
        if let Some(count) = runs.piece(bits) {
            if bits.bit() {
                let value = bits.read(4) as i32;
                runs.fill(value, count);
            } else {
                for _ in 0..count {
                    let value = runs.tree.get(bits);
                    runs.values.push(value);
                }
            }
        }
        Ok(())
    }

    fn read_types(&mut self, bits: &mut Bits, which: usize) -> Result<(), String> {
        let bundle = &mut self.bundles[which];
        let Some(count) = bundle.piece(bits) else {
            return Ok(());
        };
        if bits.bit() {
            let value = bits.read(4) as i32;
            bundle.fill(value, count);
            return Ok(());
        }
        let end = bundle.values.len() + count;
        let mut last = 0;
        while bundle.values.len() < end {
            let value = bundle.tree.get(bits);
            if value < 12 {
                last = value;
                bundle.values.push(value);
            } else {
                let run = TYPE_RUNS[(value - 12) as usize];
                if bundle.values.len() + run > end {
                    return Err("a run of block types runs past its piece".into());
                }
                bundle.fill(last, run);
            }
        }
        Ok(())
    }

    fn read_colours(&mut self, bits: &mut Bits) {
        let bundle = &mut self.bundles[COLOURS];
        let Some(count) = bundle.piece(bits) else {
            return;
        };
        let tree = bundle.tree;
        let mut colour = |bits: &mut Bits| {
            self.last_high = self.colour_trees[self.last_high as usize].get(bits);
            (self.last_high << 4) | tree.get(bits)
        };
        if bits.bit() {
            let value = colour(bits);
            bundle.fill(value, count);
        } else {
            for _ in 0..count {
                let value = colour(bits);
                bundle.values.push(value);
            }
        }
    }
}

/// Reads a piece of DC values: the first in full, then the others in
/// groups of up to 8, each as a difference from the one before.
fn read_dcs(bundle: &mut Bundle, bits: &mut Bits, has_sign: bool) -> Result<(), String> {
    let Some(count) = bundle.piece(bits) else {
        return Ok(());
    };
    let first = bits.read(DC_START_BITS - u32::from(has_sign)) as i32;
    let mut value = if has_sign { signed(bits, first) } else { first };
    bundle.values.push(value);
    let rest = count - 1;
    for group in (0..rest).step_by(8) {
        let size = (rest - group).min(8);
        let width = bits.read(4);
        for _ in 0..size {
            if width != 0 {
                let difference = bits.read(width) as i32;
                value += signed(bits, difference);
                if !(-32768..=32767).contains(&value) {
                    return Err("a DC value out of range".into());
                }
            }
            bundle.values.push(value);
        }
    }
    Ok(())
}

/// One block being decoded: where it is, and what it reads from.
struct Block<'a, 'b> {
    bundles: &'a mut [Bundle; BUNDLES],
    bits: &'a mut Bits<'b>,
    current: &'a mut [u8],
    last: &'a [u8],
    stride: usize,
    at: usize,
    furthest: usize,
}

impl Block<'_, '_> {
    /// Where the block's motion vector points in the last frame.
    fn motion(&mut self) -> Result<usize, String> {
        let x = self.bundles[X_OFF].take() as isize;
        let y = self.bundles[Y_OFF].take() as isize;
        let from = self.at as isize + x + y * self.stride as isize;
        if from < 0 || from as usize > self.furthest {
            return Err(format!(
                "a motion vector of ({x}, {y}) points out of the picture"
            ));
        }
        Ok(from as usize)
    }

    /// Copies the last frame's 8×8 pixels at `from` into the block.
    fn copy(&mut self, from: usize) {
        for row in 0..8 {
            let (to, from) = (self.at + row * self.stride, from + row * self.stride);
            self.current[to..to + 8].copy_from_slice(&self.last[from..from + 8]);
        }
    }

    fn put(&mut self, pixels: &[u8; 64]) {
        for (row, line) in pixels.chunks_exact(8).enumerate() {
            let to = self.at + row * self.stride;
            self.current[to..to + 8].copy_from_slice(line);
        }
    }

    /// Adds a residue to the block's pixels. The sum wraps, as Bink's does.
    fn add(&mut self, residue: &[i32; 64]) {
        for (row, line) in residue.chunks_exact(8).enumerate() {
            let to = self.at + row * self.stride;
            for (pixel, &value) in self.current[to..to + 8].iter_mut().zip(line) {
                *pixel = (i32::from(*pixel) + value) as u8;
            }
        }
    }

    fn fill(&mut self, colour: u8, size: usize) {
        for row in 0..size {
            let to = self.at + row * self.stride;
            self.current[to..to + size].fill(colour);
        }
    }

    /// A 16×16 block: one of five kinds of 8×8 block, drawn at twice its
    /// size.
    fn scaled(&mut self) -> Result<(), String> {
        let kind = self.bundles[SUB_BLOCK_TYPES].take();
        let mut pixels = [0u8; 64];
        match kind {
            RUN_BLOCK => self.run(&mut pixels)?,
            INTRA => {
                let mut coefficients = [0i32; 64];
                coefficients[0] = self.bundles[INTRA_DC].take();
                self.coefficients(&mut coefficients, &INTRA_QUANT);
                idct_put(&mut pixels, 8, &mut coefficients);
            }
            FILL => {
                let colour = self.bundles[COLOURS].take() as u8;
                self.fill(colour, 16);
                return Ok(());
            }
            PATTERN_BLOCK => self.pattern(&mut pixels),
            RAW => {
                for pixel in &mut pixels {
                    *pixel = self.bundles[COLOURS].take() as u8;
                }
            }
            _ => return Err(format!("a 16×16 block of kind {kind}")),
        }
        for (row, line) in pixels.chunks_exact(8).enumerate() {
            for twice in 0..2 {
                let to = self.at + (row * 2 + twice) * self.stride;
                for (x, &pixel) in line.iter().enumerate() {
                    self.current[to + 2 * x] = pixel;
                    self.current[to + 2 * x + 1] = pixel;
                }
            }
        }
        Ok(())
    }

    /// Runs of one colour or of single colours, in one of 16 orders.
    fn run(&mut self, pixels: &mut [u8; 64]) -> Result<(), String> {
        let order = &PATTERNS[self.bits.read(4) as usize];
        let mut done = 0;
        while done < 63 {
            let run = self.bundles[RUN].take() as usize + 1;
            if done + run > 64 {
                return Err("a run past its block".into());
            }
            if self.bits.bit() {
                let colour = self.bundles[COLOURS].take() as u8;
                for &place in &order[done..done + run] {
                    pixels[usize::from(place)] = colour;
                }
            } else {
                for &place in &order[done..done + run] {
                    pixels[usize::from(place)] = self.bundles[COLOURS].take() as u8;
                }
            }
            done += run;
        }
        if done == 63 {
            pixels[usize::from(order[63])] = self.bundles[COLOURS].take() as u8;
        }
        Ok(())
    }

    /// Two colours and one byte for each row, whose bits pick between them.
    fn pattern(&mut self, pixels: &mut [u8; 64]) {
        let colours = [
            self.bundles[COLOURS].take() as u8,
            self.bundles[COLOURS].take() as u8,
        ];
        for line in pixels.chunks_exact_mut(8) {
            let mut bits = self.bundles[PATTERN].take();
            for pixel in line {
                *pixel = colours[(bits & 1) as usize];
                bits >>= 1;
            }
        }
    }

    /// Reads a block's DCT coefficients after its DC, and multiplies them
    /// by the quantiser the block names.
    fn coefficients(&mut self, block: &mut [i32; 64], quantisers: &[[u32; 64]; 16]) {
        let mut coded = [0usize; 64];
        let mut count = 0;
        let bits = &mut *self.bits;
        let mut store = |bits: &mut Bits, at: usize, width: u32| {
            let value = if width == 0 {
                if bits.bit() { -1 } else { 1 }
            } else {
                let magnitude = (bits.read(width) | (1 << width)) as i32;
                if bits.bit() { -magnitude } else { magnitude }
            };
            block[usize::from(SCAN[at])] = value;
            coded[count] = at;
            count += 1;
        };
        let top = bits.read(4) as i32;
        let mut list = Candidates::new(&[(4, 0), (24, 0), (44, 0), (1, 3), (2, 3), (3, 3)]);
        for width in (0..top).rev() {
            list.walk(bits, |bits, at| {
                store(bits, at, width as u32);
                false
            });
        }
        let quantiser = &quantisers[bits.read(4) as usize];
        block[0] = (block[0] * quantiser[0] as i32) >> 11;
        for &at in &coded[..count] {
            let place = usize::from(SCAN[at]);
            block[place] = (block[place] * quantiser[at] as i32) >> 11;
        }
    }

    /// Reads a residue: coefficients coded bit plane by bit plane, from the
    /// highest. `masks` is how many bits it may set in all.
    fn residue(&mut self, block: &mut [i32; 64], mut masks: i32) {
        let bits = &mut *self.bits;
        let mut set: Vec<usize> = Vec::with_capacity(64);
        let mut list = Candidates::new(&[(4, 0), (24, 0), (44, 0), (0, 2)]);
        let mut mask = 1i32 << bits.read(3);
        while mask != 0 {
            for &place in &set {
                if !bits.bit() {
                    continue;
                }
                block[place] += if block[place] < 0 { -mask } else { mask };
                masks -= 1;
                if masks < 0 {
                    return;
                }
            }
            let stopped = list.walk(bits, |bits, at| {
                let place = usize::from(SCAN[at]);
                set.push(place);
                block[place] = if bits.bit() { -mask } else { mask };
                masks -= 1;
                masks < 0
            });
            if stopped {
                return;
            }
            mask >>= 1;
        }
    }
}

/// The list of coefficient groups a block's coefficients are found in.
/// A group is a coefficient and a mode: 0 is a group of 16 not yet split,
/// 1 is one split into its other three groups of 4, 2 is a group of 4, and
/// 3 is a single coefficient. A walk visits the list once, and each group
/// with its bit set is split or read.
struct Candidates {
    groups: [(usize, u8); 128],
    start: usize,
    end: usize,
}

impl Candidates {
    fn new(first: &[(usize, u8)]) -> Candidates {
        let mut groups = [(0, 0); 128];
        groups[64..64 + first.len()].copy_from_slice(first);
        Candidates {
            groups,
            start: 64,
            end: 64 + first.len(),
        }
    }

    /// One walk over the list. `read` is called for each coefficient found,
    /// and the walk stops when it returns true. A coefficient of a group of
    /// 4 whose own bit is set is not read yet. It goes to the front of the
    /// list as a single coefficient (mode 3), for the next walk.
    /// Returns whether `read` stopped the walk.
    fn walk(&mut self, bits: &mut Bits, mut read: impl FnMut(&mut Bits, usize) -> bool) -> bool {
        let mut at = self.start;
        while at < self.end {
            let (coefficient, mode) = self.groups[at];
            if (coefficient == 0 && mode == 0) || !bits.bit() {
                at += 1;
                continue;
            }
            match mode {
                0 | 2 => {
                    if mode == 0 {
                        self.groups[at] = (coefficient + 4, 1);
                    } else {
                        self.groups[at] = (0, 0);
                        at += 1;
                    }
                    for coefficient in coefficient..coefficient + 4 {
                        if bits.bit() {
                            self.start -= 1;
                            self.groups[self.start] = (coefficient, 3);
                        } else if read(bits, coefficient) {
                            return true;
                        }
                    }
                }
                1 => {
                    self.groups[at].1 = 2;
                    for group in 1..=3 {
                        self.groups[self.end] = (coefficient + 4 * group, 2);
                        self.end += 1;
                    }
                }
                _ => {
                    let stop = read(bits, coefficient);
                    self.groups[at] = (0, 0);
                    at += 1;
                    if stop {
                        return true;
                    }
                }
            }
        }
        false
    }
}

// Bink's integer inverse DCT.
const A1: i32 = 2896;
const A2: i32 = 2217;
const A3: i32 = 3784;
const A4: i32 = -5352;

/// One 8-point inverse transform, of `input[at(0..8)]`.
fn transform(input: impl Fn(usize) -> i32) -> [i32; 8] {
    let a0 = input(0) + input(4);
    let a1 = input(0) - input(4);
    let a2 = input(2) + input(6);
    let a3 = (A1 * (input(2) - input(6))) >> 11;
    let a4 = input(5) + input(3);
    let a5 = input(5) - input(3);
    let a6 = input(1) + input(7);
    let a7 = input(1) - input(7);
    let b0 = a4 + a6;
    let b1 = (A3 * (a5 + a7)) >> 11;
    let b2 = ((A4 * a5) >> 11) - b0 + b1;
    let b3 = ((A1 * (a6 - a4)) >> 11) - b2;
    let b4 = ((A2 * a7) >> 11) + b3 - b1;
    [
        a0 + a2 + b0,
        a1 + a3 - a2 + b2,
        a1 - a3 + a2 + b3,
        a0 - a2 - b4,
        a0 - a2 + b4,
        a1 - a3 + a2 - b3,
        a1 + a3 - a2 - b2,
        a0 + a2 - b0,
    ]
}

/// The columns' transforms, into a block of rows.
fn columns(block: &[i32; 64]) -> [i32; 64] {
    let mut out = [0; 64];
    for column in 0..8 {
        let values = if (1..8).all(|row| block[row * 8 + column] == 0) {
            [block[column]; 8]
        } else {
            transform(|row| block[row * 8 + column])
        };
        for (row, value) in values.into_iter().enumerate() {
            out[row * 8 + column] = value;
        }
    }
    out
}

/// The rows' transforms of the columns' output, rounded.
fn rows(columns: &[i32; 64], mut put: impl FnMut(usize, [i32; 8])) {
    for row in 0..8 {
        let values = transform(|x| columns[row * 8 + x]).map(|value| (value + 0x7F) >> 8);
        put(row, values);
    }
}

/// The inverse DCT in place: the block becomes the residue to add.
fn idct(block: &mut [i32; 64]) {
    let temp = columns(block);
    rows(&temp, |row, values| {
        block[row * 8..row * 8 + 8].copy_from_slice(&values)
    });
}

/// The inverse DCT written as pixels. Each value wraps to a byte, as
/// Bink's does.
fn idct_put(pixels: &mut [u8], stride: usize, block: &mut [i32; 64]) {
    let temp = columns(block);
    rows(&temp, |row, values| {
        for (x, value) in values.into_iter().enumerate() {
            pixels[row * stride + x] = value as u8;
        }
    });
}
