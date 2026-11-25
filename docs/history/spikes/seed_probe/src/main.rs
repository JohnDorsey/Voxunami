//! A throwaway probe for docs/history/seed-secrecy-survey.md. Not part of the repository; the numbers it prints are the ones the survey quotes.
//!
//! Usage: `seed_probe SECTION...` with sections `inversion`, `java`, `derive`, `palette`, and `brute BITS [BITS...]` (the last one is the slow one).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use terrain::{
    Dimensionality, FieldTerrainGenerator, FractalNoise, Heightfield, Minimum, ShellCaves,
    SurfaceLayers, TerrainGenerator, lattice_hash, lattice_value, mix,
};
use voxel_grid::{ChannelDefinition, ChannelStorage, GridLayout, StorageKind};
use voxel_space::BrickCoordinate;

// ---------------------------------------------------------------------------------------------------------------------------------
// The engine's `mix` (splitmix64's output function) and its exact inverse.
// ---------------------------------------------------------------------------------------------------------------------------------

const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const MULTIPLIER_ONE: u64 = 0xBF58_476D_1CE4_E5B9;
const MULTIPLIER_TWO: u64 = 0x94D0_49BB_1331_11EB;

/// The inverse of an odd number modulo 2^64 (Newton's iteration doubles the number of correct bits at each step).
fn inverse_of_odd(value: u64) -> u64 {
    let mut inverse = value; // an odd number times itself is 1 modulo 8: three correct bits
    for _ in 0..5 {
        inverse = inverse.wrapping_mul(2u64.wrapping_sub(value.wrapping_mul(inverse)));
    }
    inverse
}

/// Undoes `y = x ^ (x >> shift)`.
fn undo_xor_shift(value: u64, shift: u32) -> u64 {
    let mut x = value;
    for _ in 0..(64 / shift + 1) {
        x = value ^ (x >> shift);
    }
    x
}

/// The exact inverse of `terrain::mix`.
fn unmix(output: u64) -> u64 {
    let mut z = undo_xor_shift(output, 31);
    z = z.wrapping_mul(inverse_of_odd(MULTIPLIER_TWO));
    z = undo_xor_shift(z, 27);
    z = z.wrapping_mul(inverse_of_odd(MULTIPLIER_ONE));
    z = undo_xor_shift(z, 30);
    z.wrapping_sub(GOLDEN_GAMMA)
}

/// A small generator for the probe's own random inputs (xorshift64*; it is not what is being tested).
struct Driver(u64);

impl Driver {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn nanoseconds_each(start: Instant, count: u64) -> f64 {
    start.elapsed().as_nanos() as f64 / count as f64
}

fn inversion() {
    println!("== 1. The engine's seed mixer is a bijection, so it can be run backwards ==");
    println!(
        "terrain::mix(0) = {:#018X} (the published first output of splitmix64 from 0 is 0xE220A8397B1DCDAF)",
        mix(0)
    );
    let mut driver = Driver(0x1234_5678_9ABC_DEF1);
    let mut checked = 0u64;
    for _ in 0..10_000_000 {
        let value = driver.next();
        assert_eq!(unmix(mix(value)), value);
        assert_eq!(mix(unmix(value)), value);
        checked += 1;
    }
    println!("unmix(mix(x)) == x and mix(unmix(x)) == x for {checked} random x");

    // A parent seed and a child derived the way the engine hashes everything else: a lattice hash of the parent at the child's number.
    println!("-- a child seed derived with the engine's lattice hash gives its parent away --");
    let parent = driver.next();
    let child_number = 7i128;
    let child = lattice_hash(parent, child_number, 0, 0);
    let recover = |child: u64, number: u64| -> u64 {
        let h2 = unmix(child); // z = 0
        let h1 = unmix(h2); // y = 0
        let h0 = unmix(h1) ^ number; // x = number
        unmix(h0)
    };
    assert_eq!(recover(child, 7), parent);
    let start = Instant::now();
    let mut sink = 0u64;
    let repeats = 5_000_000u64;
    for round in 0..repeats {
        // (A different input each time, so that the work cannot be done once and reused.)
        sink ^= recover(std::hint::black_box(child.wrapping_add(round)), 7);
    }
    println!(
        "parent {parent:#018X}, child {child:#018X}, recovered from the child and its number: {:#018X} ({:.1} ns each, {})",
        recover(child, 7),
        nanoseconds_each(start, repeats),
        sink & 1
    );

    // A property computed from a material seed with the engine's own value noise (`lattice_value`: 53 hash bits as a float in [-1, 1)).
    println!(
        "-- one full-precision property computed with the engine's noise gives the seed away --"
    );
    let material_seed = driver.next();
    let first_property = lattice_value(material_seed, 1, 0, 0);
    let second_property = lattice_value(material_seed, 2, 0, 0);
    let top_bits = ((first_property + 1.0) * 4_503_599_627_370_496.0) as u64; // the hash shifted right by 11, exactly
    let start = Instant::now();
    let mut found = None;
    let mut tried = 0u64;
    for low in 0..2048u64 {
        let hash = (top_bits << 11) | low;
        let h2 = unmix(hash); // z = 0
        let h1 = unmix(h2); // y = 0
        let h0 = unmix(h1) ^ 1; // x = 1
        let seed = unmix(h0);
        tried += 1;
        if lattice_value(seed, 1, 0, 0) == first_property
            && lattice_value(seed, 2, 0, 0) == second_property
        {
            found = Some(seed);
            break;
        }
    }
    println!(
        "material seed {material_seed:#018X}; from the first property alone ({first_property:+.15}) and a check against the second, recovered {:?} after {tried} of 2048 candidates in {:.1} us",
        found.map(|seed| format!("{seed:#018X}")),
        start.elapsed().as_nanos() as f64 / 1000.0
    );
    assert_eq!(found, Some(material_seed));
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Java's Random (the 48-bit generator behind Minecraft's world generation) and what three of its floats give away.
// ---------------------------------------------------------------------------------------------------------------------------------

const JAVA_MULTIPLIER: u64 = 0x5_DEEC_E66D;
const JAVA_ADDEND: u64 = 0xB;
const JAVA_MASK: u64 = (1 << 48) - 1;

struct JavaRandom {
    state: u64,
}

impl JavaRandom {
    fn new(seed: u64) -> Self {
        Self {
            state: (seed ^ JAVA_MULTIPLIER) & JAVA_MASK,
        }
    }

