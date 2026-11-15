//! Rune 0.14.2: can a script yield to the host each tick and be resumed with a fresh instruction budget?
use std::sync::Arc;

use rune::runtime::{Generator, budget};
use rune::{Context, Diagnostics, Source, Sources, Vm};

fn main() {
    let context = Context::with_default_modules().unwrap();
    let runtime = Arc::new(context.runtime().unwrap());
    let mut sources = Sources::new();
    sources
        .insert(
            Source::memory(
                "fn wait_script() {\n\
                     let ticks_seen = 0;\n\
                     while ticks_seen < 5 { yield ticks_seen; ticks_seen += 1; }\n\
                     let burn = 0;\n\
                     loop { burn += 1; if burn == 1_000_000_000 { break; } yield burn; }\n\
                 }\n\
                 pub fn main() { wait_script() }",
            )
            .unwrap(),
        )
        .unwrap();
    let mut diagnostics = Diagnostics::new();
    let unit = rune::prepare(&mut sources)
        .with_context(&context)
        .with_diagnostics(&mut diagnostics)
        .build()
        .unwrap();
    let mut vm = Vm::new(runtime, Arc::new(unit));
    let value = vm.call(["main"], ()).unwrap();
    let mut generator: Generator<Vm> = rune::from_value(value).unwrap();
    for tick in 0..8 {
        // each "tick" the host resumes the script with its own fresh budget of 10,000 instructions
        let outcome = budget::with(10_000, || generator.next().into_result()).call();
        match outcome {
            Ok(Some(value)) => println!("tick {tick}: script yielded {value:?}"),
            Ok(None) => println!("tick {tick}: script finished"),
            Err(error) => {
                println!("tick {tick}: ERR {}", format!("{error}").lines().next().unwrap_or(""));
                break;
            }
        }
    }
}
