use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use grid_bodies::collision::BlockCollisions;
use namespaced_id::NamespacedId;
use player_presence::{PlayerPose, SharedPresence, lock};
use plugin_host::{
    Context, HandlerSpec, Phase, PlayerJoined, PlayerLeft, Plugin, PromptAnswered, PromptId,
    Registrar, SystemSpec,
};
use protocol::ClientId;
use server::{ServerWorld, ShipSettings};
use voxel_grid::ChannelIndex;
use voxel_space::{BrickBox, GridId, LevelIndex, Orientation, WorldPosition};
use world_storage::MovingGrid;

use crate::arrival::{self, Hull, MOVE_RADIUS, Obstacles};
use crate::distance::{squared_distance, squared_distance_to_bricks};

/// How many ticks a ship that could not be put away or brought in waits before it is tried again (the file may have been mended, or the
/// disk freed), so that a disk that fails is not asked every tick.
const TICKS_BEFORE_ANOTHER_TRY: u64 = 200;

/// The question a player is asked when the ship they were aboard cannot be found.
const LOST_TITLE: &str = "The ship you were aboard cannot be found.";
const LOST_OPTIONS: [&str; 3] = [
    "Go to the world spawn",
    "Appear in the air where you were",
    "Quit to the title",
];

/// The question a player is asked when the place their ship was left at is taken. The options are numbered from 0 in the answers.
const BLOCKED_TITLE: &str = "Something is where your ship was left.";
const BLOCKED_OPTIONS: [&str; 5] = [
    "Move the ship to the nearest free place",
    "Break the terrain that is in the way",
    "Break the blocks of the ship that are in the way",
    "Put the ship there anyway (it may be glitchy)",
    "Go to the world spawn and leave the ship behind",
];
const MOVE_SHIP: usize = 0;
const BREAK_TERRAIN: usize = 1;
const BREAK_SHIP: usize = 2;
const SPAWN_IN_TERRAIN: usize = 3;
const WARP_TO_SPAWN: usize = 4;
const GO_TO_SPAWN: usize = 0;
const APPEAR_IN_THE_AIR: usize = 1;
const QUIT_TO_THE_TITLE: usize = 2;

/// What the lifecycle has done, for whoever runs the server and for tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LifecycleReport {
    /// Ships that were frozen, and ships that were let go again.
    pub frozen: u64,
    pub thawed: u64,
    /// Ships that were put in files, and ships that were brought out of them.
    pub parked: u64,
    pub brought_in: u64,
    /// Why ships could not be put away or brought in, in the order it happened (each reason once).
    pub problems: Vec<String>,
    /// How many players are being asked what to do about a ship (and have not said).
    pub waiting: usize,
}

/// What it takes to ask where a ship that went away with its pilot lands when the pilot comes back: what the blocks of the game
/// are to something that rests on them.
#[derive(Debug)]
pub struct ArrivalConfig {
    pub collisions: BlockCollisions,
    pub block_channel: ChannelIndex,
    pub state_channel: ChannelIndex,
    /// Where the game keeps the orientation of its blocks, if it has any.
    pub orientation_channel: Option<ChannelIndex>,
}

/// Keeps ships by how near the players are: see the crate documentation.
#[derive(Debug)]
pub struct GridLifecyclePlugin {
    presence: SharedPresence,
    /// Where a player goes who chooses the world's spawn, in the level they were in.
    spawn: WorldPosition,
    arrivals: Mutex<Option<ArrivalConfig>>,
    state: Arc<Mutex<State>>,
}

/// A player who is waiting for an answer about their ship.
#[derive(Debug)]
struct Pending {
    ship: GridId,
    prompt: PromptId,
    /// The ship as it was read from its file, for a question about where it lands. The ship is still in its file, unchanged.
    landing: Option<MovingGrid>,
}

