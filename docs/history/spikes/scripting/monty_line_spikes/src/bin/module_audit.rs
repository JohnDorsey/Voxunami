//! Spike S3 of scripting-spec.md: which of Monty's built-in modules can differ from one process or one machine to the next?
//!
//! `module_audit`            runs every probe in two child processes per policy and lists the lines that differ.
//! `module_audit child NAME` prints one process's results (NAME is `default` or `pinned`).
//!
//! Policy `default` is `OsPolicy::default()` (what a host gets if it sets nothing); `pinned` is the policy a game
//! would set: a fixed clock, sleep that returns at once, a seeded generator, process time zero.

use std::process::Command;

use monty::MontyRepl;
use monty_types::{
    CompileOptions, DateTimeSource, OsPolicy, PrintWriter, RandomSeed, RandomStart, ResourceTracker, SleepMode,
};

fn policy(name: &str) -> OsPolicy {
    match name {
        "pinned" => OsPolicy {
            datetime: DateTimeSource::Fixed { unix_seconds: 1_700_000_000, microsecond: 0 },
            sleep: SleepMode::Zero,
            random_start: RandomStart::Seed(RandomSeed::Str("voxunami".to_owned())),
            ..OsPolicy::default()
        },
        _ => OsPolicy::default(),
    }
}

const MODULES: &[&str] = &[
    "sys", "typing", "asyncio", "pathlib", "os", "os.path", "math", "json", "re", "datetime", "unicodedata",
    "itertools", "dataclasses", "collections", "functools", "copy", "csv", "decimal", "fnmatch", "hashlib", "random",
    "statistics", "time", "tomllib", "base64", "binascii", "bisect", "annotationlib", "gc", "uuid", "string",
    "textwrap", "operator", "enum", "abc", "io", "struct", "secrets", "threading", "subprocess", "socket", "ctypes",
    "platform", "locale", "tempfile", "shutil", "glob", "pickle", "array", "heapq", "contextlib", "warnings",
    "traceback", "inspect", "builtins", "weakref", "types", "numbers", "fractions", "zoneinfo", "calendar",
    "difflib", "concurrent.futures", "multiprocessing", "select", "signal", "ssl", "http", "urllib", "zlib", "gzip",
];

/// (label, snippet). Each runs in a fresh session; the value of the last expression is printed.
const PROBES: &[(&str, &str)] = &[
    ("hash('abc')", "hash('abc')"),
    ("hash(b'abc')", "hash(b'abc')"),
    ("hash(1.5)", "hash(1.5)"),
    ("hash((1,'a'))", "hash((1, 'a'))"),
    ("hash(frozenset)", "hash(frozenset([1, 2, 3]))"),
    ("set of str, order", "list({'b','a','c','d','e','f','g','h'})"),
    ("set of mixed, order", "list({1.5, 2.5, 'x', (1, 2), None, True and 7})"),
    ("str(set of str)", "str({'x','y','z','w','v'})"),
    ("dict of str keys, order", "list({'b': 1, 'a': 2, 'c': 3})"),
    ("set(range) order", "list(set(range(40, 0, -3)))"),
    ("id(1)", "id(1)"),
    ("id('a')", "id('a')"),
    ("id([])", "id([])"),
    ("id of two lists differ", "id([]) == id([])"),
    ("repr(print)", "repr(print)"),
    ("repr(lambda)", "repr(lambda: 0)"),
    ("repr(object())", "repr(object())"),
    ("time.time()", "import time\ntime.time()"),
    ("time.time_ns()", "import time\ntime.time_ns()"),
    ("time.monotonic()", "import time\ntime.monotonic()"),
    ("time.perf_counter()", "import time\ntime.perf_counter()"),
    ("time.process_time()", "import time\ntime.process_time()"),
    ("time.localtime()", "import time\ntuple(time.localtime())"),
    ("time.sleep(5)", "import time\ntime.sleep(5)\n'slept'"),
    ("datetime.now()", "import datetime\nstr(datetime.datetime.now())"),
    ("datetime.utcnow()", "import datetime\nstr(datetime.datetime.utcnow())"),
    ("date.today()", "import datetime\nstr(datetime.date.today())"),
    ("random.random()", "import random\nrandom.random()"),
    ("random.randint", "import random\nrandom.randint(0, 10**9)"),
    ("random.Random().random()", "import random\nrandom.Random().random()"),
    ("random.SystemRandom()", "import random\nrandom.SystemRandom().random()"),
    ("random.seed(5) then random", "import random\nrandom.seed(5)\nrandom.random()"),
    ("random.choice", "import random\nrandom.choice(['a','b','c','d','e'])"),
    ("random.shuffle", "import random\nx = list(range(10))\nrandom.shuffle(x)\nx"),
    ("random.gauss", "import random\nrandom.gauss(0, 1)"),
    ("os.urandom(4)", "import os\nos.urandom(4)"),
    ("os.getpid()", "import os\nos.getpid()"),
    ("os.cpu_count()", "import os\nos.cpu_count()"),
    ("os.getcwd()", "import os\nos.getcwd()"),
    ("os.getenv('PATH')", "import os\nos.getenv('PATH')"),
    ("os.name", "import os\nos.name"),
    ("os.sep", "import os\nos.sep"),
    ("sys.platform", "import sys\nsys.platform"),
    ("sys.version", "import sys\nsys.version"),
    ("sys.byteorder", "import sys\nsys.byteorder"),
    ("sys.maxsize", "import sys\nsys.maxsize"),
    ("sys.getsizeof([])", "import sys\nsys.getsizeof([1,2,3])"),
    ("sys.executable", "import sys\nsys.executable"),
    ("sys.argv", "import sys\nsys.argv"),
    ("sys.getrecursionlimit()", "import sys\nsys.getrecursionlimit()"),
    ("uuid.uuid4()", "import uuid\nstr(uuid.uuid4())"),
    ("secrets.token_hex", "import secrets\nsecrets.token_hex(4)"),
    ("gc.collect()", "import gc\ngc.collect()"),
    ("math.sin(1.0)", "import math\nmath.sin(1.0)"),
    ("math.exp(0.1)", "import math\nmath.exp(0.1)"),
    ("2 ** 0.5", "2 ** 0.5"),
    ("float repr", "[0.1 + 0.2, 1e22, 1e-7, 5e-324, 1.7976931348623157e308]"),
    ("round(2.675, 2)", "round(2.675, 2)"),
    ("fsum", "import math\nmath.fsum([0.1] * 10)"),
    ("sum floats", "sum([0.1] * 10)"),
    ("nan", "float('nan') == float('nan')"),
    ("str.upper unicode", "'straße é İ'.upper()"),
    ("sorted str", "sorted(['b', 'B', 'a', 'é', 'z', 'Z'])"),
    ("hashlib.sha256", "import hashlib\nhashlib.sha256(b'abc').hexdigest()"),
    ("json.dumps dict", "import json\njson.dumps({'b': 1, 'a': [1.5, None]})"),
    ("json.dumps set error", "import json\njson.dumps({'b': 1, 'a': 2}, sort_keys=True)"),
    ("collections.Counter order", "import collections\nlist(collections.Counter('mississippi').items())"),
    ("decimal", "import decimal\nstr(decimal.Decimal(1) / decimal.Decimal(3))"),
    ("statistics.mean", "import statistics\nstatistics.mean([1, 2, 3.5])"),
    ("re.match", "import re\nbool(re.match('a+', 'aaab'))"),
    ("pathlib cwd", "import pathlib\nstr(pathlib.Path.cwd())"),
    ("open a file", "open('x.txt').read()"),
    ("input()", "input()"),
    ("globals() keys", "sorted(globals().keys())"),
    ("dir() of builtins sample", "len(dir(__builtins__)) if '__builtins__' in globals() else 'no __builtins__'"),
];

