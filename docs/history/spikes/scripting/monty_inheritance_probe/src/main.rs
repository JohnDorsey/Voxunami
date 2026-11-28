//! Throwaway probe for the scripting spec's "what a command returns" question: can the fork's Monty hold a result-class
//! hierarchy (CommandResult > CommandSuccess/CommandFailure > per-command classes), and what does it cost?

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

/// Runs the code in a fresh session; gives "value | printed text" or "ERROR last line".
fn probe(code: &str) -> String {
    let mut repl = new_repl();
    let mut output = String::new();
    let result = repl.feed_run(code, vec![], PrintWriter::CollectString(&mut output, Some(1 << 16)));
    match result {
        Ok(value) => {
            let printed = if output.is_empty() { String::new() } else { format!("   [printed {output:?}]") };
            format!("{}{printed}", value.py_repr())
        }
        Err(error) => {
            let text = error.to_string();
            format!("ERROR {}", text.lines().last().unwrap_or("").chars().take(150).collect::<String>())
        }
    }
}

const BASE: &str = "
class CommandResult:
    ok = None
    def __init__(self, message='', value=None):
        self.message = message
        self.value = value
    def __str__(self):
        return self.message if self.value is None else str(self.value)
    def __repr__(self):
        return type(self).__name__ + '(message=' + repr(self.message) + ')'

class CommandSuccess(CommandResult):
    ok = True

class CommandFailure(CommandResult):
    ok = False
    def __init__(self, message='', reason='engine:failed'):
        CommandResult.__init__(self, message, None)
        self.reason = reason

class TeleportSuccess(CommandSuccess):
    def __init__(self, message='', value=None, moved_out_of_a_wall=False):
        CommandSuccess.__init__(self, message, value)
        self.moved_out_of_a_wall = moved_out_of_a_wall

class TeleportFailure(CommandFailure):
    pass
";

