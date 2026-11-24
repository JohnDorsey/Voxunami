//! The cubes a view draws from a place, and the thread that works them out for the place that is kept warm.
//!
//! The cubes are worked out from what the terrain says may show ([`crate::lod::CoarseTerrainSource::may_show`]), and that is a walk through the cubes of the terrain for
//! each of tens of thousands of them: 44 ms the first time a place was worked out, and 13 to 20 ms about once a second while it moved (measured 2026-11-22,
//! `docs/specs/viewpoints-spec.md` 9h), on the thread of the update, which draws the frame. The cubes of the place that is **drawn** are needed before the frame can be, and
//! are worked out where they were. The cubes of the place that is only **kept warm** are not needed by any frame, and are worked out here, on a thread of their own, from a
//! copy of what the terrain says ([`crate::lod::CoarseTerrainSource::detached_may_show`]) that was taken when the work was handed over.

use std::any::Any;
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::thread::JoinHandle;

use coarse_cells::{EditedCubes, ViewSelection, select_around_brick};
use region_octree::BrickCube;
use terrain::SolidBall;
use voxel_space::{BrickCoordinate, GridId, WorldPosition};

use crate::horizon::{VIEWER_MARGIN, is_hidden_by, voxels_of};
use crate::lod::{LodSettings, MayShowOracle, squared_distance_to_middle};
use crate::may_show_memo::MayShowMemo;

/// The cubes a view draws from a place, as they were worked out, with what they were worked out from: they are the cubes of another working out for the same place if
/// nothing here has changed, so that the select, which asks the terrain about tens of thousands of cubes, is not done again.
pub(crate) struct KeptCut {
    pub(crate) centre: BrickCoordinate,
    pub(crate) settings: LodSettings,
    /// The version of the terrain ([`crate::lod::CoarseTerrainSource::version`]).
    pub(crate) version: u64,
    /// How many times the edits had changed (`LodView::edit_generation`).
    pub(crate) edits: u64,
    pub(crate) cubes: Vec<BrickCube>,
}

impl KeptCut {
    pub(crate) fn is_for(
        &self,
        centre: BrickCoordinate,
        settings: &LodSettings,
        version: u64,
        edits: u64,
    ) -> bool {
        self.centre == centre
            && self.settings == *settings
            && self.version == version
            && self.edits == edits
    }
}

/// The nodes of a cut that are to be made for a warm viewpoint, in the order the view takes them from the end of the queue: the coarsest first, the nearest of those first. The
/// bricks are the mesher's. (Sorted by a key worked out once for each cube: the distance is a sum of squares of 128-bit numbers, and a cut is tens of thousands of cubes.)
pub(crate) fn warm_queue_of(cubes: &[BrickCube], centre: BrickCoordinate) -> Vec<BrickCube> {
    let mut queue: Vec<BrickCube> = cubes
        .iter()
        .copied()
        .filter(|cube| cube.level() > 0)
        .collect();
    queue.sort_by_cached_key(|cube| {
        (
            cube.level(),
            std::cmp::Reverse(squared_distance_to_middle(*cube, centre)),
        )
    });
    queue
}

/// What the cubes of a view at a place are worked out from, besides what the terrain says may show.
pub(crate) struct Selection<'a> {
    pub(crate) centre: BrickCoordinate,
    pub(crate) viewer: WorldPosition,
    pub(crate) settings: &'a LodSettings,
    /// The version of the terrain the answers it gives are for.
    pub(crate) version: u64,
    pub(crate) grid: GridId,
    pub(crate) edited: &'a EditedCubes,
    pub(crate) core: Option<SolidBall>,
    /// Whether the cubes beside an edit are kept with it (not for a terrain that has only the nodes the server sends).
    pub(crate) draws_beside_edits: bool,
    /// How many bricks round the viewer are bricks (the mesher's radius).
    pub(crate) fine_radius: u32,
}

impl Selection<'_> {
    /// The cubes a view at the place draws: those the terrain says may show, and the ones an edit is in or beside, that the viewer is not hidden from by the core of the
    /// terrain. `memo` keeps what `may_show` said, for this working out and the next.
    pub(crate) fn select(
        &self,
        memo: &mut MayShowMemo,
        may_show: &dyn Fn(BrickCube) -> bool,
    ) -> Vec<BrickCube> {
        // A cube that is all air or buried is left out, unless an edit is in it or beside it; and one that is behind the horizon of the terrain's core.
        memo.keep_for(self.version);
        let (grid, edited, core) = (self.grid, self.edited, self.core);
        let viewer_in_voxels = voxels_of(self.viewer);
        let memo = RefCell::new(&mut *memo);
        let may_hold = |cube: BrickCube| {
            let may_show = memo.borrow_mut().get_or_ask(cube, || may_show(cube))
                || (self.draws_beside_edits && edited.near(grid, cube));
            may_show
                && !core
                    .is_some_and(|core| is_hidden_by(&core, viewer_in_voxels, cube, VIEWER_MARGIN))
        };
        let cubes = select_around_brick(
            self.centre,
            &ViewSelection {
                detail: self.settings.detail,
                zoom: self.settings.zoom,
                fine_radius: self.fine_radius,
                maximum_distance: self.settings.maximum_distance,
            },
            &may_hold,
        );
        memo.borrow_mut().finish_round();
        cubes
    }
}

