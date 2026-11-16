//! Scratch probe (not committed): at which step does the allocator-based `max_memory` stop a runaway script,
//! and how much does that move with things that should not matter?

use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use monty::MontyRepl;
use monty_types::{CompileOptions, LIVE_MEMORY, PrintWriter, ResourceLimits, ResourceTracker, memory_limit_with_headroom};

#[global_allocator]
static ALLOC: monty_alloc::LimitedAllocator = monty_alloc::LimitedAllocator;

const LIMIT: usize = 1_000_000;

const SCRIPTS: [(&str, &str); 4] = [
    ("small lists", "x = []\nwhile True:\n    x.append([0])\n"),
    ("short strings", "x = []\ni = 0\nwhile True:\n    x.append('k%d' % i)\n    i += 1\n"),
    ("dict of ints", "d = {}\ni = 0\nwhile True:\n    d[i] = i\n    i += 1\n"),
    ("one big list", "x = [0] * 200_000\ny = [0] * 200_000\n"),
];

/// Runs `code` under `max_memory = LIMIT`; returns the step count when it stopped and the error line.
fn trip(code: &str) -> (u64, String) {
    monty_alloc::set_hard_limit(memory_limit_with_headroom(Some(LIMIT), false)).unwrap();
    let limits = ResourceLimits::default()
        .max_memory(LIMIT)
        .max_feed_duration(Duration::from_secs(20));
    let mut repl = MontyRepl::new("probe.py", ResourceTracker::new(limits), CompileOptions::default());
    let result = repl.feed_run(code, vec![], PrintWriter::Disabled);
    let steps = repl.tracker().steps();
    let line = match result {
        Ok(_) => "completed".to_owned(),
        Err(error) => error.to_string().lines().last().unwrap_or("").to_owned(),
    };
    (steps, line)
}

/// How well the model tracks the allocator: the live bytes a session's heap really holds, against what the
/// model says, for sessions built by `code`.
fn calibrate() {
    let cases: [(&str, &str); 12] = [
        ("10k small lists", "x = [[0] for i in range(10_000)]"),
        ("10k long strings", "x = [str(i) * 8 + 'abcdefghijklmnopqrstuvwxyz' for i in range(10_000)]"),
        ("dict of 10k ints", "x = {i: i for i in range(10_000)}"),
        ("list of 100k ints", "x = list(range(100_000))"),
        ("list grown by 100k appends", "x = []
for i in range(100_000):
    x.append(i)"),
        ("dict grown by 10k inserts", "x = {}
for i in range(10_000):
    x[i] = i"),
        ("1k instances, 3 attributes", "class C:
    def __init__(self, a):
        self.a = a
        self.b = a
        self.c = a
x = [C(i) for i in range(1000)]"),
        ("1k big integers", "n = 3 ** 1000
x = [n + i for i in range(1000)]"),
        ("10k tuples of 3", "x = [(i, i, i) for i in range(10_000)]"),
        ("set of 10k ints", "x = set(range(10_000))"),
        ("1k lists of 100 strings", "x = [[str(j) * 5 for j in range(100)] for i in range(1000)]"),
        ("one 1 MB string", "x = 'a' * 1_000_000"),
    ];
    println!("{:<30} {:>12} {:>12} {:>7}", "session", "real bytes", "model bytes", "model/real");
    for (name, code) in cases {
        let before = LIVE_MEMORY.load(Ordering::Relaxed);
        let mut repl = MontyRepl::new("probe.py", ResourceTracker::default(), CompileOptions::default());
        repl.feed_run(code, vec![], PrintWriter::Disabled).unwrap();
        let after = LIVE_MEMORY.load(Ordering::Relaxed);
        let real = after - before;
        let model = repl.model_bytes();
        println!("{name:<30} {real:>12} {model:>12} {:>7.2}", model as f64 / real as f64);
    }
}

fn main() {
    if std::env::args().any(|argument| argument == "--calibrate") {
        calibrate();
        return;
    }
    // Noise: another part of the process allocating and freeing, as a game would.
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let noise_stop = std::sync::Arc::clone(&stop);
    let noise = thread::spawn(move || {
        let mut seed = 12345_u64;
        let mut held: Vec<Vec<u8>> = Vec::new();
        while !noise_stop.load(Ordering::Relaxed) {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let size = 20_000 + (seed >> 40) as usize % 150_000;
            held.push(vec![0_u8; size]);
            if held.len() > 3 {
                held.remove(0);
            }
            thread::sleep(Duration::from_micros(200));
        }
    });

    let noise_enabled = std::env::args().any(|argument| argument == "--noise");
    let leak = std::env::args().any(|argument| argument == "--leak");
    if !noise_enabled {
        stop.store(true, Ordering::Relaxed);
    }
    if leak {
        // A host structure that grew after the worker was armed.
        monty_alloc::set_hard_limit(None).unwrap();
        std::mem::forget(vec![1_u8; 300_000]);
    }
    for (name, code) in SCRIPTS {
        let mut line = format!("{name:<14}");
        for _ in 0..4 {
            let (steps, message) = trip(code);
            line.push_str(&format!(" {steps:>8}"));
            if message == "completed" {
                line.push('*');
            }
        }
        println!("{line}   {}", trip(code).1);
    }
    stop.store(true, Ordering::Relaxed);
    noise.join().unwrap();
}
