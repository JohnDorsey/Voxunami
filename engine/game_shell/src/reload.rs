//! The spine of hot reloading: noticing that files changed, waiting until they are whole, and swapping new state in only if all of it is good.
//!
//! Every kind of content that is reloaded while the game runs (shader packs, the engine's own shaders, pictures, block data: `docs/hot-reload-plan.md`) uses these three parts and adds
//! only what is its own:
//!
//! 1. A [`Watcher`] looks at a [`Source`] (a folder or a file) a few times a second. A file counts as moved when its modification time or its size moved, and as changed when it
//!    has also *settled* (the source looks the same at two looks at least `settle` apart, so a file an editor is still writing never loads) and its content differs from what the
//!    watcher last accepted (so saving a file without changing it reloads nothing).
//! 2. A [`Reloadable`] kind loads everything *beside* the state in use ([`Reloadable::load`] cannot change it), and only then swaps ([`Reloadable::swap`]). A load or a swap that
//!    fails leaves the game as it was, and the reason goes to the player.
//! 3. [`Reloads`] holds the kinds, tells the player what happened ([`Report::line`]), and is what the `.reload` command and `--watch` drive.
//!
//! Time is given to the watcher by the caller (a [`Duration`] since any fixed start), never read here, so a test of every timing is exact. Nothing in this module touches the
//! simulation: reloading is the client's own, or the server's at a tick boundary, and never changes what a recording must replay (`docs/hot-reload-plan.md`, step 4).

use std::collections::BTreeMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// What a [`Source`] says of a file without reading it: when it was written and how long it is. A file whose stamp is the same as it was is taken to be the same.
///
/// The clock that stamps files is coarse: on the owner's machine (Windows 10, NTFS) 196 of 199 back-to-back writes of one file had the same modification time (measured
/// 2026-11-15; the steps seen were 1, 7 and 8 ms, and Windows' own tick is 15.6 ms). So an edit that keeps the length and lands within one step of the write the watcher last read
/// is not seen. The watcher cannot be caught so after the start: it reads a file only after the stamp has been still for `settle`, which is longer than a step, so the content it
/// reads was written in an earlier step than any later write. The one exposed moment is [`Watcher::new`], which reads the files as they are at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    /// When the file was last written, if the file system says.
    pub modified: Option<SystemTime>,
    /// Its length in bytes.
    pub length: u64,
}

/// Where the files of one kind of content are, as far as the watcher needs to know.
pub trait Source {
    /// Every file now, by path, with its stamp: cheap, since it is done a few times a second. An error means the place cannot be looked at now (a folder being replaced).
    fn scan(&mut self) -> Result<BTreeMap<PathBuf, FileStamp>, String>;
    /// A number that is the same for the same content: dear (it reads the file), so it is asked for only of a file whose stamp moved.
    fn digest(&mut self, path: &Path) -> Result<u64, String>;
}

/// One file's change, as [`Watcher::poll`] tells it. Paths are those the [`Source`] gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// A file that was not there before.
    Added(PathBuf),
    /// A file whose content is not what it was.
    Modified(PathBuf),
    /// A file that is gone.
    Removed(PathBuf),
}

impl Change {
    /// The file the change is of.
    pub fn path(&self) -> &Path {
        match self {
            Self::Added(path) | Self::Modified(path) | Self::Removed(path) => path,
        }
    }
}

/// What one look of a [`Watcher`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Poll {
    /// Nothing moved (or it is too soon to look again).
    Quiet,
    /// Something moved and has not been still for long enough: do nothing yet.
    Waiting,
    /// Files changed and are settled. The watcher takes them as seen: if a load of them fails it is not tried again until the files change again, and if they change during the load
    /// the next look sees it.
    Changed(Vec<Change>),
    /// The source cannot be looked at, with the reason: told once for as long as the reason stays the same.
    Unreadable(String),
}

/// What is known of one file the watcher has taken as seen.
#[derive(Clone, Copy, Debug)]
struct Seen {
    stamp: FileStamp,
    digest: u64,
}

/// Notices that the files of a [`Source`] changed, and when they are whole.
#[derive(Debug)]
pub struct Watcher<S: Source> {
    source: S,
    /// How long the source must look the same before a change counts.
    settle: Duration,
    /// How seldom the source is looked at.
    interval: Duration,
    /// The files as last accepted (or as they were at the start).
    known: BTreeMap<PathBuf, Seen>,
    /// When the source was last looked at.
    last_look: Option<Duration>,
    /// What the source looked like at the last look that found something moved, and since when it has looked so.
    unsettled: Option<(BTreeMap<PathBuf, FileStamp>, Duration)>,
    /// The reason the last look failed, while it keeps failing.
    last_error: Option<String>,
}

