//! Keeps the bricks around a point generated and meshed, on background threads.
//!
//! Each [`WorldStreamer::update`] call collects finished work, unloads bricks that are too far
//! away, and hands out new jobs nearest-first. A brick is only meshed once it and all six of its
//! neighbours exist, so no mesh is ever built against a missing neighbour and no seam appears.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

use terrain::TerrainGenerator;
use voxel_grid::{Brick, ChannelIndex, Grid, GridLayout};
use voxel_meshing::{BrickNeighbourhood, PackedQuad, mesh_brick};
use voxel_space::{BrickCoordinate, FaceDirection};

/// How far to keep bricks loaded around the centre, and how much work to run at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamingSettings {
    /// Bricks within this distance (in brick units, as a sphere) are meshed.
    pub view_radius_bricks: i64,
    pub worker_count: usize,
}

/// A change the renderer must apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshUpdate {
    Upload {
        coordinate: BrickCoordinate,
        quads: Vec<PackedQuad>,
    },
    Remove(BrickCoordinate),
}

struct NeighbourhoodSnapshot {
    center: Arc<Brick>,
    /// Indexed by [`FaceDirection::index`].
    neighbours: [Arc<Brick>; 6],
}

enum Job {
    Generate(BrickCoordinate),
    Mesh(BrickCoordinate, Box<NeighbourhoodSnapshot>),
}

enum JobResult {
    Generated(BrickCoordinate, Arc<Brick>),
    Meshed(BrickCoordinate, Vec<PackedQuad>),
}

/// Integer offsets within `radius` of the origin, nearest first.
fn offsets_within(radius: i64) -> Vec<[i64; 3]> {
    let mut offsets = Vec::new();
    for x in -radius..=radius {
        for y in -radius..=radius {
            for z in -radius..=radius {
                if x * x + y * y + z * z <= radius * radius {
                    offsets.push([x, y, z]);
                }
            }
        }
    }
    offsets.sort_by_key(|[x, y, z]| (x * x + y * y + z * z, *x, *y, *z));
    offsets
}

fn distance_squared(a: BrickCoordinate, b: BrickCoordinate) -> i128 {
    let component = |p: i64, q: i64| i128::from(p) - i128::from(q);
    let (dx, dy, dz) = (
        component(a.x(), b.x()),
        component(a.y(), b.y()),
        component(a.z(), b.z()),
    );
    dx * dx + dy * dy + dz * dz
}

pub struct WorldStreamer {
    settings: StreamingSettings,
    grid: Grid,
    generating: HashSet<BrickCoordinate>,
    meshing: HashSet<BrickCoordinate>,
    meshed: HashSet<BrickCoordinate>,
    /// Offsets of bricks to generate: the view sphere plus a one-brick shell for neighbours.
    generation_offsets: Vec<[i64; 3]>,
    jobs: Option<mpsc::Sender<Job>>,
    results: mpsc::Receiver<JobResult>,
    workers: Vec<JoinHandle<()>>,
    scheduled_last_update: usize,
}

impl WorldStreamer {
    /// `air_value` is the material value that does not hide faces behind it.
    pub fn new(
        settings: StreamingSettings,
        layout: Arc<GridLayout>,
        material_channel: ChannelIndex,
        air_value: u32,
        generator: Arc<dyn TerrainGenerator>,
    ) -> Self {
        let (job_sender, job_receiver) = mpsc::channel::<Job>();
        let (result_sender, results) = mpsc::channel::<JobResult>();
        let job_receiver = Arc::new(Mutex::new(job_receiver));
        let workers = (0..settings.worker_count.max(1))
            .map(|_| {
                let job_receiver = Arc::clone(&job_receiver);
                let result_sender = result_sender.clone();
                let layout = Arc::clone(&layout);
                let generator = Arc::clone(&generator);
                std::thread::spawn(move || {
                    loop {
                        // Hold the lock only while waiting for the next job, not while running it.
                        let job = job_receiver.lock().expect("job queue lock").recv();
                        let Ok(job) = job else { return };
                        let result = match job {
                            Job::Generate(coordinate) => JobResult::Generated(
                                coordinate,
                                Arc::new(generator.generate_brick(&layout, coordinate)),
                            ),
                            Job::Mesh(coordinate, snapshot) => {
                                let neighbourhood = BrickNeighbourhood {
                                    center: snapshot.center.channel(material_channel),
                                    neighbours: std::array::from_fn(|face| {
                                        Some(snapshot.neighbours[face].channel(material_channel))
                                    }),
                                };
                                let quads = mesh_brick(&neighbourhood, |value| value != air_value)
                                    .iter()
                                    .map(|quad| quad.pack())
                                    .collect();
                                JobResult::Meshed(coordinate, quads)
                            }
                        };
                        if result_sender.send(result).is_err() {
                            return;
                        }
                    }
                })
            })
            .collect();
        Self {
            settings,
            grid: Grid::new(layout),
            generating: HashSet::new(),
            meshing: HashSet::new(),
            meshed: HashSet::new(),
            generation_offsets: offsets_within(settings.view_radius_bricks + 1),
            jobs: Some(job_sender),
            results,
            workers,
            scheduled_last_update: 0,
        }
    }

