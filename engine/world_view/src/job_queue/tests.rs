use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::{JobQueue, Taken};

/// How long a test waits for threads to do what they were given: for a queue that is stuck, not for a slow one.
const DEADLINE: Duration = Duration::from_secs(60);

fn taken(queue: &JobQueue<u32>) -> Option<(u32, bool)> {
    queue.try_next().map(|Taken { job, warm }| (job, warm))
}

#[test]
fn drawn_jobs_are_taken_before_warm_ones_whatever_order_they_were_pushed_in_and_each_kind_in_its_order()
 {
    let queue = JobQueue::new(2);
    queue.push(1, true);
    queue.push(2, true);
    queue.push(10, false);
    queue.push(3, true);
    queue.push(11, false);
    assert_eq!(taken(&queue), Some((10, false)));
    assert_eq!(taken(&queue), Some((11, false)));
    assert_eq!(taken(&queue), Some((1, true)));
    assert_eq!(taken(&queue), Some((2, true)));
}

#[test]
fn only_so_many_warm_jobs_are_taken_at_a_time_and_the_next_waits_until_one_is_done_but_a_drawn_job_never_waits_for_them()
 {
    let queue = JobQueue::new(1);
    queue.push(1, true);
    queue.push(2, true);
    assert_eq!(taken(&queue), Some((1, true)));
    assert_eq!(taken(&queue), None, "the one thread for warm work is busy");
    // A drawn job that comes now is taken at once, ahead of the warm job that waits.
    queue.push(10, false);
    assert_eq!(taken(&queue), Some((10, false)));
    assert_eq!(taken(&queue), None);
    queue.done(true);
    assert_eq!(taken(&queue), Some((2, true)));
    // (A drawn job being done frees nothing for warm work.)
    queue.push(3, true);
    queue.done(false);
    assert_eq!(taken(&queue), None);
    queue.done(true);
    assert_eq!(taken(&queue), Some((3, true)));
}

#[test]
fn a_share_of_no_threads_is_one() {
    let queue = JobQueue::new(0);
    queue.push(1, true);
    queue.push(2, true);
    assert_eq!(taken(&queue), Some((1, true)));
    assert_eq!(taken(&queue), None);
    // The share can be changed while jobs wait.
    queue.set_warm_threads(3);
    assert_eq!(taken(&queue), Some((2, true)));
}

#[test]
fn a_warm_job_that_waits_can_be_made_a_drawn_one_and_one_that_is_taken_or_not_there_cannot() {
    let queue = JobQueue::new(1);
    queue.push(1, true);
    queue.push(2, true);
    queue.push(3, true);
    queue.push(10, false);
    assert_eq!(taken(&queue), Some((10, false)));
    assert_eq!(taken(&queue), Some((1, true)));
    // 1 is being made, 7 was never there: neither moves.
    assert!(!queue.promote(|job| *job == 1));
    assert!(!queue.promote(|job| *job == 7));
    // 3 waits: it moves, behind the drawn jobs that wait already and ahead of the warm job that waits.
    queue.push(11, false);
    assert!(queue.promote(|job| *job == 3));
    assert_eq!(taken(&queue), Some((11, false)));
    assert_eq!(taken(&queue), Some((3, false)));
    assert_eq!(taken(&queue), None, "2 waits for the thread of warm work");
    queue.done(true);
    assert_eq!(taken(&queue), Some((2, true)));
}