impl<S: Source> Watcher<S> {
    /// How long a file must be still before it counts as written: an editor saves in two steps, and a file half written must never be loaded.
    pub const DEFAULT_SETTLE: Duration = Duration::from_millis(150);
    /// How often the source is looked at.
    pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(250);

    /// Takes the files as they are now as seen (reading each once), so only later changes are told.
    pub fn new(mut source: S, settle: Duration, interval: Duration) -> Result<Self, String> {
        let mut known = BTreeMap::new();
        for (path, stamp) in source.scan()? {
            let digest = source.digest(&path)?;
            known.insert(path, Seen { stamp, digest });
        }
        Ok(Self {
            source,
            settle,
            interval,
            known,
            last_look: None,
            unsettled: None,
            last_error: None,
        })
    }

    /// Looks at the source if `interval` has passed since the last look. `now` is any monotonic time, only ever compared with an earlier `now` of the same watcher.
    pub fn poll(&mut self, now: Duration) -> Poll {
        if self
            .last_look
            .is_some_and(|last| now.saturating_sub(last) < self.interval)
        {
            return self.in_between();
        }
        self.last_look = Some(now);
        let scan = match self.source.scan() {
            Ok(scan) => {
                self.last_error = None;
                scan
            }
            Err(reason) => {
                // Whatever was moving, it cannot be judged settled across a look that failed.
                self.unsettled = None;
                return if self.last_error.as_ref() == Some(&reason) {
                    Poll::Quiet
                } else {
                    self.last_error = Some(reason.clone());
                    Poll::Unreadable(reason)
                };
            }
        };
        if !self.moved(&scan) {
            self.unsettled = None;
            return Poll::Quiet;
        }
        match &self.unsettled {
            Some((seen, since)) if *seen == scan => {
                if now.saturating_sub(*since) < self.settle {
                    return Poll::Waiting;
                }
            }
            _ => {
                self.unsettled = Some((scan, now));
                return Poll::Waiting;
            }
        }
        self.unsettled = None;
        self.accept(scan)
    }

    /// What a poll that is too soon to look says.
    fn in_between(&self) -> Poll {
        if self.unsettled.is_some() {
            Poll::Waiting
        } else {
            Poll::Quiet
        }
    }

    /// Whether any file is added, gone or has a stamp other than the one accepted.
    fn moved(&self, scan: &BTreeMap<PathBuf, FileStamp>) -> bool {
        scan.len() != self.known.len()
            || scan
                .iter()
                .any(|(path, stamp)| self.known.get(path).is_none_or(|seen| seen.stamp != *stamp))
    }

    /// Takes a settled scan as seen, reading the files whose stamp moved to tell a change from a mere save.
    fn accept(&mut self, scan: BTreeMap<PathBuf, FileStamp>) -> Poll {
        let mut changes = Vec::new();
        let mut known = BTreeMap::new();
        for (path, stamp) in &scan {
            let before = self.known.get(path);
            if before.is_some_and(|seen| seen.stamp == *stamp) {
                known.insert(path.clone(), *before.expect("just checked"));
                continue;
            }
            let digest = match self.source.digest(path) {
                Ok(digest) => digest,
                // The file went between the look and the reading: start the waiting again.
                Err(_) => return Poll::Waiting,
            };
            match before {
                None => changes.push(Change::Added(path.clone())),
                Some(seen) if seen.digest != digest => changes.push(Change::Modified(path.clone())),
                Some(_) => {}
            }
            known.insert(
                path.clone(),
                Seen {
                    stamp: *stamp,
                    digest,
                },
            );
        }
        for path in self.known.keys() {
            if !scan.contains_key(path) {
                changes.push(Change::Removed(path.clone()));
            }
        }
        self.known = known;
        if changes.is_empty() {
            Poll::Quiet
        } else {
            changes.sort_by(|a, b| a.path().cmp(b.path()));
            Poll::Changed(changes)
        }
    }
}

