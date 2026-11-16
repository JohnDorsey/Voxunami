//! Throwaway audit: for each native operation, how many steps does it charge against how much wall time?
use std::{env, time::{Duration, Instant}};

use monty::{MontyRun, RunProgress};
use monty_types::{CompileOptions, MontyObject, PrintWriter, ResourceLimits, ResourceTracker};

// (name, setup, operation)
const BUILT_IN: &[(&str, &str, &str)] = include!("cases.in");

/// Cases come from the file named by `CASES_FILE` (tab-separated name, setup, operation; `\n` is a newline)
/// when it is set, otherwise from the built-in list.
fn load_cases() -> Vec<(String, String, String)> {
    if let Ok(path) = env::var("CASES_FILE") {
        let text = std::fs::read_to_string(path).unwrap();
        return text
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                let mut parts = line.split('\t').map(|part| part.replace("\\n", "\n"));
                (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap())
            })
            .collect();
    }
    BUILT_IN.iter().map(|(a, b, c)| ((*a).to_owned(), (*b).to_owned(), (*c).to_owned())).collect()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let cases = load_cases();
    if args.len() < 2 {
        for (i, (name, _, _)) in cases.iter().enumerate() {
            println!("{i} {name}");
        }
        return;
    }
    let index: usize = args[1].parse().unwrap();
    let (name, setup, op) = cases[index].clone();
    let code = format!("{setup}\nprobe()\n{op}\nprobe()\n");
    let limits = if let Ok(budget) = env::var("STEPS") { ResourceLimits::default().max_turn_steps(budget.parse().unwrap()) } else if env::var("NOMEM").is_ok() { ResourceLimits::default() } else { ResourceLimits::default().max_memory(2_000_000_000) };
    let run = match MontyRun::new(code, "audit.py", vec!["probe".to_owned()], CompileOptions::default()) {
        Ok(run) => run,
        Err(error) => {
            println!("{index:3} {name:<34} COMPILE ERROR {error}");
            return;
        }
    };
    let mut progress = run
        .start(vec![MontyObject::function("probe".to_owned(), None)], ResourceTracker::new(limits), PrintWriter::Disabled)
        .unwrap();
    let mut marks: Vec<(u64, Instant)> = Vec::new();
    let outcome = loop {
        match progress {
            RunProgress::FunctionCall(mut call) => {
                marks.push((call.tracker_mut().steps(), Instant::now()));
                match call.resume(MontyObject::none(), PrintWriter::Disabled) {
                    Ok(next) => progress = next,
                    Err(error) => break format!("ERR {}", error.to_string().lines().last().unwrap_or("").chars().take(70).collect::<String>()),
                }
            }
            RunProgress::Complete(_) => break "ok".to_owned(),
            other => panic!("unexpected {other:?}"),
        }
    };
    if marks.len() == 2 {
        let steps = marks[1].0 - marks[0].0;
        let wall: Duration = marks[1].1 - marks[0].1;
        let ns_per_step = wall.as_nanos() as f64 / steps.max(1) as f64;
        println!("{index:3} {name:<34} steps {steps:>12}  wall {:>9.1?}  ns/step {ns_per_step:>12.1}  {outcome}", wall);
    } else {
        println!("{index:3} {name:<34} (stopped before the end of the setup or the op)  {outcome}");
    }
}