fn main() {
    let cases: &[(&str, String)] = &[
        ("1 inheritance, method", "class A:\n    def f(self):\n        return 1\nclass B(A):\n    pass\nB().f()".into()),
        ("2 isinstance/issubclass", format!("{BASE}\nr = TeleportSuccess('ok')\n(isinstance(r, TeleportSuccess), isinstance(r, CommandSuccess), isinstance(r, CommandResult), isinstance(r, CommandFailure), issubclass(TeleportFailure, CommandResult))")),
        ("3 ok is a class variable", format!("{BASE}\n(TeleportSuccess('a').ok, TeleportFailure('b').ok, TeleportFailure('b').reason)")),
        ("4 str / repr / f-string / % / format / list", format!("{BASE}\nr = TeleportSuccess('moved', 5)\n(str(r), repr(r), f'{{r}}', '%s' % r, '{{}}'.format(r), [r], f'{{r!r}}')")),
        ("5 print(r) then bare repr", format!("{BASE}\nr = TeleportSuccess('moved')\nprint(r)\nr")),
        ("6 only __repr__ defined: str()", "class A:\n    def __repr__(self):\n        return 'A!'\nstr(A())".into()),
        ("7 bool(r) / if not r", format!("{BASE}\nf = TeleportFailure('no')\n(bool(f), 'truthy' if f else 'falsy', not f)")),
        ("8 __bool__ defined", "class A:\n    def __bool__(self):\n        return False\n(bool(A()), 'truthy' if A() else 'falsy')".into()),
        ("9 super()", "class A:\n    def __init__(self):\n        self.x = 1\nclass B(A):\n    def __init__(self):\n        super().__init__()\nB().x".into()),
        ("10 @property", "class A:\n    @property\n    def p(self):\n        return 1\nA().p".into()),
        ("11 @classmethod", "class A:\n    @classmethod\n    def c(cls):\n        return 1\nA.c()".into()),
        ("12 @staticmethod", "class A:\n    @staticmethod\n    def s():\n        return 1\nA.s()".into()),
        ("13 subclass Exception", "class E(Exception):\n    pass\nE('x')".into()),
        ("14 raise an instance of a plain class", "class A:\n    pass\nraise A()".into()),
        ("15 @dataclass", "from dataclasses import dataclass\n@dataclass\nclass P:\n    x: int\n    y: int = 0\nP(1)".into()),
        ("16 @dataclass inheritance", "from dataclasses import dataclass\n@dataclass\nclass P:\n    x: int\n@dataclass\nclass Q(P):\n    y: int = 5\n(Q(1), Q(1) == Q(1, 5), isinstance(Q(1), P))".into()),
        ("17 type(name, bases, dict)", format!("{BASE}\nX = type('RoofSuccess', (CommandSuccess,), {{}})\nr = X('built')\n(isinstance(r, CommandSuccess), type(r).__name__, repr(r), X.ok)")),
        ("18 __eq__ and ==", "class A:\n    def __init__(self, v):\n        self.v = v\n    def __eq__(self, other):\n        return self.v == other.v\n(A(1) == A(1), A(1) == A(2))".into()),
        ("19 match with class patterns and inheritance", format!("{BASE}\ndef what(r):\n    match r:\n        case TeleportSuccess(moved_out_of_a_wall=True):\n            return 'moved out of a wall'\n        case CommandSuccess():\n            return 'success'\n        case CommandFailure():\n            return 'failure'\n        case _:\n            return 'other'\n(what(TeleportSuccess('x', None, True)), what(TeleportSuccess('x')), what(TeleportFailure('n')), what(5))")),
        ("20 __match_args__ positional pattern", "class P:\n    __match_args__ = ('x', 'y')\n    def __init__(self, x, y):\n        self.x = x\n        self.y = y\nmatch P(1, 2):\n    case P(a, b):\n        (a, b)".into()),
        ("21 vars / __dict__", format!("{BASE}\nvars(TeleportSuccess('m', 3, True))")),
        ("22 multiple inheritance + MRO", "class A:\n    pass\nclass B:\n    pass\nclass C(A, B):\n    pass\n[k.__name__ for k in C.__mro__]".into()),
        ("23 a failure that is raised (custom Exception) via ValueError(str(r))", format!("{BASE}\nf = TeleportFailure('no such player')\ntry:\n    raise ValueError(str(f))\nexcept ValueError as e:\n    str(e)")),
        ("24 result as dict key / hash", format!("{BASE}\nd = {{TeleportSuccess('a'): 1}}\nlen(d)")),
        ("25 abstract-ish: base method calls the subclass override", "class A:\n    def label(self):\n        return self.word() + '!'\n    def word(self):\n        return 'a'\nclass B(A):\n    def word(self):\n        return 'b'\nB().label()".into()),
        ("26 __str__ that calls a missing attribute", "class A:\n    def __str__(self):\n        return self.nope\nstr(A())".into()),
        ("27 __init_subclass__ registry", "registry = []\nclass Base:\n    def __init_subclass__(cls, **kw):\n        registry.append(cls.__name__)\nclass A(Base):\n    pass\nclass B(Base):\n    pass\nregistry".into()),
        ("28 isinstance with a tuple of classes", format!("{BASE}\nisinstance(TeleportFailure('x'), (TeleportSuccess, CommandFailure))")),
    ];
    for (label, code) in cases {
        println!("{label:<62} => {}", probe(code));
    }

    // ---- cost: a prelude of generated classes, as a game with many commands would carry
    println!("\n--- cost of a prelude with N commands, each a success class and a failure class, each with extra fields");
    for commands in [10usize, 50, 100, 200] {
        let mut source = String::from(BASE);
        for index in 0..commands {
            source.push_str(&format!(
                "\nclass Cmd{index}Success(CommandSuccess):\n    def __init__(self, message='', value=None, amount=0, flag=False):\n        CommandSuccess.__init__(self, message, value)\n        self.amount = amount\n        self.flag = flag\n\nclass Cmd{index}Failure(CommandFailure):\n    pass\n"
            ));
        }
        let mut times = Vec::new();
        let mut kept = None;
        for run in 0..20 {
            let start = Instant::now();
            let mut repl = new_repl();
            repl.feed_run(&source, vec![], PrintWriter::Disabled).unwrap();
            times.push(start.elapsed());
            if run == 0 {
                kept = Some(repl);
            }
        }
        times.sort();
        let mut repl = kept.unwrap();
        let model = repl.model_bytes();
        let bytes = dump("probe.py", None, &[], SessionRef::Idle(&repl)).unwrap();
        let start = Instant::now();
        let loaded = Dump::load(&bytes).unwrap();
        let load_time = start.elapsed();
        let Session::Idle(mut restored) = loaded.state else { panic!("not idle") };
        restored.set_os_policy(pinned());
        let check = format!("r = Cmd{last}Success('x', 1, 2, True)\n(isinstance(r, CommandSuccess), isinstance(r, CommandResult), repr(r), r.ok)", last = commands - 1);
        let after = restored.feed_run(&check, vec![], PrintWriter::Disabled).unwrap().py_repr().to_string();

        // making one result, the thing every command call would do
        let count = 2000;
        let start = Instant::now();
        repl.feed_run(&format!("for i in range({count}):\n    r = Cmd0Success('x', 1, 2, True)"), vec![], PrintWriter::Disabled).unwrap();
        let per_instance = start.elapsed() / count;
        println!(
            "N={commands:<4} source {:>7} bytes | prelude run median {:>6.2} ms | model_bytes {:>8} | dump {:>7} bytes | load {:>6.0} us | one result {:>5.1} us | after restore: {after}",
            source.len(),
            times[times.len() / 2].as_secs_f64() * 1e3,
            model,
            bytes.len(),
            load_time.as_secs_f64() * 1e6,
            per_instance.as_secs_f64() * 1e6
        );
    }

    // ---- an instance kept in a global survives a dump
    let mut repl = new_repl();
    repl.feed_run(&format!("{BASE}\nkept = TeleportSuccess('moved', 7, True)"), vec![], PrintWriter::Disabled).unwrap();
    let bytes = dump("probe.py", None, &[], SessionRef::Idle(&repl)).unwrap();
    let Session::Idle(mut restored) = Dump::load(&bytes).unwrap().state else { panic!("not idle") };
    restored.set_os_policy(pinned());
    let after = restored
        .feed_run("(repr(kept), isinstance(kept, CommandSuccess), kept.moved_out_of_a_wall, str(kept), type(kept).__name__)", vec![], PrintWriter::Disabled)
        .unwrap()
        .py_repr()
        .to_string();
    println!("\nan instance in a global after dump and restore: {after}");

    // ---- what the host sees when a result instance is the value of the line
    let mut repl = new_repl();
    let value = repl.feed_run(&format!("{BASE}\nTeleportSuccess('moved', 7, True)"), vec![], PrintWriter::Disabled).unwrap();
    println!("what the host gets for a result instance as a line's value: {value:?}");
}