/// One kind of content that can be reloaded (a shader pack folder, a folder of pictures), with `C` what it changes (the renderer and the session, say).
///
/// A reload is the same as starting afresh with the files as they are: [`Reloadable::load`] reads *all* of them, not only the changed ones, so what a reload leaves cannot
/// differ from what a start would have made.
pub trait Reloadable<C> {
    /// Everything read and checked, ready to be put in use.
    type Loaded;
    /// Reads and checks the files. It cannot change the state in use (`context` is shared, not mutable), so an error leaves the game as it was.
    fn load(&self, context: &C) -> Result<Self::Loaded, String>;
    /// Puts what was loaded in use. Returns what the player is told, or the reason it was refused (the graphics card refused a shader), in which case the state in use must be
    /// as it was before the call.
    fn swap(&self, context: &mut C, loaded: Self::Loaded) -> Result<String, String>;
}

/// How a reload ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// New state is in use. `detail` says what was done; `took` is how long loading and swapping took.
    Reloaded {
        /// What the kind says of what it did.
        detail: String,
        /// Loading and swapping together.
        took: Duration,
    },
    /// Nothing was changed. `reason` says why.
    Failed {
        /// Why: what the kind or the source said.
        reason: String,
    },
}

/// Loads, then swaps: the one place the rule "all of it good, or nothing changes" is kept.
pub fn reload_once<C, R: Reloadable<C>>(kind: &R, context: &mut C) -> Outcome {
    let start = Instant::now();
    let loaded = match kind.load(context) {
        Ok(loaded) => loaded,
        Err(reason) => return Outcome::Failed { reason },
    };
    match kind.swap(context, loaded) {
        Ok(detail) => Outcome::Reloaded {
            detail,
            took: start.elapsed(),
        },
        Err(reason) => Outcome::Failed { reason },
    }
}

/// What happened to one kind, for the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The kind's name.
    pub kind: String,
    /// How it ended.
    pub outcome: Outcome,
}

impl Report {
    /// One line for the chat.
    pub fn line(&self) -> String {
        match &self.outcome {
            Outcome::Reloaded { detail, took } => format!(
                "reloaded {}: {detail} ({:.0} ms)",
                self.kind,
                took.as_secs_f64() * 1000.0
            ),
            Outcome::Failed { reason } => {
                format!(
                    "reload of {} failed, nothing was changed: {reason}",
                    self.kind
                )
            }
        }
    }
}

/// A kind with its watcher, behind one interface so that [`Reloads`] can hold kinds of every sort.
trait Registered<C> {
    fn name(&self) -> &str;
    fn watch(&mut self, now: Duration, context: &mut C) -> Option<Report>;
    fn reload_now(&mut self, context: &mut C) -> Report;
}

struct Kind<S: Source, R> {
    name: String,
    watcher: Watcher<S>,
    logic: R,
}

impl<C, S: Source, R: Reloadable<C>> Registered<C> for Kind<S, R> {
    fn name(&self) -> &str {
        &self.name
    }

    fn watch(&mut self, now: Duration, context: &mut C) -> Option<Report> {
        let outcome = match self.watcher.poll(now) {
            Poll::Quiet | Poll::Waiting => return None,
            Poll::Unreadable(reason) => Outcome::Failed {
                reason: format!("its files cannot be looked at: {reason}"),
            },
            Poll::Changed(_) => reload_once(&self.logic, context),
        };
        Some(Report {
            kind: self.name.clone(),
            outcome,
        })
    }

    fn reload_now(&mut self, context: &mut C) -> Report {
        Report {
            kind: self.name.clone(),
            outcome: reload_once(&self.logic, context),
        }
    }
}

/// Every kind of content that can be reloaded, and whether changes are acted on by themselves (`--watch`).
pub struct Reloads<C> {
    kinds: Vec<Box<dyn Registered<C>>>,
    watching: bool,
}

impl<C> std::fmt::Debug for Reloads<C> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Reloads")
            .field("kinds", &self.names())
            .field("watching", &self.watching)
            .finish()
    }
}

impl<C> Reloads<C> {
    /// No kinds yet; `watching` says whether [`Reloads::poll`] acts on changes (off unless the player asked: a reload by hand always works).
    pub fn new(watching: bool) -> Self {
        Self {
            kinds: Vec::new(),
            watching,
        }
    }

