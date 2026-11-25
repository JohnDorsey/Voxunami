//! Second probe: a base class that takes the extra fields as keyword arguments, classes made by `type()` only when first used,
//! and what the host sees and what a line must do to print `repr` the way the sandbox means it.

use std::time::Instant;

use monty::{Dump, MontyRepl, Session, SessionRef, dump};
use monty_types::{CompileOptions, DateTimeSource, OsPolicy, PrintWriter, RandomSeed, RandomStart, ResourceTracker, SleepMode};

fn pinned() -> OsPolicy {
    OsPolicy {
        datetime: DateTimeSource::Fixed { unix_seconds: 1_700_000_000, microsecond: 0 },
        sleep: SleepMode::Zero,
        random_start: RandomStart::Seed(RandomSeed::Str("voxunami".to_owned())),
        ..OsPolicy::default()
    }
}

fn new_repl() -> MontyRepl {
    MontyRepl::new("probe.py", ResourceTracker::default(), CompileOptions::default()).with_os_policy(pinned())
}

fn probe(code: &str) -> String {
    let mut repl = new_repl();
    let mut output = String::new();
    match repl.feed_run(code, vec![], PrintWriter::CollectString(&mut output, Some(1 << 16))) {
        Ok(value) => {
            let printed = if output.is_empty() { String::new() } else { format!("   [printed {output:?}]") };
            format!("{}{printed}", value.py_repr())
        }
        Err(error) => format!("ERROR {}", error.to_string().lines().last().unwrap_or("").chars().take(150).collect::<String>()),
    }
}

const BASE: &str = "
class CommandResult:
    ok = None
    def __init__(self, message='', value=None, **fields):
        self.message = message
        self.value = value
        self.names = list(fields)
        for name in fields:
            setattr(self, name, fields[name])
    def __str__(self):
        return self.message if self.value is None else str(self.value)
    def __repr__(self):
        parts = [repr(self.message)]
        for name in self.names:
            parts.append(name + '=' + repr(getattr(self, name)))
        return type(self).__name__ + '(' + ', '.join(parts) + ')'

class CommandSuccess(CommandResult):
    ok = True

class CommandFailure(CommandResult):
    ok = False

_classes = {}
def make(kind, base, **fields):
    if kind not in _classes:
        _classes[kind] = type(kind, (base,), {})
    return _classes[kind]('', None, **fields)
";

fn main() {
    let cases: &[(&str, String)] = &[
        ("setattr + **fields + __dict__ iteration", format!("{BASE}\nCommandSuccess('moved', 5, moved_out_of_a_wall=True, pushed=2)")),
        ("repr() inside the sandbox", format!("{BASE}\nrepr(CommandSuccess('moved', 5, moved_out_of_a_wall=True))")),
        ("class made on first use, twice (same class)", format!("{BASE}\na = make('TeleportSuccess', CommandSuccess, moved=True)\nb = make('TeleportSuccess', CommandSuccess, moved=False)\n(type(a) is type(b), isinstance(a, CommandSuccess), repr(a), repr(b))")),
        ("failure then success of one command are different classes", format!("{BASE}\na = make('TeleportSuccess', CommandSuccess)\nb = make('TeleportFailure', CommandFailure)\n(a.ok, b.ok, isinstance(b, CommandSuccess))")),
        ("a line's value: repr(_) wrapper idea (value is a list of results)", format!("{BASE}\nx = [CommandSuccess('a'), CommandFailure('b', reason='engine:no_such_player')]\nrepr(x)")),
        ("hasattr / getattr", format!("{BASE}\nr = CommandSuccess('a', moved=True)\n(hasattr(r, 'moved'), getattr(r, 'pushed', 'none'))")),
        ("a result compared: == by identity", format!("{BASE}\nCommandSuccess('a') == CommandSuccess('a')")),
        ("the failure message in a splice: str(failure)", format!("{BASE}\nstr(CommandFailure('no such player Bob', reason='engine:no_such_player'))")),
        ("raise RuntimeError(str(failure)) and catch it", format!("{BASE}\nf = CommandFailure('no such player')\ntry:\n    raise RuntimeError(f.message)\nexcept RuntimeError as e:\n    out = str(e)\nout")),
    ];
    for (label, code) in cases {
        println!("{label:<66} => {}", probe(code));
    }

    println!("\n--- lazy classes: cost of the base, then of making N command classes on first use, then one result");
    let mut base_times = Vec::new();
    let mut kept = None;
    for run in 0..30 {
        let start = Instant::now();
        let mut repl = new_repl();
        repl.feed_run(BASE, vec![], PrintWriter::Disabled).unwrap();
        base_times.push(start.elapsed());
        if run == 0 {
            kept = Some(repl);
        }
    }
    base_times.sort();
    let mut repl = kept.unwrap();
    let bytes = dump("probe.py", None, &[], SessionRef::Idle(&repl)).unwrap();
    println!("base only: run {:.2} ms, model_bytes {}, dump {} bytes", base_times[15].as_secs_f64() * 1e3, repl.model_bytes(), bytes.len());
    for commands in [10usize, 50, 100, 200] {
        let start = Instant::now();
        repl.feed_run(&format!("for i in range({commands}):\n    make('Cmd' + str(i) + 'Success', CommandSuccess, amount=i)\n    make('Cmd' + str(i) + 'Failure', CommandFailure)"), vec![], PrintWriter::Disabled).unwrap();
        let made = start.elapsed();
        let model = repl.model_bytes();
        let bytes = dump("probe.py", None, &[], SessionRef::Idle(&repl)).unwrap();
        let start = Instant::now();
        let loaded = Dump::load(&bytes).unwrap();
        let load = start.elapsed();
        let Session::Idle(mut restored) = loaded.state else { panic!("not idle") };
        restored.set_os_policy(pinned());
        let after = restored.feed_run(&format!("r = make('Cmd{}Success', CommandSuccess, amount=1)\n(isinstance(r, CommandSuccess), repr(r))", commands - 1), vec![], PrintWriter::Disabled).unwrap().py_repr().to_string();
        let count = 2000;
        let start = Instant::now();
        repl.feed_run(&format!("for i in range({count}):\n    r = make('Cmd0Success', CommandSuccess, amount=i, flag=True)"), vec![], PrintWriter::Disabled).unwrap();
        let per = start.elapsed() / count;
        println!(
            "classes made for {commands:<4} commands ({:<3} classes): took {:>6.2} ms | model_bytes {:>8} | dump {:>7} bytes | load {:>5.0} us | one result via make() {:>4.1} us | after restore {after}",
            commands * 2,
            made.as_secs_f64() * 1e3,
            model,
            bytes.len(),
            load.as_secs_f64() * 1e6,
            per.as_secs_f64() * 1e6
        );
    }
}
