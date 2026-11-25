//! Runs Cave's rolling-hills generator over the same bricks natively and as a wasm module in two runtimes (wasmtime, which compiles to machine code, and
//! wasmi, an interpreter), with and without fuel, checks that every brick equals the native one voxel for voxel, and prints what each costs.
//!
//! `cargo run --release -p harness -- WASM_FILE [PASSES]`; see `run.sh`. Wall-clock time is used here because this measures speed; nothing in the engine does.

use std::time::{Duration, Instant};

use spike_generator::SpikeGenerator;
use voxel_space::BRICK_VOXEL_COUNT;

const SEED: u64 = 5;
const BYTES_PER_BRICK: usize = BRICK_VOXEL_COUNT * 4;

/// The bricks measured: a block of columns round the origin, from below the caves' floor to above the highest hill (the surface is near height 64, hills add up to
/// about 80, so bricks 0 to 4 hold ground; the rest is sky or deep ground, which the generator classifies without sampling).
fn bricks() -> Vec<(i32, i32, i32)> {
    let mut bricks = Vec::new();
    for z in -6..6 {
        for x in -6..6 {
            for y in -2..6 {
                bricks.push((x, y, z));
            }
        }
    }
    bricks
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// The brick holds one value all through (sky, or deep ground). It is *not* always made without sampling: the fuel and the time show that a good part of
    /// these bricks are sampled (the bounds do not classify every one), so they cost about a quarter of a mixed brick, not nothing.
    Uniform,
    /// A brick with a surface or a cave in it.
    Mixed,
}

#[derive(Default, Clone, Copy)]
struct Tally {
    count: u32,
    call: Duration,
    copy: Duration,
    worst_call: Duration,
}

impl Tally {
    fn add(&mut self, call: Duration, copy: Duration) {
        self.count += 1;
        self.call += call;
        self.copy += copy;
        self.worst_call = self.worst_call.max(call);
    }
}

trait Runner {
    /// Makes the brick and returns it as read out by the host.
    fn generate(&mut self, brick: (i32, i32, i32), out: &mut [u32]) -> (Duration, Duration);
    fn fuel_used_since_last_call(&mut self) -> Option<u64> {
        None
    }
    /// The size of the module's linear memory after the run.
    fn memory_bytes(&self) -> Option<usize> {
        None
    }
}

struct Native(SpikeGenerator);

impl Runner for Native {
    fn generate(&mut self, (x, y, z): (i32, i32, i32), out: &mut [u32]) -> (Duration, Duration) {
        let started = Instant::now();
        self.0.materials(x, y, z, out);
        (started.elapsed(), Duration::ZERO)
    }
}

fn read_u32s(bytes: &[u8], out: &mut [u32]) {
    for (slot, chunk) in out.iter_mut().zip(bytes.chunks_exact(4)) {
        *slot = u32::from_le_bytes(chunk.try_into().expect("four bytes"));
    }
}

struct Wasmtime {
    store: wasmtime::Store<()>,
    generate: wasmtime::TypedFunc<(i32, i32, i32), ()>,
    memory: wasmtime::Memory,
    address: usize,
    fuel: bool,
    fuel_before: u64,
}

impl Wasmtime {
    /// `nan`: every NaN a float operation makes is made the one canonical NaN, so that the bits of a NaN cannot differ between machines (it costs a check after
    /// float operations). `pulley`: the portable interpreter of wasmtime instead of machine code, for a platform where code cannot be made executable.
    fn new(bytes: &[u8], fuel: bool, nan: bool, pulley: bool) -> (Self, Durations) {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(fuel);
        config.cranelift_nan_canonicalization(nan);
        if pulley {
            config.target("pulley64").expect("the pulley target is built in");
        }
        let engine = wasmtime::Engine::new(&config).expect("engine");
        let started = Instant::now();
        let module = wasmtime::Module::new(&engine, bytes).expect("module compiles");
        let compile = started.elapsed();
        let mut store = wasmtime::Store::new(&engine, ());
        if fuel {
            store.set_fuel(u64::MAX).expect("fuel");
        }
        let started = Instant::now();
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("instance");
        let instantiate = started.elapsed();
        let init = instance
            .get_typed_func::<(u32, u32), u32>(&mut store, "init")
            .expect("init");
        let started = Instant::now();
        let address = init
            .call(&mut store, (SEED as u32, (SEED >> 32) as u32))
            .expect("init runs") as usize;
        let init_time = started.elapsed();
        let generate = instance
            .get_typed_func::<(i32, i32, i32), ()>(&mut store, "generate")
            .expect("generate");
        let memory = instance.get_memory(&mut store, "memory").expect("memory");
        let fuel_before = if fuel { store.get_fuel().expect("fuel") } else { 0 };
        (
            Self {
                store,
                generate,
                memory,
                address,
                fuel,
                fuel_before,
            },
            Durations {
                compile,
                instantiate,
                init: init_time,
            },
        )
    }
}

impl Runner for Wasmtime {
    fn memory_bytes(&self) -> Option<usize> {
        Some(self.memory.data_size(&self.store))
    }