    /// Adds a kind of content, watched at `source`. An error says the name is taken or the source cannot be read.
    pub fn add<S: Source + 'static, R: Reloadable<C> + 'static>(
        &mut self,
        name: &str,
        source: S,
        settle: Duration,
        interval: Duration,
        logic: R,
    ) -> Result<(), String>
    where
        C: 'static,
    {
        if self.kinds.iter().any(|kind| kind.name() == name) {
            return Err(format!("a kind of reload called `{name}` is already there"));
        }
        let watcher = Watcher::new(source, settle, interval)
            .map_err(|reason| format!("`{name}`: its files cannot be looked at: {reason}"))?;
        self.kinds.push(Box::new(Kind {
            name: name.to_owned(),
            watcher,
            logic,
        }));
        Ok(())
    }

    /// The names of the kinds, in the order they were added.
    pub fn names(&self) -> Vec<&str> {
        self.kinds.iter().map(|kind| kind.name()).collect()
    }

    /// Whether changes are acted on by themselves.
    pub fn watching(&self) -> bool {
        self.watching
    }

    /// Turns acting on changes by themselves on or off.
    pub fn set_watching(&mut self, watching: bool) {
        self.watching = watching;
    }

    /// Called often (every frame is fine): when watching, looks at each kind's files (at the kind's own interval) and reloads the kinds whose files changed and are whole. Does
    /// nothing, and does not look at any file, when not watching.
    pub fn poll(&mut self, now: Duration, context: &mut C) -> Vec<Report> {
        if !self.watching {
            return Vec::new();
        }
        self.kinds
            .iter_mut()
            .filter_map(|kind| kind.watch(now, context))
            .collect()
    }

    /// Reloads the kind called `name`, or every kind if there is no name (`.reload [kind]`), whether or not anything changed. An error says no kind has that name.
    pub fn reload(&mut self, name: Option<&str>, context: &mut C) -> Result<Vec<Report>, String> {
        if let Some(name) = name
            && !self.kinds.iter().any(|kind| kind.name() == name)
        {
            return Err(if self.kinds.is_empty() {
                format!("there is nothing to reload called `{name}` (nothing can be reloaded)")
            } else {
                format!(
                    "there is nothing to reload called `{name}`; there is: {}",
                    self.names().join(", ")
                )
            });
        }
        Ok(self
            .kinds
            .iter_mut()
            .filter(|kind| name.is_none_or(|name| kind.name() == name))
            .map(|kind| kind.reload_now(context))
            .collect())
    }
}

/// Files an editor leaves beside the one being edited, which are never content: hidden files, backups (`name~`), swap files and half-written copies.
fn is_editor_leftover(name: &str) -> bool {
    name.starts_with('.')
        || name.starts_with('#')
        || name.ends_with('~')
        || name.ends_with(".tmp")
        || name.ends_with(".swp")
        || name.ends_with(".swx")
        || name.ends_with(".part")
}

/// A folder (every file in it, and in the folders in it) or one file on disk.
#[derive(Clone, Debug)]
pub struct DiskSource {
    root: PathBuf,
}

impl DiskSource {
    /// A source at `root`, a folder or a file. It is not looked at until the first scan.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn collect(
        folder: &Path,
        files: &mut BTreeMap<PathBuf, FileStamp>,
    ) -> Result<(), std::io::Error> {
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            if is_editor_leftover(&entry.file_name().to_string_lossy()) {
                continue;
            }
            let path = entry.path();
            // A file may go between the listing and the question: a file that is not there is not a file, and the next look is the judge.
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };
            if metadata.is_dir() {
                Self::collect(&path, files)?;
            } else {
                files.insert(
                    path,
                    FileStamp {
                        modified: metadata.modified().ok(),
                        length: metadata.len(),
                    },
                );
            }
        }
        Ok(())
    }
}

impl Source for DiskSource {
    fn scan(&mut self) -> Result<BTreeMap<PathBuf, FileStamp>, String> {
        let metadata = std::fs::metadata(&self.root)
            .map_err(|error| format!("{}: {error}", self.root.display()))?;
        let mut files = BTreeMap::new();
        if metadata.is_dir() {
            Self::collect(&self.root, &mut files)
                .map_err(|error| format!("{}: {error}", self.root.display()))?;
        } else {
            files.insert(
                self.root.clone(),
                FileStamp {
                    modified: metadata.modified().ok(),
                    length: metadata.len(),
                },
            );
        }
        Ok(files)
    }

    fn digest(&mut self, path: &Path) -> Result<u64, String> {
        let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut hasher = std::hash::DefaultHasher::new();
        hasher.write(&bytes);
        Ok(hasher.finish())
    }
}

#[cfg(test)]
mod tests;
