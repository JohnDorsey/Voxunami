//! Spike S4 of scripting-spec.md: what a line, a host call, a session and a restart cost, and where a step budget should sit.
//!
//! Everything runs in process on the server's thread (no worker, which is why spike S1 was not run).
//! Run in release: `cargo run --release --bin session_costs`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Counts the bytes that are live, so that a session's memory is measured and not guessed.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

use monty::{Dump, MontyRepl, ReplProgress, Session, SessionRef, dump};
use monty_types::{
    CompileOptions, DateTimeSource, MontyObject, NameLookupResult, OsPolicy, PrintWriter, RandomSeed, RandomStart,
    ResourceLimits, ResourceTracker, SleepMode,
};

fn pinned() -> OsPolicy {
    OsPolicy {
        datetime: DateTimeSource::Fixed { unix_seconds: 1_700_000_000, microsecond: 0 },
        sleep: SleepMode::Zero,
        random_start: RandomStart::Seed(RandomSeed::Str("voxunami".to_owned())),
        ..OsPolicy::default()
    }
}

fn new_repl(limits: ResourceLimits) -> MontyRepl {
    MontyRepl::new("session.py", ResourceTracker::new(limits), CompileOptions::default()).with_os_policy(pinned())
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times[times.len() / 2]
}

fn p99(times: &[Duration]) -> Duration {
    let mut sorted = times.to_vec();
    sorted.sort();
    sorted[(sorted.len() * 99 / 100).min(sorted.len() - 1)]
}

fn micro(duration: Duration) -> String {
    format!("{:.1} us", duration.as_secs_f64() * 1e6)
}

/// Feeds one snippet that may call host functions named `host_*` (each answers with its first argument).
fn feed_with_host(repl: MontyRepl, code: &str) -> (MontyRepl, MontyObject, u32) {
    let mut calls = 0;
    let mut progress = repl.feed_start(code, vec![], PrintWriter::Disabled).map_err(|e| e.error.to_string()).unwrap();
    loop {
        match progress {
            ReplProgress::NameLookup(lookup) => {
                let host = MontyObject::function(lookup.name.clone(), None);
                progress = lookup.resume(NameLookupResult::Value(host), PrintWriter::Disabled).unwrap();
            }
            ReplProgress::FunctionCall(call) => {
                calls += 1;
                progress = call.resume(MontyObject::int(7), PrintWriter::Disabled).unwrap();
            }
            ReplProgress::Complete { repl, value } => return (repl, value, calls),
            other => panic!("unexpected {other:?}"),
        }
    }
}

fn time_feed(repl: &mut MontyRepl, code: &str, runs: usize) -> (Duration, Duration, u64) {
    let mut times = Vec::with_capacity(runs);
    let mut steps = 0;
    for _ in 0..runs {
        let before = repl.tracker().steps();
        let start = Instant::now();
        repl.feed_run(code, vec![], PrintWriter::Disabled).unwrap();
        times.push(start.elapsed());
        steps = repl.tracker().steps() - before;
    }
    (median(times.clone()), p99(&times), steps)
}

const REALISTIC_DEFINITIONS: &[&str] = &[
    "def add(a, b):\n    return a + b",
    "def clamp(x, low, high):\n    return max(low, min(high, x))",
    "def lerp(a, b, t):\n    return a + (b - a) * t",
    "def distance(p, q):\n    return sum((a - b) ** 2 for a, b in zip(p, q)) ** 0.5",
    "def ring(center, radius, count):\n    return [(center[0] + radius * i, center[1], center[2]) for i in range(count)]",
    "def fib(n):\n    return n if n < 2 else fib(n - 1) + fib(n - 2)",
    "def label(name, value):\n    return f'{name}: {value:.2f}'",
    "def histogram(values, bins):\n    counts = [0] * bins\n    for v in values:\n        counts[min(bins - 1, int(v * bins))] += 1\n    return counts",
    "class Waypoint:\n    def __init__(self, name, x, y, z):\n        self.name = name\n        self.x = x\n        self.y = y\n        self.z = z\n    def offset(self, dx, dy, dz):\n        return Waypoint(self.name, self.x + dx, self.y + dy, self.z + dz)",
    "class Route:\n    def __init__(self):\n        self.stops = []\n    def add(self, waypoint):\n        self.stops.append(waypoint)\n        return self\n    def length(self):\n        return sum(distance((a.x, a.y, a.z), (b.x, b.y, b.z)) for a, b in zip(self.stops, self.stops[1:]))",
];