#[derive(Debug, Default)]
struct State {
    report: LifecycleReport,
    /// The tick a ship that failed was last tried at.
    failed_at: BTreeMap<GridId, u64>,
    arrivals: Option<ArrivalConfig>,
    pending: BTreeMap<ClientId, Pending>,
    reasons: BTreeSet<String>,
}

impl State {
    /// Notes why something could not be done, once.
    fn problem(&mut self, text: String) {
        note(&mut self.report, &mut self.reasons, text);
    }
}

/// Notes in a report why something could not be done, once.
fn note(report: &mut LifecycleReport, reasons: &mut BTreeSet<String>, text: String) {
    if reasons.insert(text.clone()) {
        report.problems.push(text);
    }
}

/// What the plugin has done, and who it is waiting for.
fn report_of(state: &State) -> LifecycleReport {
    LifecycleReport {
        waiting: state.pending.len(),
        ..state.report.clone()
    }
}

fn locked(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl GridLifecyclePlugin {
    /// `spawn` is where a player goes who chooses the world's spawn when their ship cannot be found, in the level they were in.
    pub fn new(presence: SharedPresence, spawn: WorldPosition) -> Self {
        Self {
            presence,
            spawn,
            arrivals: Mutex::new(None),
            state: Arc::default(),
        }
    }

    /// With this, a ship that went away with its pilot is looked at when the pilot comes back, and if it would land in something the pilot
    /// is asked what to do. Without it such a ship is simply brought in where it was.
    pub fn with_arrivals(self, config: ArrivalConfig) -> Self {
        *self
            .arrivals
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(config);
        self
    }

    /// What the plugin has done so far.
    pub fn report(&self) -> LifecycleReport {
        report_of(&locked(&self.state))
    }

    /// A handle to the report, which stays usable after the plugin moves into a server.
    pub fn report_handle(&self) -> impl Fn() -> LifecycleReport + Send + Sync + 'static {
        let state = Arc::clone(&self.state);
        move || report_of(&locked(&state))
    }
}

impl Plugin for GridLifecyclePlugin {
    fn id(&self) -> NamespacedId {
        "engine:grid_lifecycle"
            .parse()
            .expect("constant identifier is valid")
    }

    fn depends_on(&self) -> Vec<NamespacedId> {
        vec![
            "player_presence:players"
                .parse()
                .expect("constant identifier is valid"),
        ]
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        locked(&self.state).arrivals = self
            .arrivals
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();

        // A player who joins aboard a ship that is in a file has it brought in at once, before anything places them in it: or is asked
        // what to do, if it cannot be, or it would land in something.
        let (presence, state) = (Arc::clone(&self.presence), Arc::clone(&self.state));
        registrar.on::<PlayerJoined>(
            HandlerSpec::new("engine:grid_lifecycle_bring_in_the_ship_of_a_player_who_joins")
                .after("player_presence:take_over_the_saved_player"),
            move |event, context| arrive(context, &presence, &state, event.client),
        );

        // The answer to a question about a ship.
        let (presence, state, spawn) = (
            Arc::clone(&self.presence),
            Arc::clone(&self.state),
            self.spawn,
        );
        registrar.on::<PromptAnswered>(
            HandlerSpec::new("engine:grid_lifecycle_hear_what_to_do_with_a_ship"),
            move |event, context| answered(context, &presence, &state, spawn, event),
        );

        // A player who goes is no longer asked.
        let state = Arc::clone(&self.state);
        registrar.on::<PlayerLeft>(
            HandlerSpec::new("engine:grid_lifecycle_forget_the_question_of_a_player_who_goes"),
            move |event, context| {
                if let Some(pending) = locked(&state).pending.remove(&event.client) {
                    context.cancel_prompt(event.client, pending.prompt);
                }
            },
        );

        // Before anything moves.
        let (presence, state) = (Arc::clone(&self.presence), Arc::clone(&self.state));
        registrar.system(
            SystemSpec::new("engine:grid_lifecycle_update", Phase::Controllers),
            move |context| update(context.world(), &presence, &state),
        );
    }
}