    /// Jobs handed to workers and not yet finished.
    pub fn pending_jobs(&self) -> usize {
        self.generating.len() + self.meshing.len()
    }

    /// Bricks currently held in memory (generated, whether or not drawn).
    pub fn loaded_brick_count(&self) -> usize {
        self.grid.brick_count()
    }

    /// Collects finished jobs, unloads far bricks and schedules nearby work around `center`.
    /// Returns what the renderer should change.
    pub fn update(&mut self, center: BrickCoordinate) -> Vec<MeshUpdate> {
        let mut updates = Vec::new();
        let view_radius = self.settings.view_radius_bricks;
        let keep_radius_squared = i128::from((view_radius + 2) * (view_radius + 2));

        while let Ok(result) = self.results.try_recv() {
            match result {
                JobResult::Generated(coordinate, brick) => {
                    self.generating.remove(&coordinate);
                    self.grid
                        .insert_brick(coordinate, brick)
                        .expect("generated bricks have the grid's layout");
                }
                JobResult::Meshed(coordinate, quads) => {
                    self.meshing.remove(&coordinate);
                    self.meshed.insert(coordinate);
                    if !quads.is_empty() {
                        updates.push(MeshUpdate::Upload { coordinate, quads });
                    }
                }
            }
        }

        // Unload what is out of range. A removal is reported for every meshed brick, including
        // ones that were empty, which the renderer simply ignores.
        let too_far: Vec<BrickCoordinate> = self
            .grid
            .bricks()
            .map(|(coordinate, _)| coordinate)
            .filter(|&coordinate| distance_squared(coordinate, center) > keep_radius_squared)
            .collect();
        for coordinate in too_far {
            self.grid.remove_brick(coordinate);
            if self.meshed.remove(&coordinate) {
                updates.push(MeshUpdate::Remove(coordinate));
            }
        }

        let maximum_in_flight = self.workers.len() * 4;
        let mut scheduled = 0;
        let sender = self
            .jobs
            .as_ref()
            .expect("workers run until the streamer is dropped");
        for &[dx, dy, dz] in &self.generation_offsets {
            if self.generating.len() + self.meshing.len() >= maximum_in_flight {
                break;
            }
            let Some(coordinate) = center.checked_offset(dx, dy, dz) else {
                continue;
            };
            if self.grid.brick(coordinate).is_none() && !self.generating.contains(&coordinate) {
                self.generating.insert(coordinate);
                sender
                    .send(Job::Generate(coordinate))
                    .expect("workers are running");
                scheduled += 1;
                continue;
            }
            let within_view = dx * dx + dy * dy + dz * dz <= view_radius * view_radius;
            if !within_view
                || self.meshed.contains(&coordinate)
                || self.meshing.contains(&coordinate)
            {
                continue;
            }
            if let Some(snapshot) = self.snapshot_of(coordinate) {
                self.meshing.insert(coordinate);
                sender
                    .send(Job::Mesh(coordinate, Box::new(snapshot)))
                    .expect("workers are running");
                scheduled += 1;
            }
        }
        self.scheduled_last_update = scheduled;
        updates
    }

    /// The brick and its six neighbours, if all of them have been generated.
    fn snapshot_of(&self, coordinate: BrickCoordinate) -> Option<NeighbourhoodSnapshot> {
        let center = Arc::clone(self.grid.brick(coordinate)?);
        let mut neighbours = Vec::with_capacity(6);
        for face in FaceDirection::ALL {
            let neighbour = coordinate.neighbour(face)?;
            neighbours.push(Arc::clone(self.grid.brick(neighbour)?));
        }
        Some(NeighbourhoodSnapshot {
            center,
            neighbours: neighbours.try_into().expect("exactly six faces"),
        })
    }