fn realistic_source() -> Vec<String> {
    let mut source: Vec<String> = Vec::new();
    for round in 0..3 {
        for (index, definition) in REALISTIC_DEFINITIONS.iter().enumerate() {
            // three copies of each with distinct names: 27 definitions, about 3 KB of source
            let renamed = definition
                .replace("def add", &format!("def add_{round}"))
                .replace("def clamp", &format!("def clamp_{round}"))
                .replace("def lerp", &format!("def lerp_{round}"))
                .replace("def ring", &format!("def ring_{round}"))
                .replace("def label", &format!("def label_{round}"))
                .replace("def histogram", &format!("def histogram_{round}"))
                .replace("class Waypoint", &format!("class Waypoint_{round}"))
                .replace("class Route", &format!("class Route_{round}"));
            let _ = index;
            source.push(renamed);
        }
    }
    source.push("home = (100.5, 64.0, -20.25)".to_owned());
    source.push("settings = {'color_' + str(i): i * 3 % 7 for i in range(200)}".to_owned());
    source.push("samples = [i * 0.5 for i in range(1000)]".to_owned());
    source.push("names = ['ship_' + str(i) for i in range(100)]".to_owned());
    source
}

fn main() {
    println!("== creating a session");
    let mut times = Vec::new();
    for _ in 0..2000 {
        let start = Instant::now();
        let repl = new_repl(ResourceLimits::default());
        times.push(start.elapsed());
        std::hint::black_box(repl);
    }
    println!("new session: median {}, p99 {}", micro(median(times.clone())), micro(p99(&times)));
    let before = live();
    let empty: Vec<MontyRepl> = (0..1000).map(|_| new_repl(ResourceLimits::default())).collect();
    println!("an empty session holds {} live bytes", (live() - before) / 1000);
    drop(empty);

    println!("\n== one line, no host call (steps are bytecode instructions, plus 64 per 64 items of a native loop)");
    println!("{:<64} {:>9} {:>10} {:>10} {:>9}", "snippet", "steps", "median", "p99", "ns/step");
    let setup = "def f(n):\n    return n + 1\ndef fib(n):\n    return n if n < 2 else fib(n - 1) + fib(n - 2)\nxs = list(range(1000, 0, -1))\nwords = ['w' + str(i) for i in range(200)]";
    for code in [
        "1 + 1",
        "x = 5",
        "f(1)",
        "[f(i) for i in range(100)]",
        "sum(range(1000))",
        "[i * i for i in range(1000)]",
        "sorted(xs)",
        "{i: i * i for i in range(1000)}",
        "'-'.join(words)",
        "len(str(2 ** 1000))",
        "fib(15)",
        "fib(20)",
        "fib(25)",
        "sum(f(i) for i in range(10000))",
    ] {
        let mut repl = new_repl(ResourceLimits::default());
        repl.feed_run(setup, vec![], PrintWriter::Disabled).unwrap();
        let (median, p99, steps) = time_feed(&mut repl, code, if code.contains("fib(25)") { 10 } else { 300 });
        let per_step = median.as_secs_f64() * 1e9 / steps.max(1) as f64;
        println!("{code:<64} {steps:>9} {:>10} {:>10} {per_step:>9.1}", micro(median), micro(p99));
    }

    println!("\n== a host call (the name looked up, the call suspended, resumed with a value)");
    let mut times = Vec::new();
    let mut repl = new_repl(ResourceLimits::default());
    for _ in 0..2000 {
        let start = Instant::now();
        let (next, value, calls) = feed_with_host(repl, "host_ping(1)");
        times.push(start.elapsed());
        assert_eq!(calls, 1);
        assert_eq!(value.py_repr().to_string(), "7");
        repl = next;
    }
    println!("host_ping(1) as a whole line: median {}, p99 {}", micro(median(times.clone())), micro(p99(&times)));
    let mut times = Vec::new();
    for _ in 0..500 {
        let start = Instant::now();
        let (next, _, calls) = feed_with_host(repl, "[host_ping(i) for i in range(100)]");
        times.push(start.elapsed());
        assert_eq!(calls, 100);
        repl = next;
    }
    println!("100 host calls in one line: median {} ({} per call)", micro(median(times.clone())), micro(median(times) / 100));

    println!("\n== a step budget: the same cut-off every time");
    for limit in [10_000u64, 100_000, 1_000_000] {
        let mut outcomes = Vec::new();
        for _ in 0..3 {
            let mut repl = new_repl(ResourceLimits::default().max_feed_steps(limit));
            let start = Instant::now();
            let result = repl.feed_run("n = 0\nwhile True:\n    n += 1", vec![], PrintWriter::Disabled);
            let message = result.err().map(|e| e.to_string().lines().last().unwrap_or("").to_owned()).unwrap_or_default();
            let n = repl.feed_run("n", vec![], PrintWriter::Disabled).map(|v| v.py_repr().to_string()).unwrap_or_default();
            outcomes.push((message, n, start.elapsed()));
        }
        let same = outcomes.iter().all(|o| o.0 == outcomes[0].0 && o.1 == outcomes[0].1);
        println!(
            "max_feed_steps {limit:>9}: {} | n at the cut-off = {} | same in 3 runs: {same} | {} ({} per 1000 steps)",
            outcomes[0].0,
            outcomes[0].1,
            micro(outcomes[0].2),
            micro(outcomes[0].2 / (limit as u32 / 1000)),
        );
    }

    println!("\n== a realistic session: 27 definitions, a dictionary of 200, 1,000 numbers, 100 names");
    let source = realistic_source();
    let source_bytes: usize = source.iter().map(|s| s.len()).sum();
    let mut replay_times = Vec::new();
    let mut session = new_repl(ResourceLimits::default());
    for run in 0..50 {
        let start = Instant::now();
        let mut repl = new_repl(ResourceLimits::default());
        for snippet in &source {
            repl.feed_run(snippet, vec![], PrintWriter::Disabled).unwrap();
        }
        replay_times.push(start.elapsed());
        if run == 0 {
            session = repl;
        }
    }
    println!("source: {} snippets, {source_bytes} bytes; replaying it from nothing: median {}, p99 {}", source.len(), micro(median(replay_times.clone())), micro(p99(&replay_times)));

    let bytes = dump("session.py", None, &[], SessionRef::Idle(&session)).unwrap();
    let mut dump_times = Vec::new();
    for _ in 0..200 {
        let start = Instant::now();
        std::hint::black_box(dump("session.py", None, &[], SessionRef::Idle(&session)).unwrap());
        dump_times.push(start.elapsed());
    }
    let mut load_times = Vec::new();
    for _ in 0..200 {
        let start = Instant::now();
        std::hint::black_box(Dump::load(&bytes).unwrap());
        load_times.push(start.elapsed());
    }
    println!("dump: {} bytes ({:.1}x the source); dump median {}; load median {}", bytes.len(), bytes.len() as f64 / source_bytes as f64, micro(median(dump_times)), micro(median(load_times)));

    let Session::Idle(mut restored) = Dump::load(&bytes).unwrap().state else { panic!("not idle") };
    restored.set_os_policy(pinned());
    let check = "(fib(10), add_1(2, 3), len(settings), sum(samples), Route_2().add_2(Waypoint_2('a', 0, 0, 0)).add_2(Waypoint_2('b', 3, 4, 0)).length(), label_0('x', 2.5))";
    let live_says = session.feed_run(check, vec![], PrintWriter::Disabled).unwrap().py_repr().to_string();
    let after = restored.feed_run(check, vec![], PrintWriter::Disabled).unwrap().py_repr().to_string();
    println!("live session says     {live_says}\nrestored session says {after}\nthe same: {}", live_says == after);

    println!("\n== many idle sessions (one per player who has scripts)");
    for count in [100usize, 1000] {
        let before = live();
        let start = Instant::now();
        let mut sessions = Vec::with_capacity(count);
        for _ in 0..count {
            let mut repl = new_repl(ResourceLimits::default());
            for snippet in &source {
                repl.feed_run(snippet, vec![], PrintWriter::Disabled).unwrap();
            }
            sessions.push(repl);
        }
        let bytes_each = (live() - before) / count;
        println!("{count} sessions built by replaying the source: {:.2} s in all, {} per session, {bytes_each} live bytes per session", start.elapsed().as_secs_f64(), micro(start.elapsed() / count as u32));
        let start = Instant::now();
        let restored: Vec<_> = (0..count).map(|_| Dump::load(&bytes).unwrap()).collect();
        println!("{count} sessions restored from a dump: {:.3} s in all, {} per session", start.elapsed().as_secs_f64(), micro(start.elapsed() / count as u32));
        std::hint::black_box((sessions, restored));
    }
}
