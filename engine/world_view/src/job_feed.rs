//! How many nodes the view keeps handed to its threads.
//!
//! A view hands jobs to its threads only when it is updated, once a frame, so a thread that has been given fewer jobs than it can do in a frame is idle for the rest of it. At 4 for
//! each of 11 threads, with nodes that take about a millisecond, the threads were given 44 nodes a frame (2,600 a second at 60 frames a second) where they can do about 5,000:
//! the 15,000 nodes of a view from 264 km up back to the ground took 7.5 s, and 3.2 s with enough handed out (measured 2026-11-21, `docs/history/build-log.md`; at 8 for each thread
//! it took 4.2 s, at 16, 32 and 128 3.2 s).
//!
//! More cannot simply be handed out always: what a thread has been handed cannot be taken back or put in another order when the view moves, so for nodes that take long (hills with
//! caves take 94 to 201 ms) a long queue is seconds of work for a view that has moved on. How much a thread can do in a frame is known from what the threads finished since the
//! last update, by counting (no clock): hold twice that, and a little more, between the old least and a most.

/// The least held for each thread: what was held before this was measured, and what slow nodes need.
pub(crate) const MINIMUM_JOBS_PER_WORKER: usize = 4;

/// The most held for each thread: what is handed to threads that are finishing everything they are given between two updates (4 times what 60 frames a second needs at a
/// millisecond a node; the work a thread has been handed for a view that has moved on is done and thrown away).
pub(crate) const MAXIMUM_JOBS_PER_WORKER: usize = 64;

/// How many nodes to have handed to `workers` threads, given how many they finished since the last update. Threads that finished everything held more than this update's
/// worth went idle: twice what they finished, and one for each thread, is held (so the number doubles for as long as the threads run dry, and holds still when it is twice what
/// they finish in an update).
pub(crate) fn jobs_to_hold(workers: usize, finished: usize) -> usize {
    (2 * finished + workers).clamp(
        workers * MINIMUM_JOBS_PER_WORKER,
        workers * MAXIMUM_JOBS_PER_WORKER,
    )
}

#[cfg(test)]
mod tests;
