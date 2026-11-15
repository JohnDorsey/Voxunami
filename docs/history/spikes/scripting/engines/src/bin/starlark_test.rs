//! Throwaway probe of the `starlark` crate 0.14.2: comprehensions, laziness, tick and heap limits, sandbox by absence.
use std::time::Instant;

use starlark::environment::{Globals, Module};
use starlark::eval::Evaluator;
use starlark::syntax::{AstModule, Dialect};

fn run(label: &str, code: &str, tick_limit: Option<u64>, heap_limit: Option<usize>) {
    let label_owned = label.to_string();
    let caught = std::panic::catch_unwind(|| run_inner(label, code, tick_limit, heap_limit));
    if caught.is_err() {
        println!("{label_owned:<52} PANIC in the host (Rust panic inside starlark)");
    }
}

fn run_inner(label: &str, code: &str, tick_limit: Option<u64>, heap_limit: Option<usize>) {
    let started = Instant::now();
    let ast = match AstModule::parse("probe.star", code.to_owned(), &Dialect::Extended) {
        Ok(ast) => ast,
        Err(error) => {
            let text = format!("{error}");
            println!("{label:<52} PARSE ERROR: {}", text.lines().next().unwrap_or(""));
            return;
        }
    };
    let globals = Globals::standard();
    let (outcome, ticks_used): (Result<String, String>, u64) = Module::with_temp_heap(|module| {
        let mut evaluator = Evaluator::new(&module);
        if let Some(ticks) = tick_limit {
            evaluator.set_max_tick_count(ticks).unwrap();
        }
        if let Some(bytes) = heap_limit {
            evaluator.set_max_heap_size(bytes).unwrap();
        }
        let outcome = match evaluator.eval_module(ast, &globals) {
            Ok(value) => Ok(value.to_string()),
            Err(error) => {
                let text = format!("{error}");
                Err(text.lines().map(|line| line.trim()).filter(|line| !line.is_empty() && *line != "|").collect::<Vec<_>>().join(" / ").chars().take(200).collect::<String>())
            }
        };
        let ticks = evaluator.get_total_tick_count();
        starlark::Result::Ok((outcome, ticks))
    })
    .unwrap();
    let elapsed = started.elapsed();
    match outcome {
        Ok(value) => println!("{label:<52} OK {value}  ({elapsed:.2?}, {ticks_used} ticks)"),
        Err(first) => println!("{label:<52} ERR {first}  ({elapsed:.2?}, {ticks_used} ticks)"),
    }
}

fn main() {
    println!("== comprehensions and laziness");
    run(
        "16-cube, even sum, comprehension count",
        "def main():\n    return len([(x, y, z) for x in range(16) for y in range(16) for z in range(16) if (x + y + z) % 2 == 0])\nmain()",
        None,
        None,
    );
    run("generator expression", "def main():\n    return sum([1, 2]) + len(list(x for x in range(3)))\nmain()", None, None);
    run("yield", "def f():\n    yield 1\nf()", None, None);
    run("dict comprehension", "{x: x * x for x in range(4)}", None, None);
    run("range(1000000000000) is lazy (len, membership)", "[len(range(1000000000000)), 5 in range(10000000000000)]", None, None);
    run("any() over range 1000000000000 (eager list?)", "any([x > 2 for x in range(1000000000000)])", Some(5_000_000), Some(100_000_000));

    println!("\n== limits");
    run(
        "infinite-ish loop, 1M ticks",
        "def main():\n    n = 0\n    for _ in range(1000000000):\n        n += 1\n    return n\nmain()",
        Some(1_000_000),
        None,
    );
    run(
        "loop 500k iterations, 1M ticks",
        "def main():\n    n = 0\n    for _ in range(500000):\n        n += 1\n    return n\nmain()",
        Some(1_000_000),
        None,
    );
    run(
        "comprehension of 20M, 1M ticks (counted?)",
        "def main():\n    return len([x for x in range(20000000)])\nmain()",
        Some(1_000_000),
        None,
    );
    run(
        "comprehension of 20M, 1M ticks + 50 MB heap",
        "def main():\n    return len([x for x in range(20000000)])\nmain()",
        Some(1_000_000),
        Some(50_000_000),
    );
    run("list multiply bomb, 50 MB heap", "def main():\n    return len([0] * 100000000)\nmain()", None, Some(50_000_000));
    run("string growth, 50 MB heap", "def main():\n    s = 'a'\n    for _ in range(40):\n        s = s + s\n    return len(s)\nmain()", Some(10_000), Some(50_000_000));
    run("recursion", "def f(n):\n    return f(n + 1)\nf(0)", Some(1_000_000), None);

    println!("\n== sandbox by absence and maths");
    run("math module", "math.sin(1.0)", None, None);
    run("load / import", "load('x.star', 'y')", None, None);
    run("open / print / time names", "[type(print), 'open' in dir()]", None, None);
    run("floats", "[2.5 * 3.7, 1.0 / 3.0, float('1e22')]", None, None);
    run("dict order", "list({'alpha': 1, 'beta': 2, 'gamma': 3}.keys())", None, None);
    run("set / hash order", "list({'b': 1, 'a': 2}.items())", None, None);
    run("ticks counted by a fixed program", "def main():\n    n = 0\n    for i in range(1000):\n        n += i\n    return n\nmain()", None, None);
}
