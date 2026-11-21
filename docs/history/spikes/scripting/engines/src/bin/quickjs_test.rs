//! Throwaway probe of rquickjs 0.14.0 (QuickJS-ng): generators, iterator helpers, interrupt granularity,
//! memory limit, sandbox by absence.
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rquickjs::{Context, Runtime};

fn run(label: &str, code: &str, memory_limit: Option<usize>, poll_limit: Option<usize>) {
    let runtime = Runtime::new().unwrap();
    if let Some(bytes) = memory_limit {
        runtime.set_memory_limit(bytes);
    }
    runtime.set_max_stack_size(512 * 1024);
    let polls = Arc::new(AtomicUsize::new(0));
    let counter = polls.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || {
        let seen = counter.fetch_add(1, Ordering::Relaxed) + 1;
        poll_limit.is_some_and(|limit| seen > limit)
    })));
    let context = Context::full(&runtime).unwrap();
    let started = Instant::now();
    let wrapped = format!("String((function() {{ {code} }})())");
    let outcome: Result<String, String> = context.with(|ctx| match ctx.eval::<String, _>(wrapped) {
        Ok(text) => Ok(text),
        Err(rquickjs::Error::Exception) => {
            let exception = ctx.catch();
            let message = exception
                .as_exception()
                .map(|e| format!("{}: {}", e.get::<_, String>("name").unwrap_or_default(), e.message().unwrap_or_default()))
                .unwrap_or_else(|| format!("{exception:?}"));
            Err(message)
        }
        Err(other) => Err(other.to_string()),
    });
    let elapsed = started.elapsed();
    let polled = polls.load(Ordering::Relaxed);
    match outcome {
        Ok(text) => println!("{label:<50} OK {text}  ({elapsed:.2?}, {polled} polls)"),
        Err(message) => println!("{label:<50} ERR {message}  ({elapsed:.2?}, {polled} polls)"),
    }
}

fn main() {
    println!("== generators and iterator helpers");
    run(
        "generator over a 16-cube, even sum, count",
        "function* cells() { for (let x = 0; x < 16; x++) for (let y = 0; y < 16; y++) for (let z = 0; z < 16; z++) if ((x + y + z) % 2 === 0) yield [x, y, z]; } let n = 0; for (const c of cells()) n++; return n;",
        None,
        None,
    );
    run("typeof Iterator, helpers present", "return [typeof Iterator, typeof Iterator.prototype.map, typeof Iterator.prototype.take, typeof Iterator.prototype.flatMap, typeof Iterator.prototype.toArray].join(',');", None, None);
    run(
        "infinite generator filter/map/take(5)",
        "function* nat() { let i = 0; while (true) yield i++; } return nat().filter(x => x % 2 === 0).map(x => x * x).take(5).toArray().join(',');",
        None,
        Some(10_000),
    );
    run(
        "any (some) over infinite generator, lazy?",
        "function* nat() { let i = 0; while (true) yield i++; } return nat().some(x => x > 2);",
        None,
        Some(10_000),
    );
    run(
        "flatMap cube with filter, count",
        "function* range(n) { for (let i = 0; i < n; i++) yield i; } return range(16).flatMap(x => range(16).flatMap(y => range(16).map(z => [x, y, z]))).filter(([x, y, z]) => (x + y + z) % 2 === 0).toArray().length;",
        None,
        None,
    );
    run("array comprehension syntax", "return [for (x of [1,2,3]) x * 2].length;", None, None);

    println!("\n== limits");
    run("while(true){} with 1,000 polls allowed", "while (true) {}", None, Some(1_000));
    run("10M-iteration loop, count polls (no limit)", "let n = 0; for (let i = 0; i < 10000000; i++) n += i; return n;", None, None);
    run("same loop again (poll count repeatable?)", "let n = 0; for (let i = 0; i < 10000000; i++) n += i; return n;", None, None);
    run("loop of 1M iterations, polls", "let n = 0; for (let i = 0; i < 1000000; i++) n += i; return n;", None, None);
    run("loop of 1M function calls, polls", "function f(x) { return x + 1; } let n = 0; for (let i = 0; i < 1000000; i++) n = f(n); return n;", None, None);
    run("memory bomb array, 50 MB limit", "const a = []; while (true) a.push(new Array(1000).fill(0)); return a.length;", Some(50_000_000), Some(1_000_000));
    run("string repeat bomb, 50 MB limit", "return 'a'.repeat(2 ** 30).length;", Some(50_000_000), None);
    run("deep recursion, 512 KB stack", "function f(n) { return f(n + 1) + 1; } return f(0);", None, None);

    println!("\n== sandbox by absence, clock, maths");
    run("typeof globals", "return [typeof require, typeof process, typeof fetch, typeof std, typeof os, typeof console, typeof setTimeout, typeof globalThis.eval, typeof Function].join(',');", None, None);
    run("Date.now (clock reachable?)", "return typeof Date.now();", None, None);
    run("Math.random unseeded x2", "return [Math.random(), Math.random()].join(',');", None, None);
    run("Math.sin(1), exp(1), pow(2.5,3.7)", "return [Math.sin(1), Math.exp(1), Math.pow(2.5, 3.7), Math.sin(1e22)].join(',');", None, None);
    run("Function constructor escape attempt", "return (function(){}).constructor('return typeof process')();", None, None);
    run("object key order (integer keys first)", "const o = {b: 1, 2: 1, a: 1, 1: 1}; return Object.keys(o).join(',');", None, None);
}