    fn next(&mut self, bits: u32) -> u32 {
        self.state = self
            .state
            .wrapping_mul(JAVA_MULTIPLIER)
            .wrapping_add(JAVA_ADDEND)
            & JAVA_MASK;
        (self.state >> (48 - bits)) as u32
    }

    /// `nextFloat()`: `next(24) / 2^24`, which a float holds exactly.
    fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / 16_777_216.0
    }
}

fn java() {
    println!("== 2. Three floats of Java's Random give its whole state ==");
    let mut driver = Driver(0x0FED_CBA9_8765_4321);
    let world_seed = driver.next();
    let mut random = JavaRandom::new(world_seed);
    // (Warm it up, as a game that has been running would have.)
    for _ in 0..1000 {
        random.next_float();
    }
    let state_before = random.state;
    let floats = [
        random.next_float(),
        random.next_float(),
        random.next_float(),
    ];
    let numerators = floats.map(|float| (float * 16_777_216.0) as u64);
    let start = Instant::now();
    let mut recovered_state = None;
    for low in 0..(1u64 << 24) {
        let first = (numerators[0] << 24) | low;
        let second = first
            .wrapping_mul(JAVA_MULTIPLIER)
            .wrapping_add(JAVA_ADDEND)
            & JAVA_MASK;
        if second >> 24 != numerators[1] {
            continue;
        }
        let third = second
            .wrapping_mul(JAVA_MULTIPLIER)
            .wrapping_add(JAVA_ADDEND)
            & JAVA_MASK;
        if third >> 24 == numerators[2] {
            recovered_state = Some(first);
            break;
        }
    }
    let elapsed = start.elapsed();
    let first_state = recovered_state.expect("three floats pin the 48-bit state");
    // Step it back to the state before the first of the three floats, with the inverse of the multiplier.
    let inverse = inverse_of_odd(JAVA_MULTIPLIER);
    let stepped_back = first_state.wrapping_sub(JAVA_ADDEND).wrapping_mul(inverse) & JAVA_MASK;
    println!(
        "3 observed floats {floats:?}: the 48-bit state found by trying at most 2^24 low parts in {:.1} ms; stepping it back one place gives the state before them: {} (it is {state_before:#014X})",
        elapsed.as_secs_f64() * 1000.0,
        if stepped_back == state_before {
            "equal"
        } else {
            "NOT equal"
        }
    );
    assert_eq!(stepped_back, state_before);
    // And stepping back many places is as easy as forward.
    let mut earlier = stepped_back;
    for _ in 0..5000 {
        earlier = earlier.wrapping_sub(JAVA_ADDEND).wrapping_mul(inverse) & JAVA_MASK;
    }
    let mut forward = JavaRandom { state: earlier };
    for _ in 0..5000 {
        forward.next(32);
    }
    assert_eq!(forward.state, stepped_back);
    println!("5,000 steps back and 5,000 forward land on the same state: yes");
}

