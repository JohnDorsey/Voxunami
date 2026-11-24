//! The queue the threads that make nodes take their jobs from, for a view that keeps a second place warm ([`crate::LodView::set_warm_viewpoint`]).
//!
//! The nodes of the place that is drawn and the nodes of the place that is only kept ready are made by the same threads, but not alike: a **drawn** job is taken before any
//! **warm** one that waits, and only **a share of the threads** (`warm_threads`) make warm jobs at the same time. The share is what leaves the rest of the machine, and the
//! rest of the game, to the player: a ground map that is made when it is first kept ready took the whole machine for 1.7 s (10 cores, measured 2026-11-23) while the
//! game was being played. A warm job that waits can be made a drawn one ([`JobQueue::promote`]) when the place it is for is the one that is drawn now.
//!
//! What a thread has been given cannot be taken back, and neither can what waits here be put in another order: the view holds what it holds in the queue by
//! [`crate::job_feed`], for each kind apart.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// A job and what kind it is, as a thread takes it: it tells the queue when it is done with it ([`JobQueue::done`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Taken<T> {
    pub job: T,
    pub warm: bool,
}

struct State<T> {
    drawn: VecDeque<T>,
    warm: VecDeque<T>,
    /// How many warm jobs are being made, and how many may be at the same time (at least one).
    warm_running: usize,
    warm_threads: usize,
    closed: bool,
}

impl<T> State<T> {
    fn take(&mut self) -> Option<Taken<T>> {
        if let Some(job) = self.drawn.pop_front() {
            return Some(Taken { job, warm: false });
        }
        if self.warm_running < self.warm_threads {
            let job = self.warm.pop_front()?;
            self.warm_running += 1;
            return Some(Taken { job, warm: true });
        }
        None
    }

    /// Whether a thread that asked now would be given a job.
    fn has_a_job_to_take(&self) -> bool {
        !self.drawn.is_empty() || (self.warm_running < self.warm_threads && !self.warm.is_empty())
    }
}

pub(crate) struct JobQueue<T> {
    state: Mutex<State<T>>,
    wake: Condvar,
}

impl<T> JobQueue<T> {
    /// A queue in which `warm_threads` threads (at least one) make warm jobs at the same time.
    pub fn new(warm_threads: usize) -> Self {
        Self {
            state: Mutex::new(State {
                drawn: VecDeque::new(),
                warm: VecDeque::new(),
                warm_running: 0,
                warm_threads: warm_threads.max(1),
                closed: false,
            }),
            wake: Condvar::new(),
        }
    }

    fn locked(&self) -> MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How many threads make warm jobs at the same time from now on (at least one).
    pub fn set_warm_threads(&self, warm_threads: usize) {
        self.locked().warm_threads = warm_threads.max(1);
        self.wake.notify_all();
    }

    /// Puts a job at the back of its kind. Nothing is taken once the queue is closed.
    pub fn push(&self, job: T, warm: bool) {
        let mut state = self.locked();
        if state.closed {
            return;
        }
        if warm {
            state.warm.push_back(job);
        } else {
            state.drawn.push_back(job);
        }
        drop(state);
        self.wake.notify_one();
    }

    /// Makes the first warm job that waits and that `is` says is the one a drawn job, at the back of the drawn jobs. Whether there was one: a job that a thread has taken
    /// cannot be moved.
    pub fn promote(&self, is: impl Fn(&T) -> bool) -> bool {
        let mut state = self.locked();
        let Some(position) = state.warm.iter().position(is) else {
            return false;
        };
        let job = state.warm.remove(position).expect("it was just found");
        state.drawn.push_back(job);
        drop(state);
        self.wake.notify_one();
        true
    }

    /// The next job, waiting for one that may be taken, or `None` when the queue is closed (the thread ends).
    pub fn next(&self) -> Option<Taken<T>> {
        let mut state = self.locked();
        loop {
            if state.closed {
                return None;
            }
            if let Some(taken) = state.take() {
                // (Another thread that waits may take what is left: it is not woken by the push that was for this one.)
                if state.has_a_job_to_take() {
                    self.wake.notify_one();
                }
                return Some(taken);
            }
            state = self
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The next job if there is one to take now.
    #[cfg(test)]
    pub fn try_next(&self) -> Option<Taken<T>> {
        let mut state = self.locked();
        if state.closed {
            return None;
        }
        state.take()
    }

    /// A thread is done with a job it took: a warm one makes room for the next warm job.
    pub fn done(&self, warm: bool) {
        if !warm {
            return;
        }
        let mut state = self.locked();
        state.warm_running = state.warm_running.saturating_sub(1);
        let more = state.has_a_job_to_take();
        drop(state);
        if more {
            self.wake.notify_one();
        }
    }

    /// Ends the threads: what waits is dropped, and each thread that asks is told to end.
    pub fn close(&self) {
        let mut state = self.locked();
        state.closed = true;
        state.drawn.clear();
        state.warm.clear();
        drop(state);
        self.wake.notify_all();
    }
}

#[cfg(test)]
mod tests;