    fn generate(&mut self, (x, y, z): (i32, i32, i32), out: &mut [u32]) -> (Duration, Duration) {
        let started = Instant::now();
        self.generate.call(&mut self.store, (x, y, z)).expect("generate runs");
        let call = started.elapsed();
        let started = Instant::now();
        let data = self.memory.data(&self.store);
        read_u32s(&data[self.address..self.address + BYTES_PER_BRICK], out);
        (call, started.elapsed())
    }

    fn fuel_used_since_last_call(&mut self) -> Option<u64> {
        if !self.fuel {
            return None;
        }
        let now = self.store.get_fuel().expect("fuel");
        let used = self.fuel_before - now;
        self.fuel_before = now;
        Some(used)
    }
}

struct Wasmi {
    store: wasmi::Store<()>,
    generate: wasmi::TypedFunc<(i32, i32, i32), ()>,
    memory: wasmi::Memory,
    address: usize,
    fuel: bool,
    fuel_before: u64,
}

impl Wasmi {
    fn new(bytes: &[u8], fuel: bool) -> (Self, Durations) {
        let mut config = wasmi::Config::default();
        config.consume_fuel(fuel);
        let engine = wasmi::Engine::new(&config);
        let started = Instant::now();
        let module = wasmi::Module::new(&engine, bytes).expect("module translates");
        let compile = started.elapsed();
        let mut store = wasmi::Store::new(&engine, ());
        if fuel {
            store.set_fuel(u64::MAX / 2).expect("fuel");
        }
        let linker = wasmi::Linker::<()>::new(&engine);
        let started = Instant::now();
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .expect("instance");
        let instantiate = started.elapsed();
        let init = instance
            .get_typed_func::<(u32, u32), u32>(&store, "init")
            .expect("init");
        let started = Instant::now();
        let address = init
            .call(&mut store, (SEED as u32, (SEED >> 32) as u32))
            .expect("init runs") as usize;
        let init_time = started.elapsed();
        let generate = instance
            .get_typed_func::<(i32, i32, i32), ()>(&store, "generate")
            .expect("generate");
        let memory = instance.get_memory(&store, "memory").expect("memory");
        let fuel_before = if fuel { store.get_fuel().expect("fuel") } else { 0 };
        (
            Self {
                store,
                generate,
                memory,
                address,
                fuel,
                fuel_before,
            },
            Durations {
                compile,
                instantiate,
                init: init_time,
            },
        )
    }
}

impl Runner for Wasmi {
    fn memory_bytes(&self) -> Option<usize> {
        Some(self.memory.data(&self.store).len())
    }

    fn generate(&mut self, (x, y, z): (i32, i32, i32), out: &mut [u32]) -> (Duration, Duration) {
        let started = Instant::now();
        self.generate.call(&mut self.store, (x, y, z)).expect("generate runs");
        let call = started.elapsed();
        let started = Instant::now();
        let data = self.memory.data(&self.store);
        read_u32s(&data[self.address..self.address + BYTES_PER_BRICK], out);
        (call, started.elapsed())
    }

    fn fuel_used_since_last_call(&mut self) -> Option<u64> {
        if !self.fuel {
            return None;
        }
        let now = self.store.get_fuel().expect("fuel");
        let used = self.fuel_before - now;
        self.fuel_before = now;
        Some(used)
    }
}

struct Durations {
    compile: Duration,
    instantiate: Duration,
    init: Duration,
}

fn micros(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e6
}

struct Report {
    name: String,
    uniform: Tally,
    mixed: Tally,
    differing_bricks: u32,
    fuel_per_mixed_brick: Option<u64>,
    fuel_per_uniform_brick: Option<u64>,
}

fn measure(
    name: &str,
    runner: &mut dyn Runner,
    bricks: &[(i32, i32, i32)],
    classes: &[Class],
    reference: &mut SpikeGenerator,
    passes: usize,
) -> Report {
    let mut out = vec![0u32; BRICK_VOXEL_COUNT];
    let mut expected = vec![0u32; BRICK_VOXEL_COUNT];
    let mut report = Report {
        name: name.to_string(),
        uniform: Tally::default(),
        mixed: Tally::default(),
        differing_bricks: 0,
        fuel_per_mixed_brick: None,
        fuel_per_uniform_brick: None,
    };
    for pass in 0..passes {
        let last = pass + 1 == passes;
        let (mut uniform, mut mixed) = (Tally::default(), Tally::default());
        let (mut fuel_uniform, mut fuel_mixed) = (0u64, 0u64);
        let mut differing = 0;
        runner.fuel_used_since_last_call();
        for (&brick, &class) in bricks.iter().zip(classes) {
            let (call, copy) = runner.generate(brick, &mut out);
            let fuel = runner.fuel_used_since_last_call();
            match class {
                Class::Uniform => {
                    uniform.add(call, copy);
                    fuel_uniform += fuel.unwrap_or(0);
                }
                Class::Mixed => {
                    mixed.add(call, copy);
                    fuel_mixed += fuel.unwrap_or(0);
                }
            }
            if last {
                // The exact check, not timed: the native brick again, voxel for voxel.
                reference.materials(brick.0, brick.1, brick.2, &mut expected);
                if out != expected {
                    differing += 1;
                }
            }
        }
        if last {
            report.uniform = uniform;
            report.mixed = mixed;
            report.differing_bricks = differing;
            if runner.fuel_used_since_last_call().is_some() || fuel_uniform + fuel_mixed > 0 {
                report.fuel_per_uniform_brick = Some(fuel_uniform / u64::from(uniform.count.max(1)));
                report.fuel_per_mixed_brick = Some(fuel_mixed / u64::from(mixed.count.max(1)));
            }
        }
    }
    if let Some(bytes) = runner.memory_bytes() {
        println!("  {name}: linear memory {} KiB after the run", bytes / 1024);
    }
    report
}