#[test]
fn every_warm_job_that_waits_and_is_wanted_is_made_a_drawn_one_in_the_order_it_waited_and_how_many_is_said()
 {
    let queue = JobQueue::new(1);
    for job in [1, 2, 3, 4, 5] {
        queue.push(job, true);
    }
    queue.push(10, false);
    assert_eq!(taken(&queue), Some((10, false)));
    assert_eq!(taken(&queue), Some((1, true)));
    queue.push(11, false);
    // 1 is being made and does not move; 3 and 5 wait and do; 2 and 4 are not wanted.
    assert_eq!(queue.promote_all(|job| *job % 2 == 1), 2);
    assert_eq!(queue.promote_all(|job| *job == 99), 0, "nothing is wanted");
    assert_eq!(
        queue.promote_all(|job| *job == 10),
        0,
        "a drawn job is not a warm one"
    );
    // Behind the drawn job that waited already, in the order they waited in, and ahead of the warm jobs that are left.
    assert_eq!(taken(&queue), Some((11, false)));
    assert_eq!(taken(&queue), Some((3, false)));
    assert_eq!(taken(&queue), Some((5, false)));
    assert_eq!(
        taken(&queue),
        None,
        "2 and 4 wait for the thread of warm work"
    );
    queue.done(true);
    assert_eq!(taken(&queue), Some((2, true)));
}

#[test]
fn a_queue_that_is_closed_drops_what_waited_and_gives_nothing_more() {
    let queue = JobQueue::new(2);
    queue.push(1, true);
    queue.push(10, false);
    queue.close();
    assert_eq!(taken(&queue), None);
    assert!(queue.next().is_none(), "a thread that asks is told to end");
    queue.push(2, false);
    assert!(queue.next().is_none(), "nothing is taken after the end");
}

/// Threads that take jobs from the queue and do them, `work` for each (told whether it is warm), until the queue is closed.
fn threads(
    queue: &Arc<JobQueue<u32>>,
    count: usize,
    work: impl Fn(u32, bool) + Send + Sync + 'static,
) -> Vec<std::thread::JoinHandle<()>> {
    let work = Arc::new(work);
    (0..count)
        .map(|_| {
            let (queue, work) = (Arc::clone(queue), Arc::clone(&work));
            std::thread::spawn(move || {
                while let Some(Taken { job, warm }) = queue.next() {
                    work(job, warm);
                    queue.done(warm);
                }
            })
        })
        .collect()
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(started.elapsed() < DEADLINE, "{what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn no_more_threads_than_the_share_make_warm_jobs_at_the_same_time_and_every_job_is_made() {
    let queue = Arc::new(JobQueue::new(2));
    let (running, most, made) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let handles = {
        let (running, most, made) = (Arc::clone(&running), Arc::clone(&most), Arc::clone(&made));
        threads(&queue, 6, move |_, warm| {
            if warm {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(2));
                running.fetch_sub(1, Ordering::SeqCst);
            }
            made.fetch_add(1, Ordering::SeqCst);
        })
    };
    for job in 0..80 {
        queue.push(job, true);
        if job % 4 == 0 {
            queue.push(1000 + job, false);
        }
    }
    wait_until("every job is made", || {
        made.load(Ordering::SeqCst) == 80 + 20
    });
    queue.close();
    for handle in handles {
        handle.join().unwrap();
    }
    assert!(
        most.load(Ordering::SeqCst) <= 2,
        "{} warm jobs were made at once",
        most.load(Ordering::SeqCst)
    );
}

/// A gate that holds the threads that wait at it until it is opened.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
    }

    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

#[test]
fn a_drawn_job_is_made_at_once_by_a_thread_that_is_free_though_warm_jobs_are_held_up_and_waiting() {
    let queue = Arc::new(JobQueue::new(1));
    let gate = Arc::new(Gate::default());
    let made: Arc<Mutex<Vec<u32>>> = Arc::default();
    let handles = {
        let (gate, made) = (Arc::clone(&gate), Arc::clone(&made));
        threads(&queue, 2, move |job, warm| {
            if warm {
                gate.wait();
            }
            made.lock().unwrap().push(job);
        })
    };
    // The one thread for warm work takes the first warm job and is held; the second waits (the other thread is free but is not given it).
    queue.push(1, true);
    queue.push(2, true);
    queue.push(10, false);
    wait_until("the drawn job is made while the warm job is held", || {
        made.lock().unwrap().contains(&10)
    });
    assert_eq!(*made.lock().unwrap(), [10]);
    gate.open();
    wait_until("both warm jobs are made once the gate is open", || {
        made.lock().unwrap().len() == 3
    });
    queue.close();
    for handle in handles {
        handle.join().unwrap();
    }
}