// ---------------------------------------------------------------------------------------------------------------------------------
// What a cryptographic derivation costs, and what searching a small secret costs even through it.
// ---------------------------------------------------------------------------------------------------------------------------------

fn derive() {
    println!("== 3a. What deriving a seed with BLAKE3 costs ==");
    let root = [0x5Au8; 32];
    let context = "voxunami 2026-11-25 planet seed";
    let count = 10_000_000u64;
    let start = Instant::now();
    let mut sink = 0u8;
    for planet in 0..count {
        let mut material = [0u8; 40];
        material[..32].copy_from_slice(&root);
        material[32..].copy_from_slice(&planet.to_le_bytes());
        sink ^= blake3::derive_key(context, &material)[0];
    }
    println!(
        "blake3::derive_key of a 40-byte input under a context string: {:.1} ns each ({} calls; {})",
        nanoseconds_each(start, count),
        count,
        sink & 1
    );
    let key = [0x33u8; 32];
    let start = Instant::now();
    for planet in 0..count {
        sink ^= blake3::keyed_hash(&key, &planet.to_le_bytes()).as_bytes()[0];
    }
    println!(
        "blake3::keyed_hash of an 8-byte input: {:.1} ns each ({})",
        nanoseconds_each(start, count),
        sink & 1
    );
}

fn key_of(candidate: u64) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[..8].copy_from_slice(&candidate.to_le_bytes());
    key
}

fn tag_of(key: &[u8; 32]) -> u64 {
    let hash = blake3::keyed_hash(key, b"material density 1");
    u64::from_le_bytes(hash.as_bytes()[..8].try_into().expect("eight bytes"))
}

/// Searches the whole space of `bits`-bit secrets for the one that gives `target`, on `threads` threads, and says how long that took (the secret is the last one
/// searched, so the whole space is covered).
fn brute_force(bits: u32, threads: u64) -> (u64, f64) {
    let space = 1u64 << bits;
    let secret = space - 1;
    let target = tag_of(&key_of(secret));
    let found = AtomicU64::new(u64::MAX);
    let start = Instant::now();
    std::thread::scope(|scope| {
        for thread in 0..threads {
            let found = &found;
            scope.spawn(move || {
                let mut candidate = thread;
                let mut counter = 0u32;
                while candidate < space {
                    if tag_of(&key_of(candidate)) == target {
                        found.store(candidate, Ordering::Relaxed);
                        return;
                    }
                    candidate += threads;
                    counter += 1;
                    if counter & 0xFFFF == 0 && found.load(Ordering::Relaxed) != u64::MAX {
                        return;
                    }
                }
            });
        }
    });
    let seconds = start.elapsed().as_secs_f64();
    assert_eq!(found.load(Ordering::Relaxed), secret);
    (space, seconds)
}

fn human(seconds: f64) -> String {
    const MINUTE: f64 = 60.0;
    const HOUR: f64 = 3600.0;
    const DAY: f64 = 86_400.0;
    const YEAR: f64 = 31_557_600.0;
    if seconds < 1.0 {
        format!("{:.0} ms", seconds * 1000.0)
    } else if seconds < MINUTE {
        format!("{seconds:.1} s")
    } else if seconds < HOUR {
        format!("{:.1} minutes", seconds / MINUTE)
    } else if seconds < DAY {
        format!("{:.1} hours", seconds / HOUR)
    } else if seconds < YEAR {
        format!("{:.1} days", seconds / DAY)
    } else {
        format!("{:.3e} years", seconds / YEAR)
    }
}

