//! Throwaway probe of Rune 0.14.2: generators, lazy chains, instruction budget, memory limit, sandbox by absence.
use std::sync::Arc;
use std::time::Instant;

use rune::alloc::limit;
use rune::runtime::budget;
use rune::{Context, Diagnostics, Source, Sources, Vm};

fn run(label: &str, source: &str, instruction_budget: usize, memory_limit: Option<usize>) {
    let context = Context::with_default_modules().unwrap();
    let runtime = Arc::new(context.runtime().unwrap());
    let mut sources = Sources::new();
    sources.insert(Source::memory(source).unwrap()).unwrap();
    let mut diagnostics = Diagnostics::new();
    let built = rune::prepare(&mut sources)
        .with_context(&context)
        .with_diagnostics(&mut diagnostics)
        .build();
    let unit = match built {
        Ok(unit) => unit,
        Err(_) => {
            let mut text = rune::termcolor::Buffer::no_color();
            diagnostics.emit(&mut text, &sources).ok();
            let rendered = String::from_utf8_lossy(text.as_slice()).to_string();
            let first = rendered.lines().find(|line| line.contains("error")).unwrap_or("?").trim().to_string();
            println!("{label:<52} COMPILE ERROR: {first}");
            return;
        }
    };
    let mut vm = Vm::new(runtime, Arc::new(unit));
    let start = Instant::now();
    let outcome = match memory_limit {
        Some(bytes) => budget::with(instruction_budget, limit::with(bytes, || vm.call(["main"], ()))).call(),
        None => budget::with(instruction_budget, || vm.call(["main"], ())).call(),
    };
    let elapsed = start.elapsed();
    match outcome {
        Ok(value) => {
            let shown = match rune::from_value::<String>(value.clone()) {
                Ok(text) => format!("{text:?}"),
                Err(_) => format!("{value:?}"),
            };
            println!("{label:<52} OK {shown}  ({elapsed:.2?})");
        }
        Err(error) => {
            let text = format!("{error}");
            println!("{label:<52} ERR {}  ({elapsed:.2?})", text.lines().next().unwrap_or(""));
        }
    }
}

fn main() {
    let big = usize::MAX - 1;
    println!("== generators and lazy chains");
    run(
        "generator over a 16-cube, even sum, count",
        "fn cells() { for x in 0..16 { for y in 0..16 { for z in 0..16 { if (x + y + z) % 2 == 0 { yield (x, y, z); } } } } }\n\
         pub fn main() { let n = 0; for cell in cells() { n += 1; } n }",
        big,
        None,
    );
    run(
        "infinite generator, take 5 by next()",
        "fn naturals() { let i = 0; loop { yield i; i += 1; } }\n\
         pub fn main() { let g = naturals(); let out = []; while let Some(v) = g.next() { if v >= 5 { break; } out.push(v); } out.len() * 1000 + out[0] + out[4] }",
        big,
        None,
    );
    run(
        "infinite generator through .iter().take(5)",
        "fn naturals() { let i = 0; loop { yield i; i += 1; } }\n\
         pub fn main() { let v = naturals().iter().take(5).collect::<Vec>(); v.len() * 1000 + v[0] + v[4] }",
        big,
        None,
    );
    run(
        "range filter map collect",
        "pub fn main() { (0..16).iter().filter(|x| x % 2 == 0).map(|x| x * x).collect::<Vec>().len() }",
        big,
        None,
    );
    run(
        "any over range of 10^12 (lazy?)",
        "pub fn main() { (0..1_000_000_000_000).iter().any(|x| x > 2) }",
        1_000_000,
        None,
    );
    run(
        "nested flat_map cube, even sum, count",
        "pub fn main() { (0..16).iter().flat_map(|x| (0..16).iter().flat_map(move |y| (0..16).iter().map(move |z| (x, y, z)))).filter(|c| (c.0 + c.1 + c.2) % 2 == 0).count() }",
        big,
        None,
    );
    run(
        "zip / take_while / flatten (reported missing)",
        "pub fn main() { [1, 2, 3].iter().zip([4, 5, 6].iter()).count() }",
        big,
        None,
    );
    run("sum over mapped range", "pub fn main() { (0..10).iter().map(|x| x * x).sum::<i64>() }", big, None);
    run("closure captures + generator closure", "pub fn main() { let f = |n| { let out = []; for i in 0..n { out.push(i * 2); } out.len() }; f(4) }", big, None);

    println!("\n== limits");
    run("infinite loop, budget 1,000,000", "pub fn main() { loop {} }", 1_000_000, None);
    run("1M-iteration while loop, budget 100M", "pub fn main() { let i = 0; while i < 1_000_000 { i += 1; } i }", 100_000_000, None);
    run("1M-iteration while loop, budget 1M", "pub fn main() { let i = 0; while i < 1_000_000 { i += 1; } i }", 1_000_000, None);
    run("10M-iteration while loop, budget 100M (speed)", "pub fn main() { let i = 0; while i < 10_000_000 { i += 1; } i }", 100_000_000, None);
    run(
        "memory bomb push loop, 50 MB limit",
        "pub fn main() { let v = []; let i = 0; while i < 50_000_000 { v.push(i); i += 1; } v.len() }",
        200_000_000,
        Some(50_000_000),
    );
    run(
        "string growth, 50 MB limit",
        "pub fn main() { let s = \"a\"; let i = 0; while i < 40 { s = s + s; i += 1; } s.len() }",
        1_000_000,
        Some(50_000_000),
    );
    run("deep recursion", "fn f(n) { f(n + 1) }\npub fn main() { f(0) }", 10_000_000, None);

    println!("\n== sandbox by absence");
    run("std::fs::read_to_string", "pub fn main() { std::fs::read_to_string(\"C:/Windows/win.ini\") }", big, None);
    run("std::process::Command", "pub fn main() { std::process::Command::new(\"cmd\") }", big, None);
    run("std::time::Instant::now", "pub fn main() { std::time::Instant::now() }", big, None);
    run("std::env::var", "pub fn main() { std::env::var(\"PATH\") }", big, None);
    run("std::io::println (print)", "pub fn main() { println!(\"hi\"); 1 }", big, None);
    run("std::net", "pub fn main() { std::net::TcpStream::connect(\"1.1.1.1:80\") }", big, None);

    println!("\n== determinism");
    run("sqrt(2.0)", "pub fn main() { 2.0.sqrt() }", big, None);
    run("powi + powf(2.5, 3.7)", "pub fn main() { 1.0.powi(3) + 2.5.powf(3.7) }", big, None);
    run("exp(1.0) (absent in 0.14.2?)", "pub fn main() { 1.0.exp() }", big, None);
    run(
        "HashMap string keys, iteration order",
        "use std::collections::HashMap;\npub fn main() { let m = HashMap::new(); for k in [\"alpha\", \"beta\", \"gamma\", \"delta\", \"eps\"] { m.insert(k, 1); } let out = []; let s = String::new(); for (k, v) in m { s.push_str(k); s.push_str(\",\"); } s }",
        big,
        None,
    );
    run(
        "object (struct-like) key order",
        "pub fn main() { let o = #{ alpha: 1, beta: 2, gamma: 3, delta: 4 }; let out = []; let s = String::new(); for (k, v) in o { s.push_str(k); s.push_str(\",\"); } s }",
        big,
        None,
    );
}