/// What the thread is given to work out the cubes of a place: everything that is not the terrain itself is a copy, so that the view goes on as it was.
pub(crate) struct WarmJob {
    /// Which of the views' lives the job is for ([`crate::lod::LodView::reset`] and taking the warm viewpoint away start another).
    pub(crate) epoch: u64,
    pub(crate) centre: BrickCoordinate,
    pub(crate) viewer: WorldPosition,
    pub(crate) settings: LodSettings,
    pub(crate) version: u64,
    pub(crate) edits: u64,
    pub(crate) grid: GridId,
    pub(crate) fine_radius: u32,
    pub(crate) core: Option<SolidBall>,
    pub(crate) draws_beside_edits: bool,
    pub(crate) edited: EditedCubes,
    pub(crate) oracle: MayShowOracle,
    /// What the thread said of the cubes the time before, and goes on from.
    pub(crate) memo: MayShowMemo,
}

/// What the thread gives back: the cubes, the nodes of them in the order they are to be made in ([`warm_queue_of`]), and the memory it kept of what the terrain said.
pub(crate) struct WarmDone {
    pub(crate) epoch: u64,
    pub(crate) cut: KeptCut,
    pub(crate) queue: Vec<BrickCube>,
    pub(crate) memo: MayShowMemo,
}

pub(crate) enum WarmFinished {
    Done(Box<WarmDone>),
    /// The terrain's answers panicked, with this payload.
    Panicked(Box<dyn Any + Send>),
}

fn work_out(job: WarmJob) -> WarmDone {
    let WarmJob {
        epoch,
        centre,
        viewer,
        settings,
        version,
        edits,
        grid,
        fine_radius,
        core,
        draws_beside_edits,
        edited,
        oracle,
        mut memo,
    } = job;
    let cubes = Selection {
        centre,
        viewer,
        settings: &settings,
        version,
        grid,
        edited: &edited,
        core,
        draws_beside_edits,
        fine_radius,
    }
    .select(&mut memo, &*oracle);
    let queue = warm_queue_of(&cubes, centre);
    WarmDone {
        epoch,
        queue,
        cut: KeptCut {
            centre,
            settings,
            version,
            edits,
            cubes,
        },
        memo,
    }
}

/// The thread that works out the cubes of the warm viewpoint, one place at a time.
pub(crate) struct WarmWorker {
    jobs: Option<mpsc::Sender<WarmJob>>,
    results: mpsc::Receiver<WarmFinished>,
    thread: Option<JoinHandle<()>>,
}

impl WarmWorker {
    pub(crate) fn start() -> Self {
        let (job_sender, job_receiver) = mpsc::channel::<WarmJob>();
        let (result_sender, results) = mpsc::channel::<WarmFinished>();
        let thread = std::thread::Builder::new()
            .name("warm cut".to_string())
            .spawn(move || {
                while let Ok(job) = job_receiver.recv() {
                    // A bug in what the terrain says panics here, on a thread nobody is watching: it is caught, and said to the view, which panics with it.
                    let finished = catch_unwind(AssertUnwindSafe(|| work_out(job)))
                        .map_or_else(WarmFinished::Panicked, |done| {
                            WarmFinished::Done(Box::new(done))
                        });
                    if result_sender.send(finished).is_err() {
                        return;
                    }
                }
            })
            .expect("a thread can be made");
        Self {
            jobs: Some(job_sender),
            results,
            thread: Some(thread),
        }
    }

    pub(crate) fn hand_over(&self, job: WarmJob) {
        self.jobs
            .as_ref()
            .expect("the thread runs until the view is dropped")
            .send(job)
            .expect("the thread is running");
    }

    /// What the thread has finished, if anything (one at a time).
    pub(crate) fn finished(&self) -> Option<WarmFinished> {
        self.results.try_recv().ok()
    }
}

impl Drop for WarmWorker {
    fn drop(&mut self) {
        // Closing the queue ends the loop of the thread, after the working out that it is at.
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests;