fn run_probe(policy: &OsPolicy, code: &str) -> String {
    let mut repl =
        MontyRepl::new("probe.py", ResourceTracker::default(), CompileOptions::default()).with_os_policy(policy.clone());
    let mut output = String::new();
    let result = repl.feed_run(code, vec![], PrintWriter::CollectString(&mut output, Some(1 << 16)));
    match result {
        Ok(value) => value.py_repr().to_string(),
        Err(error) => {
            let text = error.to_string();
            format!("ERROR {}", text.lines().last().unwrap_or("").chars().take(90).collect::<String>())
        }
    }
}

fn child(name: &str) {
    let policy = policy(name);
    for module in MODULES {
        println!("import {module:<22} => {}", run_probe(&policy, &format!("import {module}\n'imported'")));
    }
    for (label, code) in PROBES {
        println!("{label:<34} => {}", run_probe(&policy, code));
    }
}

fn run_child(name: &str) -> Vec<String> {
    let exe = std::env::current_exe().unwrap();
    let output = Command::new(exe).args(["child", name]).output().unwrap();
    String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.get(1).map(String::as_str) == Some("child") {
        child(&arguments[2]);
        return;
    }
    for name in ["default", "pinned"] {
        let (first, second) = (run_child(name), run_child(name));
        println!("=== policy {name}: {} lines, two separate processes", first.len());
        let importable: Vec<&str> = first
            .iter()
            .filter(|line| line.starts_with("import ") && line.ends_with("=> 'imported'"))
            .map(|line| line.trim_start_matches("import ").split_whitespace().next().unwrap())
            .collect();
        println!("importable ({}): {}", importable.len(), importable.join(" "));
        let refused: Vec<&str> = first
            .iter()
            .filter(|line| line.starts_with("import ") && !line.ends_with("=> 'imported'"))
            .map(|line| line.trim_start_matches("import ").split_whitespace().next().unwrap())
            .collect();
        println!("refused ({}): {}", refused.len(), refused.join(" "));
        let mut differing = 0;
        for (a, b) in first.iter().zip(second.iter()) {
            if a != b {
                differing += 1;
                println!("DIFFERS  {a}\n     vs  {}", b.split("=> ").last().unwrap_or(""));
            }
        }
        println!("{differing} of {} lines differ between the two processes\n", first.len());
        if name == "pinned" {
            println!("--- every probe's result under policy pinned (for reading, not only for the differences)");
            for line in first.iter().filter(|line| !line.starts_with("import ")) {
                println!("{line}");
            }
        }
    }
}