/// A player has joined: if they were aboard a ship that is in a file, it comes in (after a look at where it lands, if it went away with
/// them), and if that cannot be done they are asked what to do.
fn arrive(
    context: &mut Context<'_>,
    presence: &SharedPresence,
    state: &Mutex<State>,
    client: ClientId,
) {
    let Some(frame) = lock(presence).frame_of(context.world().data(), client) else {
        return;
    };
    let Some(parked) = context.world().data().grids().get_parked(frame).cloned() else {
        return;
    };
    let mut guard = locked(state);
    let State {
        report,
        arrivals,
        pending,
        reasons,
        ..
    } = &mut *guard;
    // A ship that went away with its pilot, with something to check where it lands against, is looked at before it is brought in.
    let Some(arrivals) = arrivals.as_ref().filter(|_| parked.waits_for_those_aboard) else {
        match context.world().load_ship(frame) {
            Ok(()) => report.brought_in += 1,
            Err(error) => {
                note(
                    report,
                    reasons,
                    format!("{frame} could not be brought in: {error}"),
                );
                ask_lost(context, pending, client, frame);
            }
        }
        return;
    };
    let ship = match context.world().read_parked_ship(frame) {
        Ok(ship) => ship,
        Err(error) => {
            note(
                report,
                reasons,
                format!("{frame} could not be brought in: {error}"),
            );
            ask_lost(context, pending, client, frame);
            return;
        }
    };
    let hull = Hull::of(
        &ship,
        &arrivals.collisions,
        arrivals.block_channel,
        arrivals.state_channel,
        arrivals.orientation_channel,
    );
    let free = {
        let mut obstacles = Obstacles::new(
            context.world(),
            parked.level,
            arrivals.block_channel,
            arrivals.state_channel,
            arrivals.orientation_channel,
            &arrivals.collisions,
        );
        hull.is_free(&ship.pose, &mut |place| obstacles.solid_at(place))
    };
    if free {
        match context.world().admit_ship(frame, ship) {
            Ok(()) => report.brought_in += 1,
            Err(error) => note(
                report,
                reasons,
                format!("{frame} could not be brought in: {error}"),
            ),
        }
        return;
    }
    ask_blocked(context, pending, client, frame, ship, None);
}

/// Asks a player what to do about a ship that cannot be found.
fn ask_lost(
    context: &mut Context<'_>,
    pending: &mut BTreeMap<ClientId, Pending>,
    client: ClientId,
    ship: GridId,
) {
    if let Ok(prompt) = context.prompt(client, LOST_TITLE, &LOST_OPTIONS) {
        pending.insert(
            client,
            Pending {
                ship,
                prompt,
                landing: None,
            },
        );
    }
}

/// Asks a player what to do about a ship that would land in something, saying more if there is more to say.
fn ask_blocked(
    context: &mut Context<'_>,
    pending: &mut BTreeMap<ClientId, Pending>,
    client: ClientId,
    ship: GridId,
    landing: MovingGrid,
    note: Option<&str>,
) {
    let title = match note {
        Some(note) => format!("{BLOCKED_TITLE} {note}"),
        None => BLOCKED_TITLE.to_owned(),
    };
    if let Ok(prompt) = context.prompt(client, &title, &BLOCKED_OPTIONS) {
        pending.insert(
            client,
            Pending {
                ship,
                prompt,
                landing: Some(landing),
            },
        );
    }
}

/// Puts a player in the level, at a place, upright and looking as they did, and tells them where they are.
fn put_in_the_level(
    context: &mut Context<'_>,
    presence: &SharedPresence,
    client: ClientId,
    level: LevelIndex,
    position: WorldPosition,
) {
    let mut presence = lock(presence);
    let world = context.world();
    let Some((_, saved, _)) = presence.local_pose_of(world.data(), client) else {
        return;
    };
    let pose = PlayerPose { position, ..saved };
    let _ = presence.set_pose_in(
        world,
        client,
        GridId::root(level),
        pose,
        pose.view_orientation(Orientation::IDENTITY),
    );
    presence.tell_placed(world, client);
}

