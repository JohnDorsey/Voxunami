//! Developer tool: fly through generated terrain, or render it headlessly to an image.

mod headless;
mod options;
mod scene;
mod streaming;
mod windowed;

use std::process::ExitCode;

use options::Command;

fn main() -> ExitCode {
    let options = match options::parse(std::env::args().skip(1)) {
        Ok(Command::Run(options)) => options,
        Ok(Command::ShowHelp) => {
            println!("{}", options::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("error: {message}\n\n{}", options::USAGE);
            return ExitCode::FAILURE;
        }
    };
    let result = if options.screenshot.is_some() || options.benchmark {
        headless::run(&options)
    } else {
        windowed::run(options)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