    /// Updates until no work is pending or schedulable, returning every change. For headless
    /// rendering and tests; an interactive viewer calls [`WorldStreamer::update`] once per frame.
    pub fn settle(&mut self, center: BrickCoordinate) -> Vec<MeshUpdate> {
        let mut updates = Vec::new();
        loop {
            updates.extend(self.update(center));
            if self.pending_jobs() == 0 && self.scheduled_last_update == 0 {
                return updates;
            }
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
    }
}

impl Drop for WorldStreamer {
    fn drop(&mut self) {
        // Closing the job queue ends each worker's loop.
        self.jobs = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use terrain::{FieldTerrainGenerator, FractalNoise, Heightfield, TerrainMaterials};
    use voxel_grid::ChannelDefinition;
    use voxel_meshing::Quad;

    use super::*;

    const MATERIALS: TerrainMaterials = TerrainMaterials {
        air: 0,
        stone: 1,
        dirt: 2,
        grass: 3,
    };

    fn brick(x: i64, y: i64, z: i64) -> BrickCoordinate {
        BrickCoordinate::new(x, y, z)
    }

    fn streamer(radius: i64, ground_height: f64) -> WorldStreamer {
        let layout = Arc::new(
            GridLayout::new(vec![ChannelDefinition::number(
                "viewer:material".parse().unwrap(),
                0,
            )])
            .unwrap(),
        );
        let material = layout
            .channel_index(&"viewer:material".parse().unwrap())
            .unwrap();
        let generator = Arc::new(FieldTerrainGenerator::new(
            Arc::new(Heightfield::new(ground_height, FractalNoise::none())),
            material,
            MATERIALS,
        ));
        WorldStreamer::new(
            StreamingSettings {
                view_radius_bricks: radius,
                worker_count: 3,
            },
            layout,
            material,
            0,
            generator,
        )
    }

    /// Applies updates the way a renderer would, returning the bricks currently shown.
    fn apply(live: &mut HashMap<BrickCoordinate, Vec<PackedQuad>>, updates: Vec<MeshUpdate>) {
        for update in updates {
            match update {
                MeshUpdate::Upload { coordinate, quads } => {
                    live.insert(coordinate, quads);
                }
                MeshUpdate::Remove(coordinate) => {
                    live.remove(&coordinate);
                }
            }
        }
    }

    #[test]
    fn offsets_are_a_sphere_listed_nearest_first() {
        assert_eq!(offsets_within(0), [[0, 0, 0]]);
        assert_eq!(offsets_within(1).len(), 7);
        assert_eq!(offsets_within(2).len(), 33);
        let offsets = offsets_within(5);
        assert_eq!(offsets[0], [0, 0, 0]);
        let distances: Vec<i64> = offsets
            .iter()
            .map(|[x, y, z]| x * x + y * y + z * z)
            .collect();
        assert!(distances.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(distances.iter().all(|&distance| distance <= 25));
    }

    #[test]
    fn flat_ground_streams_in_exactly_one_top_quad_per_surface_brick() {
        let mut streamer = streamer(3, 20.0);
        let mut live = HashMap::new();
        apply(&mut live, streamer.settle(brick(0, 0, 0)));

        // Surface bricks (y = 0) within 3 bricks horizontally: integer points with x^2 + z^2 <= 9.
        let expected: Vec<BrickCoordinate> = (-3..=3)
            .flat_map(|x| (-3..=3).map(move |z| (x, z)))
            .filter(|(x, z)| x * x + z * z <= 9)
            .map(|(x, z)| brick(x, 0, z))
            .collect();
        assert_eq!(expected.len(), 29);
        let mut shown: Vec<BrickCoordinate> = live.keys().copied().collect();
        shown.sort();
        let mut expected_sorted = expected.clone();
        expected_sorted.sort();
        assert_eq!(shown, expected_sorted);

        // One 32 x 32 grass quad on top and nothing else: any border faces would reveal a mesh
        // built before its neighbours existed.
        for coordinate in expected {
            let quads: Vec<Quad> = live[&coordinate].iter().map(|quad| quad.unpack()).collect();
            assert_eq!(
                quads,
                [Quad {
                    origin: [0, 19, 0],
                    width: 32,
                    height: 32,
                    face: FaceDirection::PositiveY,
                    material: MATERIALS.grass
                }],
                "{coordinate}"
            );
        }
    }

    #[test]
    fn a_settled_streamer_has_nothing_more_to_do() {
        let mut streamer = streamer(2, 20.0);
        streamer.settle(brick(0, 0, 0));
        assert_eq!(streamer.pending_jobs(), 0);
        assert!(streamer.update(brick(0, 0, 0)).is_empty());
        assert_eq!(streamer.scheduled_last_update, 0);
    }

    #[test]
    fn moving_away_unloads_old_bricks_and_loads_new_ones() {
        let mut streamer = streamer(3, 20.0);
        let mut live = HashMap::new();
        apply(&mut live, streamer.settle(brick(0, 0, 0)));
        let loaded_before = streamer.loaded_brick_count();
        assert_eq!(live.len(), 29);

        apply(&mut live, streamer.settle(brick(10, 0, 0)));

        assert_eq!(
            live.len(),
            29,
            "only the area around the new centre remains"
        );
        assert!(
            live.keys()
                .all(|coordinate| (coordinate.x() - 10).pow(2) + coordinate.z().pow(2) <= 9)
        );
        assert!(
            streamer.loaded_brick_count() <= loaded_before + 40,
            "old bricks should have been unloaded"
        );
        assert!(streamer.loaded_brick_count() > 100);
    }

    #[test]
    fn nothing_is_drawn_where_the_world_is_all_sky() {
        let mut streamer = streamer(2, 20.0);
        let updates = streamer.settle(brick(0, 40, 0));
        assert!(updates.is_empty());
    }

    #[test]
    fn dropping_the_streamer_stops_its_workers() {
        let mut streamer = streamer(2, 20.0);
        streamer.update(brick(0, 0, 0));
        drop(streamer); // must not hang
    }
}
