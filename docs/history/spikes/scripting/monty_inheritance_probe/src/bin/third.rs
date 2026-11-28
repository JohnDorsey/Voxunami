//! Third probe: a result that raises when a failure is used as if it were a success (Rust's Result, unwrap).
use monty::MontyRepl;
use monty_types::{CompileOptions, PrintWriter, ResourceTracker};

fn probe(code: &str) -> String {
    let mut repl = MontyRepl::new("probe.py", ResourceTracker::default(), CompileOptions::default());
    let mut output = String::new();
    match repl.feed_run(code, vec![], PrintWriter::CollectString(&mut output, Some(1 << 16))) {
        Ok(value) => format!("{}{}", value.py_repr(), if output.is_empty() { String::new() } else { format!("  [printed {output:?}]") }),
        Err(error) => format!("ERROR {}", error.to_string().lines().last().unwrap_or("").chars().take(170).collect::<String>()),
    }
}

const BASE: &str = "
class CommandResult:
    ok = None
    def __init__(self, message='', **fields):
        self.message = message
        self.names = list(fields)
        for name in fields:
            setattr(self, name, fields[name])
    def __repr__(self):
        parts = [repr(self.message)]
        for name in self.names:
            parts.append(name + '=' + repr(getattr(self, name)))
        return type(self).__name__ + '(' + ', '.join(parts) + ')'

class CommandSuccess(CommandResult):
    ok = True
    def __init__(self, message='', value=None, **fields):
        CommandResult.__init__(self, message, **fields)
        self.value = value
    def unwrap(self):
        return self.value
    def unwrap_or(self, default):
        return self.value
    def __str__(self):
        return self.message if self.value is None else str(self.value)

class CommandFailure(CommandResult):
    ok = False
    def __init__(self, message='', reason='engine:failed', **fields):
        CommandResult.__init__(self, message, **fields)
        self.reason = reason
    def unwrap(self):
        raise RuntimeError('unwrapped a failure of ' + self.reason + ': ' + self.message)
    def unwrap_or(self, default):
        return default
    def __str__(self):
        raise RuntimeError('str() of a failure of ' + self.reason + ': ' + self.message + ' (check .ok or match first)')
";

fn main() {
    let cases: &[(&str, String)] = &[
        ("success.value", format!("{BASE}\nCommandSuccess('moved', 5).value")),
        ("failure.value (no such attribute)", format!("{BASE}\nCommandFailure('no such player').value")),
        ("failure.unwrap()", format!("{BASE}\nCommandFailure('no such player', 'engine:no_such_player').unwrap()")),
        ("failure.unwrap_or(0)", format!("{BASE}\nCommandFailure('x').unwrap_or(0)")),
        ("success.unwrap()", format!("{BASE}\nCommandSuccess('m', 5).unwrap()")),
        ("str(failure) raises", format!("{BASE}\nstr(CommandFailure('no such player'))")),
        ("f-string of a failure raises", format!("{BASE}\nf = CommandFailure('no such player')\nf'went to {{f}}'")),
        ("print(failure) raises", format!("{BASE}\nprint(CommandFailure('no such player'))")),
        ("repr(failure) still works", format!("{BASE}\nrepr(CommandFailure('no such player', reason='engine:no_such_player'))")),
        ("failure + 1 / iteration / int()", format!("{BASE}\nf = CommandFailure('x')\nf + 1")),
        ("int(failure)", format!("{BASE}\nint(CommandFailure('x'))")),
        ("failure in a list repr", format!("{BASE}\nrepr([CommandFailure('x'), CommandSuccess('y', 1)])")),
        ("match on the result", format!("{BASE}\nr = CommandFailure('no such player', 'engine:no_such_player')\nmatch r:\n    case CommandSuccess(value=v):\n        out = v\n    case CommandFailure(reason=why):\n        out = why\nout")),
        ("if r: (silent trap)", format!("{BASE}\nr = CommandFailure('x')\n'taken' if r else 'not taken'")),
        ("__getattr__ available?", "class A:\n    def __getattr__(self, name):\n        raise AttributeError('no ' + name)\nA().zzz".into()),
        ("__eq__ on a failure", format!("{BASE}\nCommandFailure('x') == 5")),
    ];
    for (label, code) in cases {
        println!("{label:<40} => {}", probe(code));
    }
}
