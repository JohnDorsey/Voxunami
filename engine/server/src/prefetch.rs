//! A worker thread that makes the bricks of planets before they are read, so that a tick does not wait for the generator (a brick of ground costs 6 to 9 ms).

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use terrain::TerrainGenerator;
use voxel_grid::{Brick, GridLayout};

use crate::generated_cache::BrickKey;

struct Job {
    key: BrickKey,
    generator: Arc<dyn TerrainGenerator>,
    layout: Arc<GridLayout>,
}

struct Worker {
    jobs: Sender<Job>,
    made: Receiver<(BrickKey, Brick)>,
}

/// The bricks that have been asked for and the worker that makes them, started when the first is asked for. The worker ends when this is dropped (its
/// jobs are no more), after the brick it is making.
#[derive(Default)]
pub(crate) struct Prefetcher {
    worker: Option<Worker>,
    asked: BTreeSet<BrickKey>,
}

impl std::fmt::Debug for Prefetcher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Prefetcher")
            .field("asked", &self.asked.len())
            .finish_non_exhaustive()
    }
}

impl Prefetcher {
    /// Asks for a brick, unless it has been asked for and not yet taken.
    pub(crate) fn ask(
        &mut self,
        key: BrickKey,
        generator: &Arc<dyn TerrainGenerator>,
        layout: &Arc<GridLayout>,
    ) {
        if !self.asked.insert(key) {
            return;
        }
        let worker = self.worker.get_or_insert_with(|| {
            let (jobs, wanted) = channel::<Job>();
            let (sender, made) = channel();
            std::thread::Builder::new()
                .name("brick prefetch".to_owned())
                .spawn(move || {
                    while let Ok(job) = wanted.recv() {
                        let brick = job.generator.generate_brick(&job.layout, job.key.1);
                        if sender.send((job.key, brick)).is_err() {
                            break;
                        }
                    }
                })
                .expect("a thread can be started");
            Worker { jobs, made }
        });
        let job = Job {
            key,
            generator: Arc::clone(generator),
            layout: Arc::clone(layout),
        };
        if worker.jobs.send(job).is_err() {
            self.asked.remove(&key);
        }
    }

    /// The bricks the worker has finished since the last time, which may be asked for again.
    pub(crate) fn finished(&mut self) -> Vec<(BrickKey, Brick)> {
        let Some(worker) = &self.worker else {
            return Vec::new();
        };
        let made: Vec<(BrickKey, Brick)> = worker.made.try_iter().collect();
        for (key, _) in &made {
            self.asked.remove(key);
        }
        made
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::{BrickCoordinate, GridId};

    use super::*;

    #[derive(Debug)]
    struct Empty;

    impl TerrainGenerator for Empty {
        fn generate_brick(&self, layout: &GridLayout, _: BrickCoordinate) -> Brick {
            Brick::new(layout)
        }
    }

    fn wait_for_one(prefetcher: &mut Prefetcher) -> Vec<(BrickKey, Brick)> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let made = prefetcher.finished();
            if !made.is_empty() {
                return made;
            }
            assert!(Instant::now() < deadline, "the worker did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_brick_is_made_once_while_it_is_asked_for_and_can_be_asked_for_again_once_it_is_taken() {
        let layout = StandardTestLayout::default().layout;
        let generator: Arc<dyn TerrainGenerator> = Arc::new(Empty);
        let key = (
            GridId::new(GridId::FIRST_ALLOCATED),
            BrickCoordinate::new(1, 2, 3),
        );
        let mut prefetcher = Prefetcher::default();
        // Nothing is asked: nothing is made, and no worker is started.
        assert!(prefetcher.finished().is_empty());
        prefetcher.ask(key, &generator, &layout);
        prefetcher.ask(key, &generator, &layout);
        let made = wait_for_one(&mut prefetcher);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].0, key);
        std::thread::sleep(Duration::from_millis(20));
        assert!(prefetcher.finished().is_empty(), "asked twice, made once");
        // Taken, it can be asked for again (the cache may have forgotten it).
        prefetcher.ask(key, &generator, &layout);
        assert_eq!(wait_for_one(&mut prefetcher).len(), 1);
    }
}