/// A player has answered a question about a ship.
fn answered(
    context: &mut Context<'_>,
    presence: &SharedPresence,
    state: &Mutex<State>,
    spawn: WorldPosition,
    event: &PromptAnswered,
) {
    let mut guard = locked(state);
    let State {
        report,
        arrivals,
        pending,
        reasons,
        ..
    } = &mut *guard;
    if pending
        .get(&event.client)
        .is_none_or(|waiting| waiting.prompt != event.prompt)
    {
        return;
    }
    let mut waiting = pending
        .remove(&event.client)
        .expect("the player was just found waiting");
    let ship = waiting.ship;
    // The ship may have come in since, for someone else's answer: then there is nothing to decide.
    let Some(parked) = context.world().data().grids().get_parked(ship).cloned() else {
        return;
    };
    let (client, level) = (event.client, parked.level);
    let here = lock(presence).pose_of(context.world().data(), client);

    let Some(mut landing) = waiting.landing.take() else {
        // A ship that could not be found.
        match event.choice {
            GO_TO_SPAWN => put_in_the_level(context, presence, client, level, spawn),
            APPEAR_IN_THE_AIR => {
                if let Some(here) = here {
                    put_in_the_level(context, presence, client, level, here.position);
                }
            }
            QUIT_TO_THE_TITLE => context
                .world()
                .disconnect_client(client, "you chose to quit"),
            _ => {}
        }
        return;
    };

    // A ship that would land in something.
    let Some(arrivals) = arrivals.as_ref() else {
        return;
    };
    let hull = Hull::of(
        &landing,
        &arrivals.collisions,
        arrivals.block_channel,
        arrivals.state_channel,
        arrivals.orientation_channel,
    );
    let mut obstacles = Obstacles::new(
        context.world(),
        level,
        arrivals.block_channel,
        arrivals.state_channel,
        arrivals.orientation_channel,
        &arrivals.collisions,
    );
    let blocked = hull.blocked_cells(&landing.pose, &mut |place| obstacles.solid_at(place));
    match event.choice {
        MOVE_SHIP => {
            let found = hull.nearest_free_pose(&landing.pose, MOVE_RADIUS, &mut |place| {
                obstacles.solid_at(place)
            });
            drop(obstacles);
            match found {
                Some(pose) => landing.pose = pose,
                None => {
                    context.reply(
                        client,
                        &format!("there is no free place for the ship within {MOVE_RADIUS} voxels"),
                    );
                    let note =
                        format!("There is no free place for it within {MOVE_RADIUS} voxels.");
                    ask_blocked(context, pending, client, ship, landing, Some(&note));
                    return;
                }
            }
        }
        BREAK_TERRAIN => {
            let in_the_way = obstacles.terrain_in_the_way(&hull, &blocked, &landing.pose);
            drop(obstacles);
            arrival::break_terrain(context.world(), level, &in_the_way);
        }
        BREAK_SHIP => {
            drop(obstacles);
            arrival::break_cells(&mut landing, &hull, &blocked);
        }
        SPAWN_IN_TERRAIN => drop(obstacles),
        WARP_TO_SPAWN => {
            drop(obstacles);
            // The ship is left where it is, and is nobody's to wait for any more: whoever comes near it finds it.
            let _ = context.world().set_parked_ship_waits(ship, false);
            put_in_the_level(context, presence, client, level, spawn);
            return;
        }
        _ => return,
    }
    match context.world().admit_ship(ship, landing) {
        Ok(()) => {
            report.brought_in += 1;
            // Whoever else was asked about this ship has no question any more.
            let others: Vec<ClientId> = pending
                .iter()
                .filter(|(_, waiting)| waiting.ship == ship)
                .map(|(other, _)| *other)
                .collect();
            for other in others {
                if let Some(waiting) = pending.remove(&other) {
                    context.cancel_prompt(other, waiting.prompt);
                }
            }
        }
        Err(error) => note(
            report,
            reasons,
            format!("{ship} could not be brought in: {error}"),
        ),
    }
}