fn print(report: &Report, native: Option<&Report>) {
    let per = |tally: &Tally, which: fn(&Tally) -> Duration| micros(which(tally)) / f64::from(tally.count.max(1));
    let call_mixed = per(&report.mixed, |t| t.call);
    let ratio = native.map_or(String::from("      -"), |native| {
        format!("{:>7.2}", call_mixed / per(&native.mixed, |t| t.call))
    });
    println!(
        "{:<22} mixed: call {:>9.0} us (worst {:>9.0}) copy {:>5.0} us   x native {}   uniform: call {:>6.1} us copy {:>5.0} us   differing bricks {}   fuel/mixed {}  fuel/uniform {}",
        report.name,
        call_mixed,
        micros(report.mixed.worst_call),
        per(&report.mixed, |t| t.copy),
        ratio,
        per(&report.uniform, |t| t.call),
        per(&report.uniform, |t| t.copy),
        report.differing_bricks,
        report.fuel_per_mixed_brick.map_or("-".into(), |fuel| fuel.to_string()),
        report.fuel_per_uniform_brick.map_or("-".into(), |fuel| fuel.to_string()),
    );
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let path = arguments.next().expect("the path of the .wasm file");
    let passes: usize = arguments.next().map_or(2, |text| text.parse().expect("a count"));
    // Which runtimes to run, as words in one argument ("wasmtime", "wasmi"); the default is both.
    let only = arguments.next().unwrap_or_else(|| "wasmtime wasmi".to_string());
    let bytes = std::fs::read(&path).expect("the module can be read");
    let bricks = bricks();

    // Which bricks have a surface in them, from the native generator.
    let mut reference = SpikeGenerator::new(SEED);
    let mut scratch = vec![0u32; BRICK_VOXEL_COUNT];
    let classes: Vec<Class> = bricks
        .iter()
        .map(|&(x, y, z)| {
            reference.materials(x, y, z, &mut scratch);
            if scratch.iter().all(|&value| value == scratch[0]) {
                Class::Uniform
            } else {
                Class::Mixed
            }
        })
        .collect();
    let mixed = classes.iter().filter(|&&class| class == Class::Mixed).count();
    println!(
        "module {} bytes; {} bricks ({} with a surface or cave, {} uniform); {} passes, the last one reported",
        bytes.len(),
        bricks.len(),
        mixed,
        bricks.len() - mixed,
        passes
    );

    let mut native = Native(SpikeGenerator::new(SEED));
    let native_report = measure("native", &mut native, &bricks, &classes, &mut reference, passes);
    print(&native_report, None);

    // (fuel, canonical NaNs, pulley, the word that selects it, the label)
    let configurations = [
        (false, false, false, "wasmtime", "wasmtime"),
        (true, false, false, "wasmtime", "wasmtime + fuel"),
        (false, true, false, "nan", "wasmtime + nan"),
        (true, true, false, "nan", "wasmtime + fuel + nan"),
        (false, false, true, "pulley", "pulley"),
        (true, true, true, "pulley", "pulley + fuel + nan"),
    ];
    for (fuel, nan, pulley, word, label) in configurations {
        if !only.contains(word) {
            continue;
        }
        let (mut runner, durations) = Wasmtime::new(&bytes, fuel, nan, pulley);
        println!(
            "  {label}: compile {:.1} ms, instantiate {:.0} us, init {:.0} us",
            micros(durations.compile) / 1e3,
            micros(durations.instantiate),
            micros(durations.init)
        );
        let report = measure(label, &mut runner, &bricks, &classes, &mut reference, passes);
        print(&report, Some(&native_report));
    }
    for fuel in [false, true].into_iter().filter(|_| only.contains("wasmi")) {
        let label = if fuel { " + fuel" } else { "" };
        let (mut runner, durations) = Wasmi::new(&bytes, fuel);
        println!(
            "  wasmi{label}: translate {:.1} ms, instantiate {:.0} us, init {:.0} us",
            micros(durations.compile) / 1e3,
            micros(durations.instantiate),
            micros(durations.init)
        );
        let report = measure(
            &format!("wasmi{label}"),
            &mut runner,
            &bricks,
            &classes,
            &mut reference,
            passes,
        );
        print(&report, Some(&native_report));
    }
}
