//! What the plugin host does, checked through a real server and the pretend game of
//! `world_storage::test_support`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use namespaced_id::NamespacedId;
use plugin_host::{
    CASCADE_LIMIT, CommandSpec, Event, HandlerSpec, Phase, PlayerJoined, PlayerLeft, Plugin,
    PluginHost, Registrar, RegistrationError, SaveScope, SystemSpec, Verdict,
};
use protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage};
use server::{GameLogic, GridView, Server, ServerConfig};
use transport::Connection;
use transport::loopback::{LoopbackConnection, loopback_listener};
use voxel_grid::ChannelIndex;
use voxel_space::{GridId, VoxelCoordinate};
use world_storage::Section;
use world_storage::test_support::{Game, Plugin as FixturePlugin, id, sample_world};

type Log = Arc<Mutex<Vec<String>>>;

fn new_log() -> Log {
    Arc::new(Mutex::new(Vec::new()))
}

fn note(log: &Log, text: impl Into<String>) {
    log.lock().unwrap().push(text.into());
}

fn lines(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

type Registration = dyn Fn(&mut Registrar<'_>) + Send + Sync;

/// A plugin that is whatever a closure registers.
struct Scripted {
    id: NamespacedId,
    depends_on: Vec<NamespacedId>,
    register: Box<Registration>,
}

impl Plugin for Scripted {
    fn id(&self) -> NamespacedId {
        self.id.clone()
    }

    fn depends_on(&self) -> Vec<NamespacedId> {
        self.depends_on.clone()
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        (self.register)(registrar);
    }
}

fn plugin(
    name: &str,
    register: impl Fn(&mut Registrar<'_>) + Send + Sync + 'static,
) -> Box<dyn Plugin> {
    Box::new(Scripted {
        id: id(name),
        depends_on: Vec::new(),
        register: Box::new(register),
    })
}

fn plugin_depending_on(
    name: &str,
    depends_on: &[&str],
    register: impl Fn(&mut Registrar<'_>) + Send + Sync + 'static,
) -> Box<dyn Plugin> {
    Box::new(Scripted {
        id: id(name),
        depends_on: depends_on.iter().map(|name| id(name)).collect(),
        register: Box::new(register),
    })
}

struct Rig {
    server: Server,
    connector: transport::loopback::LoopbackConnector,
    block: ChannelIndex,
}

struct Client {
    connection: LoopbackConnection,
}

impl Client {
    fn send(&mut self, message: &ClientMessage) {
        self.connection.send(message.encode()).unwrap();
    }

    fn replies(&mut self) -> Vec<String> {
        let mut replies = Vec::new();
        while let Some(frame) = self.connection.receive().unwrap_or(None) {
            if let Ok(ServerMessage::CommandReply { text }) = ServerMessage::decode(&frame) {
                replies.push(text);
            }
        }
        replies
    }
}

fn game() -> Game {
    Game::new(&[FixturePlugin::BaseBlocks])
}

fn build(
    make: impl FnOnce(ChannelIndex, ChannelIndex) -> Vec<Box<dyn Plugin>>,
) -> Result<Rig, RegistrationError> {
    let game = game();
    let world = sample_world(&game);
    let layout = world.main().overrides.layout().clone();
    let block = layout.channel_index(&id("core:block")).unwrap();
    let temperature = layout.channel_index(&id("core:temperature")).unwrap();
    let host = PluginHost::new(make(block, temperature), &world)?;
    let (listener, connector) = loopback_listener();
    let server = Server::new(
        ServerConfig::new(id("test_game:game")),
        world,
        &game.registries(),
        Box::new(host),
        Box::new(listener),
    )
    .unwrap();
    Ok(Rig {
        server,
        connector,
        block,
    })
}

impl Rig {
    fn join(&mut self) -> Client {
        let mut client = Client {
            connection: self.connector.connect().unwrap(),
        };
        client.send(&ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            game: id("test_game:game"),
        });
        self.server.step();
        client
    }

    fn voxel(&mut self, channel: ChannelIndex, x: i64) -> u32 {
        self.server
            .world()
            .main()
            .overrides
            .get(channel, VoxelCoordinate::new(x, 100, 0))
    }
}

fn at(x: i64) -> VoxelCoordinate {
    VoxelCoordinate::new(x, 100, 0)
}

fn registration_error(plugins: Vec<Box<dyn Plugin>>) -> Option<RegistrationError> {
    let game = game();
    let world = sample_world(&game);
    PluginHost::new(plugins, &world).err()
}

// ---------------------------------------------------------------- the order of the tick

/// A plugin with one system, which says its name when it runs.
fn named_system(plugin_name: &'static str, spec: fn() -> SystemSpec, log: &Log) -> Box<dyn Plugin> {
    let log = Arc::clone(log);
    plugin(plugin_name, move |registrar| {
        let log = Arc::clone(&log);
        let name = registrar.plugin_id().to_string();
        registrar.system(spec(), move |_| note(&log, name.clone()));
    })
}

#[test]
fn systems_run_by_phase_then_by_what_they_declare_then_by_plugin_and_name() {
    let log = new_log();
    let plugins = vec![
        named_system(
            "test:rules_late",
            || SystemSpec::new("test:rules_late", Phase::Rules).after("test:rules_early"),
            &log,
        ),
        named_system(
            "test:rules_early",
            || SystemSpec::new("test:rules_early", Phase::Rules),
            &log,
        ),
        named_system(
            "test:settle",
            || SystemSpec::new("test:settle", Phase::Settle),
            &log,
        ),
        named_system(
            "test:physics",
            || SystemSpec::new("test:physics", Phase::Physics),
            &log,
        ),
        named_system(
            "test:decide",
            || SystemSpec::new("test:decide", Phase::Decide).before("test:controllers"),
            &log,
        ),
        named_system(
            "test:controllers",
            || SystemSpec::new("test:controllers", Phase::Controllers),
            &log,
        ),
        // Two with nothing between them: the plugin's name decides, not the order of listing.
        named_system(
            "test:z_free",
            || SystemSpec::new("test:z_free", Phase::Rules),
            &log,
        ),
        named_system(
            "test:a_free",
            || SystemSpec::new("test:a_free", Phase::Rules),
            &log,
        ),
    ];
    let mut rig = build(|_, _| plugins).unwrap();
    rig.server.step();
    assert_eq!(
        lines(&log),
        [
            "test:decide",
            "test:controllers",
            "test:physics",
            "test:settle",
            "test:a_free",
            "test:rules_early",
            "test:rules_late",
            "test:z_free"
        ]
    );
}

/// Plugins of every kind of registration, writing to one log, so that any order-dependence shows.
fn busy_plugins(log: &Log, block: ChannelIndex) -> Vec<Box<dyn Plugin>> {
    let mut plugins = Vec::new();
    for name in ["test:alpha", "test:beta", "test:gamma"] {
        let log = Arc::clone(log);
        plugins.push(plugin(name, move |registrar| {
            let short = registrar.plugin_id().to_string();
            let system_log = Arc::clone(&log);
            let system_name = short.clone();
            // Each runs after the one before in the alphabet, so that the writes of all of them to the one
            // channel are ordered.
            let mut spec =
                SystemSpec::new(&format!("{short}_system"), Phase::Rules).writes(&[block]);
            match short.as_str() {
                "test:beta" => spec = spec.after("test:alpha_system"),
                "test:gamma" => spec = spec.after("test:beta_system"),
                _ => {}
            }
            registrar.system(spec, move |context| {
                note(&system_log, format!("{system_name} system"));
                // Everyone writes a different voxel, which every handler hears of.
                let x = i64::from(system_name.len() as u8);
                context
                    .grid(GridId::MAIN)
                    .unwrap()
                    .set(block, at(x), 5)
                    .unwrap();
            });
            let handler_log = Arc::clone(&log);
            let handler_name = short.clone();
            registrar.on_channels(
                HandlerSpec::new(&format!("{short}_hears")),
                &[block],
                move |event, _| {
                    note(
                        &handler_log,
                        format!("{handler_name} heard {:?}", event.voxels),
                    )
                },
            );
            let join_log = Arc::clone(&log);
            let join_name = short.clone();
            registrar
                .on::<PlayerJoined>(HandlerSpec::new(&format!("{short}_greets")), move |_, _| {
                    note(&join_log, format!("{join_name} greets"))
                });
        }));
    }
    plugins
}

#[test]
fn the_order_does_not_depend_on_the_order_the_plugins_are_listed() {
    let runs: Vec<Vec<String>> = (0..6)
        .map(|rotation| {
            let log = new_log();
            let mut rig = build(|block, _| {
                let mut plugins = busy_plugins(&log, block);
                match rotation {
                    0 => {}
                    1 => plugins.reverse(),
                    2 => plugins.rotate_left(1),
                    3 => plugins.rotate_right(1),
                    4 => plugins.swap(0, 1),
                    _ => plugins.swap(1, 2),
                }
                plugins
            })
            .unwrap();
            let _client = rig.join();
            rig.server.step();
            lines(&log)
        })
        .collect();
    assert!(runs[0].len() > 6, "{:?}", runs[0]);
    for run in &runs[1..] {
        assert_eq!(*run, runs[0]);
    }
}

// ---------------------------------------------------------------- registration is checked

#[test]
fn plugins_that_cannot_be_registered_are_refused_with_the_reason() {
    let nothing = |_: &mut Registrar<'_>| {};
    // Two plugins with one name.
    assert_eq!(
        registration_error(vec![plugin("test:a", nothing), plugin("test:a", nothing)]),
        Some(RegistrationError::DuplicatePlugin(id("test:a")))
    );
    // A dependency that is not there, and a cycle of dependencies.
    assert_eq!(
        registration_error(vec![plugin_depending_on(
            "test:a",
            &["test:missing"],
            nothing
        )]),
        Some(RegistrationError::UnknownDependency {
            plugin: id("test:a"),
            dependency: id("test:missing")
        })
    );
    let cycle = registration_error(vec![
        plugin_depending_on("test:a", &["test:b"], nothing),
        plugin_depending_on("test:b", &["test:a"], nothing),
    ]);
    assert!(
        matches!(cycle, Some(RegistrationError::DependencyCycle(ref names)) if names.len() == 2)
    );
    // A message with two handlers, and a command registered twice.
    let message = |registrar: &mut Registrar<'_>| registrar.message(id("test:kind"), |_, _, _| {});
    assert_eq!(
        registration_error(vec![plugin("test:a", message), plugin("test:b", message)]),
        Some(RegistrationError::DuplicateMessage(id("test:kind")))
    );
    let command = |registrar: &mut Registrar<'_>| {
        registrar.command(
            CommandSpec {
                name: "go",
                usage: "/go",
                summary: "goes",
            },
            |_, _, _| {},
        );
    };
    assert_eq!(
        registration_error(vec![plugin("test:a", command), plugin("test:b", command)]),
        Some(RegistrationError::DuplicateCommand("go".to_owned()))
    );
}

#[test]
fn systems_and_handlers_that_cannot_be_ordered_are_refused_with_the_reason() {
    let system = |spec: fn() -> SystemSpec| {
        plugin("test:p", move |registrar| registrar.system(spec(), |_| {}))
    };
    assert_eq!(
        registration_error(vec![system(
            || SystemSpec::new("test:a", Phase::Rules).after("test:nothing")
        )]),
        Some(RegistrationError::UnknownOrderingTarget {
            item: id("test:a"),
            relation: "after",
            target: id("test:nothing")
        })
    );
    // Two systems with the same name.
    assert_eq!(
        registration_error(vec![plugin("test:p", |registrar| {
            registrar.system(SystemSpec::new("test:a", Phase::Rules), |_| {});
            registrar.system(SystemSpec::new("test:a", Phase::Rules), |_| {});
        }),]),
        Some(RegistrationError::DuplicateSystem(id("test:a")))
    );
    // A cycle among systems of one phase.
    let cycle = registration_error(vec![plugin("test:p", |registrar| {
        registrar.system(
            SystemSpec::new("test:a", Phase::Rules).after("test:b"),
            |_| {},
        );
        registrar.system(
            SystemSpec::new("test:b", Phase::Rules).after("test:a"),
            |_| {},
        );
    })]);
    assert!(matches!(cycle, Some(RegistrationError::OrderingCycle(ref names)) if names.len() == 2));
    // An edge that runs against the phases: a Rules system that must run before a Physics one.
    assert_eq!(
        registration_error(vec![plugin("test:p", |registrar| {
            registrar.system(
                SystemSpec::new("test:rules", Phase::Rules).before("test:physics"),
                |_| {},
            );
            registrar.system(SystemSpec::new("test:physics", Phase::Physics), |_| {});
        })]),
        Some(RegistrationError::OrderAgainstPhase {
            item: id("test:rules"),
            relation: "before",
            target: id("test:physics")
        })
    );
    // Handlers of the same event with a name that is not one of them, and two with one name.
    assert_eq!(
        registration_error(vec![plugin("test:p", |registrar| {
            registrar.on::<PlayerLeft>(HandlerSpec::new("test:h").after("test:other"), |_, _| {});
        })]),
        Some(RegistrationError::UnknownOrderingTarget {
            item: id("test:h"),
            relation: "after",
            target: id("test:other")
        })
    );
    assert_eq!(
        registration_error(vec![plugin("test:p", |registrar| {
            registrar.on::<PlayerLeft>(HandlerSpec::new("test:h"), |_, _| {});
            registrar.on::<PlayerJoined>(HandlerSpec::new("test:h"), |_, _| {});
        })]),
        Some(RegistrationError::DuplicateHandler(id("test:h")))
    );
}

#[test]
fn two_systems_that_use_one_channel_with_no_order_between_them_are_refused_unless_both_only_read() {
    let game = game();
    let world = sample_world(&game);
    let block = world
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    let pair = move |first: SystemSpec, second: SystemSpec| {
        let (first, second) = (first.clone(), second.clone());
        vec![
            plugin("test:one", move |registrar| {
                registrar.system(first.clone(), |_| {})
            }),
            plugin("test:two", move |registrar| {
                registrar.system(second.clone(), |_| {})
            }),
        ]
    };
    let write = move |name: &str| SystemSpec::new(name, Phase::Rules).writes(&[block]);
    let read = |name: &str| SystemSpec::new(name, Phase::Rules).reads(&[block]);
    let refused = |plugins| PluginHost::new(plugins, &world).err();
    assert!(matches!(
        refused(pair(write("test:a"), write("test:b"))),
        Some(RegistrationError::AmbiguousOrder { channel, .. }) if channel == block
    ));
    assert!(matches!(
        refused(pair(write("test:a"), read("test:b"))),
        Some(RegistrationError::AmbiguousOrder { .. })
    ));
    assert!(refused(pair(read("test:a"), read("test:b"))).is_none());
    // An order between them, directly or through a third, settles it.
    assert!(refused(pair(write("test:a"), write("test:b").after("test:a"))).is_none());
    let chain = vec![plugin("test:one", move |registrar| {
        registrar.system(write("test:a"), |_| {});
        registrar.system(
            SystemSpec::new("test:middle", Phase::Rules).after("test:a"),
            |_| {},
        );
        registrar.system(write("test:b").after("test:middle"), |_| {});
    })];
    assert!(refused(chain).is_none());
}

#[test]
fn a_handler_for_an_event_nobody_defined_is_refused() {
    struct Mine;
    impl Event for Mine {
        fn id() -> NamespacedId {
            id("test:mine")
        }
    }
    assert_eq!(
        registration_error(vec![plugin("test:p", |registrar| {
            registrar.on::<Mine>(HandlerSpec::new("test:h"), |_, _| {});
        })]),
        Some(RegistrationError::UnknownEvent {
            handler: id("test:h"),
            event: id("test:mine")
        })
    );
    // Defined, it is accepted; defined twice, it is not.
    assert_eq!(
        registration_error(vec![plugin("test:p", |registrar| {
            registrar.event::<Mine>();
            registrar.on::<Mine>(HandlerSpec::new("test:h"), |_, _| {});
        })]),
        None
    );
    assert_eq!(
        registration_error(vec![plugin("test:p", |registrar| {
            registrar.event::<Mine>();
            registrar.event::<Mine>();
        })]),
        Some(RegistrationError::DuplicateEvent(id("test:mine")))
    );
}

// ---------------------------------------------------------------- events

#[test]
fn what_a_system_writes_is_heard_before_the_next_system_runs_in_the_same_tick() {
    // The way fire and light work together in Cave: fire writes, the light is told, the light runs.
    let log = new_log();
    let mut rig = build(|block, _| {
        let fire_log = Arc::clone(&log);
        let hear_log = Arc::clone(&log);
        let light_log = Arc::clone(&log);
        vec![
            plugin("test:fire", move |registrar| {
                let log = Arc::clone(&fire_log);
                registrar.system(
                    SystemSpec::new("test:fire", Phase::Rules).writes(&[block]),
                    move |context| {
                        note(&log, "fire writes");
                        let mut grid = context.grid(GridId::MAIN).unwrap();
                        grid.set(block, at(3), 7).unwrap();
                        grid.set(block, at(4), 7).unwrap();
                        grid.set(block, at(4), 7).unwrap(); // not a change
                    },
                );
            }),
            plugin("test:light", move |registrar| {
                let hear = Arc::clone(&hear_log);
                registrar.on_channels(
                    HandlerSpec::new("test:light_wakes"),
                    &[block],
                    move |event, _| {
                        note(
                            &hear,
                            format!("light wakes {:?} {:?}", event.grid, event.voxels),
                        );
                    },
                );
                let log = Arc::clone(&light_log);
                registrar.system(
                    SystemSpec::new("test:light", Phase::Rules)
                        .reads(&[block])
                        .after("test:fire"),
                    move |_| note(&log, "light runs"),
                );
            }),
        ]
    })
    .unwrap();
    rig.server.step();
    assert_eq!(
        lines(&log),
        [
            "fire writes",
            &format!("light wakes {:?} {:?}", GridId::MAIN, [at(3), at(4)]),
            "light runs"
        ]
    );
}

#[test]
fn only_the_channels_somebody_asked_for_are_reported() {
    let log = new_log();
    let mut rig = build(|block, temperature| {
        let hear = Arc::clone(&log);
        vec![plugin("test:p", move |registrar| {
            let hear = Arc::clone(&hear);
            registrar.on_channels(HandlerSpec::new("test:hears"), &[block], move |event, _| {
                note(&hear, format!("{:?}", event.channel));
            });
            registrar.system(
                SystemSpec::new("test:writes", Phase::Rules).writes(&[block, temperature]),
                move |context| {
                    let mut grid = context.grid(GridId::MAIN).unwrap();
                    grid.set(temperature, at(1), 3).unwrap();
                    grid.set(block, at(1), 3).unwrap();
                },
            );
        })]
    })
    .unwrap();
    let block = rig.block;
    rig.server.step();
    assert_eq!(lines(&log), [format!("{block:?}")]);
}

struct Ping;
impl Event for Ping {
    fn id() -> NamespacedId {
        id("test:ping")
    }
}

#[test]
fn what_a_handler_sends_or_writes_waits_behind_the_events_already_waiting_and_so_for_the_other_handlers_of_its_event()
 {
    let log = new_log();
    let mut rig = build(|block, _| {
        let log = Arc::clone(&log);
        vec![plugin("test:p", move |registrar| {
            registrar.event::<Ping>();
            let first = Arc::clone(&log);
            registrar.on::<Ping>(HandlerSpec::new("test:first"), move |_, context| {
                note(&first, "first handles ping");
                context
                    .grid(GridId::MAIN)
                    .unwrap()
                    .set(block, at(9), 1)
                    .unwrap();
            });
            let second = Arc::clone(&log);
            registrar.on::<Ping>(
                HandlerSpec::new("test:second").after("test:first"),
                move |_, _| note(&second, "second handles ping"),
            );
            let heard = Arc::clone(&log);
            registrar.on_channels(HandlerSpec::new("test:heard"), &[block], move |_, _| {
                note(&heard, "write heard");
            });
            let sender = Arc::clone(&log);
            registrar.system(
                SystemSpec::new("test:sender", Phase::Rules).writes(&[block]),
                move |context| {
                    note(&sender, "ping sent");
                    context.emit(Ping);
                },
            );
        })]
    })
    .unwrap();
    rig.server.step();
    assert_eq!(
        lines(&log),
        [
            "ping sent",
            "first handles ping",
            "second handles ping",
            "write heard"
        ]
    );
}

#[test]
fn a_loop_of_events_stops_at_the_limit_and_the_next_tick_is_free_to_run_again() {
    let count = Arc::new(Mutex::new(0usize));
    let mut rig = build(|block, _| {
        let count = Arc::clone(&count);
        vec![plugin("test:p", move |registrar| {
            let handler_count = Arc::clone(&count);
            // Every change makes another, one voxel further on: it never ends by itself.
            registrar.on_channels(
                HandlerSpec::new("test:loops"),
                &[block],
                move |event, context| {
                    *handler_count.lock().unwrap() += 1;
                    let next = at(event.voxels[0].x() + 1);
                    context
                        .grid(GridId::MAIN)
                        .unwrap()
                        .set(block, next, 2)
                        .unwrap();
                },
            );
            registrar.system(
                SystemSpec::new("test:starts", Phase::Rules).writes(&[block]),
                move |context| {
                    context
                        .grid(GridId::MAIN)
                        .unwrap()
                        .set(block, at(0), 2)
                        .unwrap();
                },
            );
        })]
    })
    .unwrap();
    rig.server.step();
    assert_eq!(*count.lock().unwrap(), CASCADE_LIMIT);
    // The tick ended, and the next one starts afresh (the system writes a voxel that is the same as
    // before, so nothing is heard; it is free to run, and the server has not hung).
    rig.server.step();
    assert_eq!(*count.lock().unwrap(), CASCADE_LIMIT);
}

struct Move(u32);
impl Event for Move {
    fn id() -> NamespacedId {
        id("test:move")
    }
}

#[test]
fn the_first_handler_in_order_to_refuse_wins_and_none_refusing_allows() {
    let verdicts = Arc::new(Mutex::new(Vec::new()));
    let mut rig = build(|_, _| {
        let verdicts = Arc::clone(&verdicts);
        vec![
            plugin("test:a", |registrar| {
                registrar.event::<Move>();
                registrar.on_ask::<Move>(HandlerSpec::new("test:a_checks"), |event, _| {
                    if event.0 > 100 {
                        Verdict::Refuse("a: too far".to_owned())
                    } else {
                        Verdict::Allow
                    }
                });
            }),
            plugin("test:b", |registrar| {
                registrar.on_ask::<Move>(HandlerSpec::new("test:b_checks"), |event, _| {
                    if event.0 > 10 {
                        Verdict::Refuse("b: too far".to_owned())
                    } else {
                        Verdict::Allow
                    }
                });
            }),
            plugin("test:asker", move |registrar| {
                let verdicts = Arc::clone(&verdicts);
                registrar.system(SystemSpec::new("test:asks", Phase::Rules), move |context| {
                    for distance in [5, 50, 500] {
                        verdicts.lock().unwrap().push(context.ask(&Move(distance)));
                    }
                });
            }),
        ]
    })
    .unwrap();
    rig.server.step();
    assert_eq!(
        *verdicts.lock().unwrap(),
        [
            Verdict::Allow,
            Verdict::Refuse("b: too far".to_owned()),
            Verdict::Refuse("a: too far".to_owned()),
        ]
    );
}

#[test]
fn an_event_nobody_answers_is_allowed() {
    let verdict = Arc::new(Mutex::new(None));
    let mut rig = build(|_, _| {
        let verdict = Arc::clone(&verdict);
        vec![plugin("test:p", move |registrar| {
            registrar.event::<Move>();
            let verdict = Arc::clone(&verdict);
            registrar.system(SystemSpec::new("test:asks", Phase::Rules), move |context| {
                *verdict.lock().unwrap() = Some(context.ask(&Move(1)));
            });
        })]
    })
    .unwrap();
    rig.server.step();
    assert_eq!(*verdict.lock().unwrap(), Some(Verdict::Allow));
}

// ---------------------------------------------------------------- declared channels

#[test]
fn a_system_that_uses_a_channel_it_did_not_declare_fails() {
    let try_system = |reads: bool, declared_write: bool, declared_read: bool| {
        let mut rig = build(|block, temperature| {
            vec![plugin("test:p", move |registrar| {
                let mut spec = SystemSpec::new("test:s", Phase::Rules);
                if declared_write {
                    spec = spec.writes(&[block]);
                }
                if declared_read {
                    spec = spec.reads(&[block]);
                }
                let _ = temperature;
                registrar.system(spec, move |context| {
                    let mut grid = context.grid(GridId::MAIN).unwrap();
                    if reads {
                        let _ = grid.get(block, at(1));
                    } else {
                        let _ = grid.set(block, at(1), 4);
                    }
                });
            })]
        })
        .unwrap();
        catch_unwind(AssertUnwindSafe(|| {
            rig.server.step();
        }))
        .is_ok()
    };
    // The check is made in debug builds, which is what tests normally are; a release build does not make it.
    if !cfg!(debug_assertions) {
        return;
    }
    assert!(!try_system(false, false, false), "an undeclared write");
    assert!(
        !try_system(false, false, true),
        "a write declared only as a read"
    );
    assert!(!try_system(true, false, false), "an undeclared read");
    assert!(try_system(false, true, false), "a declared write");
    assert!(try_system(true, false, true), "a declared read");
    assert!(try_system(true, true, false), "a read of what it writes");
}

// ---------------------------------------------------------------- messages, commands, players

#[test]
fn a_message_reaches_its_handler_with_the_client_and_the_payload_and_unknown_kinds_are_ignored() {
    let log = new_log();
    let mut rig = build(|_, _| {
        let log = Arc::clone(&log);
        vec![plugin("test:p", move |registrar| {
            let log = Arc::clone(&log);
            registrar.message(id("test:hello"), move |_, client, payload| {
                note(&log, format!("{client:?} {payload:?}"));
            });
        })]
    })
    .unwrap();
    let mut client = rig.join();
    client.send(&ClientMessage::GameMessage {
        kind: id("test:hello"),
        payload: vec![1, 2, 3],
    });
    client.send(&ClientMessage::GameMessage {
        kind: id("test:other"),
        payload: vec![9],
    });
    rig.server.step();
    let heard = lines(&log);
    assert_eq!(heard.len(), 1);
    assert!(heard[0].ends_with("[1, 2, 3]"), "{heard:?}");
}

#[test]
fn a_command_gets_its_arguments_and_an_unknown_one_is_answered_with_the_usages() {
    let log = new_log();
    let mut rig = build(|_, _| {
        let log = Arc::clone(&log);
        vec![plugin("test:p", move |registrar| {
            let log = Arc::clone(&log);
            registrar.command(
                CommandSpec {
                    name: "tp",
                    usage: "/tp X Y Z",
                    summary: "teleports",
                },
                move |context, client, arguments| {
                    note(&log, format!("tp {arguments:?}"));
                    context.reply(client, "teleported");
                },
            );
            registrar.command(
                CommandSpec {
                    name: "fly",
                    usage: "/fly",
                    summary: "flies",
                },
                |_, _, _| {},
            );
        })]
    })
    .unwrap();
    let mut client = rig.join();
    for text in ["/tp 1 2 3", "/nonsense", "", "/"] {
        client.send(&ClientMessage::Command {
            text: text.to_owned(),
        });
    }
    rig.server.step();
    assert_eq!(lines(&log), ["tp [\"1\", \"2\", \"3\"]"]);
    assert_eq!(
        client.replies(),
        [
            "teleported",
            "unknown command; try /fly, /tp X Y Z",
            "unknown command; try /fly, /tp X Y Z",
            "unknown command; try /fly, /tp X Y Z",
        ]
    );
}

#[test]
fn a_game_with_no_commands_says_so() {
    let mut rig = build(|_, _| vec![plugin("test:p", |_| {})]).unwrap();
    let mut client = rig.join();
    client.send(&ClientMessage::Command {
        text: "/anything".to_owned(),
    });
    rig.server.step();
    assert_eq!(client.replies(), ["this game has no commands"]);
}

#[test]
fn players_joining_and_leaving_are_events() {
    let log = new_log();
    let mut rig = build(|_, _| {
        let log = Arc::clone(&log);
        vec![plugin("test:p", move |registrar| {
            let joined = Arc::clone(&log);
            registrar.on::<PlayerJoined>(HandlerSpec::new("test:joined"), move |event, _| {
                note(&joined, format!("joined {}", event.client.get()));
            });
            let left = Arc::clone(&log);
            registrar.on::<PlayerLeft>(HandlerSpec::new("test:left"), move |event, _| {
                note(&left, format!("left {}", event.client.get()));
            });
        })]
    })
    .unwrap();
    let mut client = rig.join();
    client.send(&ClientMessage::Goodbye);
    rig.server.step();
    let heard = lines(&log);
    assert_eq!(heard.len(), 2, "{heard:?}");
    assert!(heard[0].starts_with("joined ") && heard[1].starts_with("left "));
}

// ---------------------------------------------------------------- minds

#[test]
fn what_a_mind_asks_for_goes_through_the_handler_a_players_message_would() {
    let log = new_log();
    let mut rig = build(|block, _| {
        let handler_log = Arc::clone(&log);
        let decide_log = Arc::clone(&log);
        let rules_log = Arc::clone(&log);
        vec![plugin("test:p", move |registrar| {
            let handler_log = Arc::clone(&handler_log);
            // The handler is the one validated path: it writes the world, as a message from a client may.
            registrar.message(id("test:walk"), move |context, client, payload| {
                note(
                    &handler_log,
                    format!("walk {} for {}", payload[0], client.get()),
                );
                context
                    .grid(GridId::MAIN)
                    .unwrap()
                    .set(block, at(2), 6)
                    .unwrap();
            });
            let decide_log = Arc::clone(&decide_log);
            registrar.mind_system(
                SystemSpec::new("test:brain", Phase::Decide),
                move |context| {
                    note(
                        &decide_log,
                        format!("brain decides at {}", context.tick().get()),
                    );
                    context.push_intent(protocol::ClientId::new(7), id("test:walk"), vec![3]);
                },
            );
            let rules_log = Arc::clone(&rules_log);
            registrar.system(SystemSpec::new("test:rules", Phase::Rules), move |_| {
                note(&rules_log, "rules run");
            });
        })]
    })
    .unwrap();
    rig.server.step();
    let heard = lines(&log);
    assert_eq!(heard.len(), 3, "{heard:?}");
    assert!(heard[0].starts_with("brain decides"));
    assert_eq!(heard[1], "walk 3 for 7");
    assert_eq!(heard[2], "rules run");
    assert_eq!(rig.voxel(rig.block, 2), 6);
}

// ---------------------------------------------------------------- saving

#[test]
fn what_plugins_save_is_collected_with_its_scope() {
    let game = game();
    let world = sample_world(&game);
    let host = PluginHost::new(
        vec![
            plugin("test:a", |registrar| {
                registrar.save_section(SaveScope::Global, || {
                    vec![Section {
                        identifier: id("test:a_state"),
                        version: 1,
                        payload: vec![1],
                    }]
                });
            }),
            plugin("test:b", |registrar| {
                registrar.save_section(SaveScope::PerGrid { movable: true }, || {
                    vec![Section {
                        identifier: id("test:b_state"),
                        version: 1,
                        payload: vec![2, 2],
                    }]
                });
            }),
        ],
        &world,
    )
    .unwrap();
    let sections = host.save_sections();
    let names: Vec<String> = sections.iter().map(|s| s.identifier.to_string()).collect();
    assert_eq!(names, ["test:a_state", "test:b_state"]);
    // In the order of their names, whatever order the plugins were registered in.
    let reversed = PluginHost::new(
        vec![
            plugin("test:z", |registrar| {
                registrar.save_section(SaveScope::Global, || {
                    vec![Section {
                        identifier: id("test:zz_state"),
                        version: 1,
                        payload: vec![],
                    }]
                });
            }),
            plugin("test:a", |registrar| {
                registrar.save_section(SaveScope::Global, || {
                    vec![Section {
                        identifier: id("test:aa_state"),
                        version: 1,
                        payload: vec![],
                    }]
                });
            }),
        ],
        &world,
    )
    .unwrap();
    let names: Vec<String> = reversed
        .save_sections()
        .iter()
        .map(|s| s.identifier.to_string())
        .collect();
    assert_eq!(names, ["test:aa_state", "test:zz_state"]);
    assert_eq!(
        host.saved_state_scopes(),
        [SaveScope::Global, SaveScope::PerGrid { movable: true }]
    );
}

#[test]
fn a_plugin_is_given_the_world_it_starts_from() {
    let seen = Arc::new(Mutex::new(None));
    let game = game();
    let world = sample_world(&game);
    let seed = world.world_seed;
    let probe = Arc::clone(&seen);
    let _host = PluginHost::new(
        vec![plugin("test:p", move |registrar| {
            *probe.lock().unwrap() = Some(registrar.world().world_seed);
        })],
        &world,
    )
    .unwrap();
    assert_eq!(*seen.lock().unwrap(), Some(seed));
}