/// What to do with a ship this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Freeze,
    Thaw,
    Park,
    BringIn,
}

/// The square of the distance to the nearest of the players in the level, infinite for a level with none.
fn squared_distance_to_players(
    players: &[(WorldPosition, LevelIndex)],
    level: LevelIndex,
    distance: impl Fn(WorldPosition) -> f64,
) -> f64 {
    players
        .iter()
        .filter(|(_, in_level)| *in_level == level)
        .map(|(position, _)| distance(*position))
        .fold(f64::INFINITY, f64::min)
}

/// One tick: every ship is looked at once, in order of id, and does what the distance of the nearest player asks of its state.
fn update(world: &mut ServerWorld, presence: &SharedPresence, state: &Mutex<State>) {
    let settings: ShipSettings = *world.ship_settings();
    let players: Vec<(WorldPosition, LevelIndex)> = lock(presence)
        .players(world.data())
        .into_iter()
        .map(|(_, pose, level)| (pose.position, level))
        .collect();
    let squared = |distance: f64| distance * distance;
    let simulate = squared(settings.simulation_distance);
    let stop = squared(settings.simulation_distance * ShipSettings::HYSTERESIS);
    let keep = squared(settings.unload_distance * ShipSettings::HYSTERESIS);

    let mut actions: Vec<(GridId, Action)> = Vec::new();
    for (id, ship) in world.data().grids().iter() {
        let bounds: Option<BrickBox> = ship.world_brick_bounds();
        let away = squared_distance_to_players(&players, ship.level, |player| match bounds {
            Some(bounds) => squared_distance_to_bricks(player, bounds),
            None => squared_distance(player, ship.pose.position),
        });
        if !ship.frozen {
            if away > stop {
                actions.push((id, Action::Freeze));
            }
        } else if away <= simulate {
            actions.push((id, Action::Thaw));
        } else if away > keep && ship.terrain.is_none() {
            // (A planet is asleep when no one is near, and is never put away: a client draws it from as far as there is anything to see of it.)
            actions.push((id, Action::Park));
        }
    }
    for (id, parked) in world.data().grids().parked() {
        // A ship that went away with someone waits for them.
        if parked.waits_for_those_aboard {
            continue;
        }
        let away =
            squared_distance_to_players(&players, parked.level, |player| match parked.bounds {
                Some(bounds) => squared_distance_to_bricks(player, bounds),
                None => squared_distance(player, parked.pose.position),
            });
        if away <= simulate {
            actions.push((id, Action::BringIn));
        }
    }

    let tick = world.tick().get();
    let mut state = locked(state);
    for (id, action) in actions {
        let retry_at = state
            .failed_at
            .get(&id)
            .map(|failed| failed + TICKS_BEFORE_ANOTHER_TRY);
        if matches!(action, Action::Park | Action::BringIn) && retry_at.is_some_and(|at| tick < at)
        {
            continue;
        }
        match action {
            Action::Freeze => {
                if world.set_grid_frozen(id, true).is_ok() {
                    state.report.frozen += 1;
                }
            }
            Action::Thaw => {
                if world.set_grid_frozen(id, false).is_ok() {
                    state.report.thawed += 1;
                }
            }
            Action::Park => match world.park_ship(id, false) {
                Ok(()) => state.report.parked += 1,
                Err(error) => {
                    state.failed_at.insert(id, tick);
                    state.problem(format!("{id} could not be put away: {error}"));
                }
            },
            Action::BringIn => match world.load_ship(id) {
                Ok(()) => {
                    state.failed_at.remove(&id);
                    state.report.brought_in += 1;
                }
                Err(error) => {
                    state.failed_at.insert(id, tick);
                    state.problem(format!("{id} could not be brought in: {error}"));
                }
            },
        }
    }
}
