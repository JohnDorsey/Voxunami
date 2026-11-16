//! In-process embedding probe for the Voxunami Monty fork (scratch program, not part of the game).
use std::time::{Duration, Instant};

use monty::{MontyRun, RunProgress};
use monty_types::{
    CompileOptions, DateTimeSource, MontyObject, OsPolicy, PrintWriter, RandomSeed, RandomStart, ResourceLimits,
    ResourceTracker, SleepMode,
};

fn policy() -> OsPolicy {
    OsPolicy {
        datetime: DateTimeSource::Fixed { unix_seconds: 1_700_000_000, microsecond: 0 },
        sleep: SleepMode::Zero,
        random_start: RandomStart::Seed(RandomSeed::Str("voxunami".to_owned())),
        ..OsPolicy::default()
    }
}

fn try_runner(code: &str, inputs: &[&str]) -> Result<MontyRun, monty_types::MontyException> {
    MontyRun::new(code.to_owned(), "probe.py", inputs.iter().map(|s| (*s).to_owned()).collect(), CompileOptions::default())
        .map(|run| run.with_os_policy(policy()))
}

fn runner(code: &str, inputs: &[&str]) -> MontyRun {
    try_runner(code, inputs).unwrap()
}

fn show(label: &str, result: &Result<MontyObject, monty_types::MontyException>, elapsed: Duration) {
    match result {
        Ok(value) => println!("{label:<44} ok    {:>9.3?}  {}", elapsed, value.py_repr()),
        Err(error) => {
            let text = error.to_string();
            let first = text.lines().last().unwrap_or("");
            println!("{label:<44} ERROR {:>9.3?}  {}", elapsed, first);
        }
    }
}

fn run_limited(label: &str, code: &str, limits: ResourceLimits) {
    let start = Instant::now();
    let mut run = match try_runner(code, &[]) {
        Ok(run) => run,
        Err(error) => return show(label, &Err(error), start.elapsed()),
    };
    let result = run.run(vec![], ResourceTracker::new(limits), PrintWriter::Disabled);
    show(label, &result, start.elapsed());
}

/// A pipeline-setup-shaped script: 60 textures, 120 passes, 500 settings, through two host functions.
const PIPELINE: &str = "
settings = {}
for i in range(500):
    settings['option_%d' % i] = (i * 7) % 13
for i in range(60):
    texture('tex_%d' % i, 1920 >> (i % 3), 1080 >> (i % 3), 'rgba16f' if i % 2 else 'rgba8')
for i in range(120):
    render_pass('pass_%d' % i, 'shaders/pass_%d.glsl' % i, ['tex_%d' % (i % 60)], ['tex_%d' % ((i + 1) % 60)], settings['option_%d' % (i % 500)])
len(settings)
";

fn run_pipeline() -> (usize, Duration) {
    let start = Instant::now();
    let run = runner(PIPELINE, &["texture", "render_pass"]);
    let functions = vec![MontyObject::function("texture".to_owned(), None), MontyObject::function("render_pass".to_owned(), None)];
    let mut progress = run
        .start(functions, ResourceTracker::new(ResourceLimits::default().max_feed_duration(Duration::from_secs(1))), PrintWriter::Disabled)
        .unwrap();
    let mut calls = 0;
    loop {
        match progress {
            RunProgress::FunctionCall(call) => {
                calls += 1;
                progress = call.resume(MontyObject::none(), PrintWriter::Disabled).unwrap();
            }
            RunProgress::Complete(_) => return (calls, start.elapsed()),
            other => panic!("unexpected progress: {other:?}"),
        }
    }
}

fn main() {
    println!("== pipeline setup, in process, 60 textures + 120 passes + 500 settings");
    let (calls, first) = run_pipeline();
    let mut times: Vec<Duration> = (0..200).map(|_| run_pipeline().1).collect();
    times.sort();
    println!("host calls per run: {calls}; first run {first:.3?}; 200 runs: median {:.3?}, p99 {:.3?}", times[100], times[198]);

    println!("\n== limits");
    run_limited("while True: pass, 50 ms feed limit", "while True:\n    pass", ResourceLimits::default().max_feed_duration(Duration::from_millis(50)));
    run_limited("same, no limit set -> skipped (would hang)", "1", ResourceLimits::default());
    run_limited("sum(range(10**9)), 50 ms feed limit", "sum(range(10**9))", ResourceLimits::default().max_feed_duration(Duration::from_millis(50)));
    run_limited("[0]*10**8, max_memory 50 MB (no monty-alloc)", "len([0] * (10**8))", ResourceLimits { max_memory: Some(50_000_000), ..ResourceLimits::default() });
    run_limited("unbounded recursion, default depth 1000", "def f(n):\n    return f(n + 1)\ntry:\n    f(0)\nexcept RecursionError:\n    r = 'caught'\nr", ResourceLimits::default());
    run_limited("genexpr over range(10**12), 200 ms + 50 MB", "any(x == -1 for x in range(10**12))", ResourceLimits { max_feed_duration: Some(Duration::from_millis(200)), max_memory: Some(50_000_000), ..ResourceLimits::default() });
    run_limited("generator function (yield)", "def g():\n    yield 1\nlist(g())", ResourceLimits::default());
    run_limited("append forever, 300 ms + 50 MB", "x = []
while True:
    x.append(0)", ResourceLimits { max_feed_duration: Some(Duration::from_millis(300)), max_memory: Some(50_000_000), ..ResourceLimits::default() });
    run_limited("string doubling x40, 300 ms + 50 MB", "s = 'ab'
for i in range(40):
    s = s + s
len(s)", ResourceLimits { max_feed_duration: Some(Duration::from_millis(300)), max_memory: Some(50_000_000), ..ResourceLimits::default() });
    run_limited("sum(x*x for x in range(10))", "sum(x*x for x in range(10))", ResourceLimits::default());

    println!("\n== frozen clock and seed (two fresh runs must agree)");
    for run_index in 0..2 {
        let mut run = runner("import time, datetime, random\n(time.time(), str(datetime.datetime.now()), random.random(), random.random())", &[]);
        let start = Instant::now();
        let result = run.run(vec![], ResourceTracker::default(), PrintWriter::Disabled);
        show(&format!("run {run_index}"), &result, start.elapsed());
    }

    println!("\n== ambient authority");
    for (label, code) in [
        ("open('/etc/passwd')", "open('/etc/passwd').read()"),
        ("import os; os.getenv('PATH')", "import os\nos.getenv('PATH')"),
        ("import socket", "import socket"),
        ("import subprocess", "import subprocess"),
        ("__import__('os').system", "__import__('os').system('echo hi')"),
        ("eval('1+1')", "eval('1+1')"),
    ] {
        let run = runner(code, &[]);
        let start = Instant::now();
        let mut progress = run.start(vec![], ResourceTracker::default(), PrintWriter::Disabled);
        let result = loop {
            match progress {
                Ok(RunProgress::OsCall(call)) => {
                    let answer = call.function_call.no_handler_result();
                    progress = call.resume(answer, PrintWriter::Disabled);
                }
                Ok(RunProgress::Complete(value)) => break Ok(value),
                Ok(other) => break Ok(MontyObject::string(format!("suspended: {other:?}").chars().take(60).collect::<String>())),
                Err(error) => break Err(error),
            }
        };
        show(label, &result, start.elapsed());
    }
}