fn brute(bits_list: &[u32]) {
    println!(
        "== 3b. Searching a small secret through a keyed BLAKE3 hash, whole space, all threads =="
    );
    let threads = std::thread::available_parallelism().map_or(1, |count| count.get() as u64);
    let mut rate = 0.0;
    for &bits in bits_list {
        let (space, seconds) = brute_force(bits, threads);
        rate = space as f64 / seconds;
        println!(
            "{bits} bits: {space} candidates in {seconds:.2} s on {threads} threads = {:.1} million a second",
            rate / 1e6
        );
    }
    if rate > 0.0 {
        println!(
            "-- the last rate, {:.0} million a second, extended (one hash per candidate; real checks cost more, so these are the cheapest an attacker can have) --",
            rate / 1e6
        );
        // 22.1 GH/s is hashcat v7.0.0's figure for SHA2-256 on one RTX 4090 (a published benchmark, not measured here; BLAKE3 would differ).
        let gpu = 22.1e9;
        for bits in [32u32, 40, 48, 56, 64, 80, 96, 128] {
            let candidates = 2f64.powi(bits as i32);
            println!(
                "2^{bits:<3} this machine: {:>16}   one RTX 4090 at 22.1 GH/s: {:>16}   a thousand of them: {:>16}",
                human(candidates / rate),
                human(candidates / gpu),
                human(candidates / gpu / 1000.0)
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// The real bricks of Cave's rolling hills, and what a palette saves.
// ---------------------------------------------------------------------------------------------------------------------------------

fn palette() {
    println!(
        "== 4. The block channel of real bricks (Cave's rolling hills, 12 x 12 x 8 bricks round the origin, seed 5) =="
    );
    let layout = GridLayout::new(vec![ChannelDefinition::number(
        "probe:material".parse().expect("valid identifier"),
        0,
    )])
    .expect("one channel");
    let material = layout
        .channel_index(&"probe:material".parse().expect("valid identifier"))
        .expect("channel exists");
    let seed = 5u64;
    let ground = Heightfield::new(
        64.0,
        FractalNoise::standard(seed, Dimensionality::Planar, 7, 6, 40.0, 0.5),
    );
    let caves = ShellCaves::new(
        FractalNoise::standard(
            seed ^ 0xCA7E_5CA7_E5CA_7E5C,
            Dimensionality::Volumetric,
            5,
            2,
            1.0,
            0.5,
        ),
        0.08,
        16.0,
    );
    let generator = FieldTerrainGenerator::new(
        Arc::new(Minimum {
            first: ground,
            second: caves,
        }),
        material,
        SurfaceLayers {
            air: 0,
            surface: 3,
            subsurface: 2,
            deep: 1,
        },
    );
    let mut by_kind: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let (mut bricks, mut heap_total, mut encoded_total) = (0usize, 0usize, 0usize);
    for z in -6i128..6 {
        for x in -6i128..6 {
            for y in -2i128..6 {
                let brick = generator.generate_brick(&layout, BrickCoordinate::new(x, y, z));
                let mut compact = brick.channel(material).clone();
                compact.compact();
                let name = match compact.kind() {
                    StorageKind::Uniform => "uniform".to_owned(),
                    StorageKind::Paletted {
                        palette_length,
                        bits_per_index,
                    } => format!("paletted, {palette_length} values, {bits_per_index}-bit indices"),
                    StorageKind::Dense => "dense".to_owned(),
                };
                let mut bytes = Vec::new();
                compact.encode(&mut bytes);
                let entry = by_kind.entry(name).or_default();
                entry.0 += 1;
                entry.1 += compact.heap_size_bytes();
                entry.2 += bytes.len();
                bricks += 1;
                heap_total += compact.heap_size_bytes();
                encoded_total += bytes.len();
            }
        }
    }
    for (name, (count, heap, encoded)) in &by_kind {
        println!(
            "{name:<44} {count:>5} bricks, {:>9.0} bytes in memory and {:>9.0} encoded, each on average",
            *heap as f64 / *count as f64,
            *encoded as f64 / *count as f64
        );
    }
    println!(
        "{bricks} bricks: {heap_total} bytes in memory ({:.0} each), {encoded_total} encoded ({:.0} each); with no palette: {} bytes with a u16 for each voxel, {} with a u32",
        heap_total as f64 / bricks as f64,
        encoded_total as f64 / bricks as f64,
        bricks * 65_536,
        bricks * 131_072
    );

    println!(
        "-- what a brick costs by how many different values it holds (constructed, the values spread over the brick) --"
    );
    for distinct in [1usize, 2, 3, 4, 5, 16, 17, 255, 256, 257, 1000] {
        let values: Vec<u32> = (0..32_768)
            .map(|index| 1000 + (index % distinct) as u32 * 7)
            .collect();
        let storage = ChannelStorage::from_values(&values);
        let mut bytes = Vec::new();
        storage.encode(&mut bytes);
        println!(
            "{distinct:>5} distinct: {:<44} {:>7} bytes in memory, {:>7} encoded",
            match storage.kind() {
                StorageKind::Uniform => "uniform".to_owned(),
                StorageKind::Paletted {
                    palette_length,
                    bits_per_index,
                } => format!("paletted, {palette_length} values, {bits_per_index}-bit indices"),
                StorageKind::Dense => "dense".to_owned(),
            },
            storage.heap_size_bytes(),
            bytes.len()
        );
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mut position = 0;
    while position < arguments.len() {
        match arguments[position].as_str() {
            "inversion" => inversion(),
            "java" => java(),
            "derive" => derive(),
            "palette" => palette(),
            "brute" => {
                let bits: Vec<u32> = arguments[position + 1..]
                    .iter()
                    .map_while(|argument| argument.parse().ok())
                    .collect();
                position += bits.len();
                brute(&bits);
            }
            other => panic!("unknown section {other}"),
        }
        position += 1;
    }
}
