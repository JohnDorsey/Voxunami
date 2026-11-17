#![allow(unsafe_code)]
//! Throwaway probe (not committed): real allocator growth per kept file source against the model's price.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use monty::{MontyRepl, ReplProgress};
use monty_types::{
    CompileOptions, ExtFunctionResult, IMPORT_CALL, MontyObject, OsFunctionCall, PrintWriter, ResourceTracker,
    ModuleRead, find_module_file,
};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;

fn feed(repl: MontyRepl, code: &str, source: &str) -> MontyRepl {
    let mut progress = repl.feed_start(code, Vec::<(String, MontyObject)>::new(), PrintWriter::Disabled);
    loop {
        match progress {
            Ok(ReplProgress::Complete { repl, .. }) => return repl,
            Ok(ReplProgress::OsCall(call)) => {
                let result = match &call.function_call {
                    OsFunctionCall::Import(args) => find_module_file(args, |path| {
                        if path == "/proj/bad.py" { ModuleRead::Text(source.to_owned()) } else { ModuleRead::Missing }
                    })
                    .unwrap_or_else(|| ExtFunctionResult::NotFound(IMPORT_CALL.to_owned())),
                    other => other.no_handler_result(),
                };
                progress = call.resume(result, PrintWriter::Disabled);
            }
            Ok(_) => panic!("suspension"),
            Err(err) => panic!("{}", err.error),
        }
    }
}

#[test]
fn probe() {
    let attempt = "try:\n    import bad\nexcept (ValueError, SyntaxError):\n    pass";
    let n = 500usize;
    println!("{:>8} {:>12} {:>12} {:>8}", "bytes", "real/try", "model/try", "m/r");
    for (label, source) in [
        ("tiny", "raise ValueError('x')\n".to_owned()),
        ("body 10 lines", "x = 1\n".repeat(10) + "raise ValueError('x')\n"),
        ("200 bytes of code", "x = 1\n".repeat(33) + "raise ValueError('x')\n"),
        ("2 KB of code", "x = 1\n".repeat(330) + "raise ValueError('x')\n"),
        ("2 KB of comment", "#".repeat(2000) + "\nraise ValueError('x')\n"),
        ("functions", "def f(a, b):\n    return a + b\n".repeat(20) + "raise ValueError('x')\n"),
    ] {
        let mut repl = MontyRepl::new("main.py", ResourceTracker::default(), CompileOptions::default());
        repl.set_cwd("/proj");
        repl = feed(repl, attempt, &source);
        let (real0, model0) = (LIVE.load(Ordering::Relaxed), repl.model_bytes());
        for _ in 0..n {
            repl = feed(repl, attempt, &source);
        }
        let real = (LIVE.load(Ordering::Relaxed) as i64 - real0 as i64) as f64 / n as f64;
        let model = (repl.model_bytes() - model0) as f64 / n as f64;
        println!("{label:<20} {:>6} {real:>12.0} {model:>12.0} {:>8.2}", source.len(), model / real);
    }
}

#[test]
fn probe_exec() {
    let n = 500usize;
    println!("{:>8} {:>12} {:>12} {:>8}", "bytes", "real/try", "model/try", "m/r");
    for (label, source) in [
        ("tiny", "x = 1\n".to_owned()),
        ("200 bytes of code", "x = 1\n".repeat(33)),
        ("2 KB of code", "x = 1\n".repeat(330)),
        ("2 KB of comment", "#".repeat(2000) + "\nx = 1\n"),
        ("functions", "def f(a, b):\n    return a + b\n".repeat(20)),
    ] {
        let mut repl = MontyRepl::new("main.py", ResourceTracker::default(), CompileOptions::default());
        repl = feed(repl, &format!("src = {source:?}\nexec(src)"), "");
        let (real0, model0) = (LIVE.load(Ordering::Relaxed), repl.model_bytes());
        for _ in 0..n {
            repl = feed(repl, "exec(src)", "");
        }
        let real = (LIVE.load(Ordering::Relaxed) as i64 - real0 as i64) as f64 / n as f64;
        let model = (repl.model_bytes() - model0) as f64 / n as f64;
        println!("exec {label:<20} {:>6} {real:>12.0} {model:>12.0} {:>8.2}", source.len(), model / real);
    }
}
