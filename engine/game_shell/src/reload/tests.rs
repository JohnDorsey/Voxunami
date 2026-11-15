use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::shader_packs::tests::Folder;

const SETTLE: Duration = Duration::from_millis(150);
const INTERVAL: Duration = Duration::from_millis(100);

fn at(milliseconds: u64) -> Duration {
    Duration::from_millis(milliseconds)
}

/// Files that live in memory, shared between the test (which edits them) and the watcher (which looks at them).
#[derive(Clone, Debug, Default)]
struct FakeFolder(Rc<RefCell<FakeState>>);

#[derive(Debug, Default)]
struct FakeState {
    files: BTreeMap<PathBuf, (FileStamp, Vec<u8>)>,
    unreadable: Option<String>,
    clock: u64,
    scans: usize,
    digests: usize,
}

impl FakeFolder {
    fn with(files: &[(&str, &str)]) -> Self {
        let folder = Self::default();
        for (name, text) in files {
            folder.write(name, text);
        }
        folder
    }

    /// Writes a file: a new modification time, and the length of the text.
    fn write(&self, name: &str, text: &str) {
        let mut state = self.0.borrow_mut();
        state.clock += 1;
        let stamp = FileStamp {
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(state.clock)),
            length: text.len() as u64,
        };
        state
            .files
            .insert(PathBuf::from(name), (stamp, text.as_bytes().to_vec()));
    }

    /// Saves a file again with the same content: a new modification time and nothing else.
    fn touch(&self, name: &str) {
        let text = self.text(name);
        self.write(name, &text);
    }

    fn remove(&self, name: &str) {
        self.0.borrow_mut().files.remove(&PathBuf::from(name));
    }

    fn text(&self, name: &str) -> String {
        let state = self.0.borrow();
        String::from_utf8(state.files[&PathBuf::from(name)].1.clone()).unwrap()
    }

    fn make_unreadable(&self, reason: Option<&str>) {
        self.0.borrow_mut().unreadable = reason.map(str::to_owned);
    }

    fn scans(&self) -> usize {
        self.0.borrow().scans
    }

    fn digests(&self) -> usize {
        self.0.borrow().digests
    }
}

impl Source for FakeFolder {
    fn scan(&mut self) -> Result<BTreeMap<PathBuf, FileStamp>, String> {
        let mut state = self.0.borrow_mut();
        state.scans += 1;
        if let Some(reason) = &state.unreadable {
            return Err(reason.clone());
        }
        Ok(state
            .files
            .iter()
            .map(|(path, (stamp, _))| (path.clone(), *stamp))
            .collect())
    }

    fn digest(&mut self, path: &Path) -> Result<u64, String> {
        let mut state = self.0.borrow_mut();
        state.digests += 1;
        let (_, bytes) = state
            .files
            .get(path)
            .ok_or_else(|| format!("{}: not found", path.display()))?;
        let mut hasher = std::hash::DefaultHasher::new();
        hasher.write(bytes);
        Ok(hasher.finish())
    }
}

fn watching(folder: &FakeFolder) -> Watcher<FakeFolder> {
    Watcher::new(folder.clone(), SETTLE, INTERVAL).expect("a folder that can be looked at")
}

fn path(name: &str) -> PathBuf {
    PathBuf::from(name)
}

// ---- the watcher

#[test]
fn nothing_moving_is_quiet_however_long_it_is_watched() {
    let folder = FakeFolder::with(&[("lighting.wgsl", "a"), ("pack.json", "b")]);
    let mut watcher = watching(&folder);
    for time in (0..2000).step_by(100) {
        assert_eq!(watcher.poll(at(time)), Poll::Quiet, "at {time} ms");
    }
}

#[test]
fn a_change_is_told_once_when_it_has_been_still_for_the_settle_time() {
    let folder = FakeFolder::with(&[("lighting.wgsl", "a")]);
    let mut watcher = watching(&folder);
    folder.write("lighting.wgsl", "ab");
    // First sight at 100 ms; unchanged at 200 ms (100 ms still: not enough); unchanged at 300 ms (200 ms still: enough).
    assert_eq!(watcher.poll(at(100)), Poll::Waiting);
    assert_eq!(watcher.poll(at(200)), Poll::Waiting);
    assert_eq!(
        watcher.poll(at(300)),
        Poll::Changed(vec![Change::Modified(path("lighting.wgsl"))])
    );
    for time in [400, 500, 600] {
        assert_eq!(watcher.poll(at(time)), Poll::Quiet, "at {time} ms");
    }
}

#[test]
fn a_file_still_being_written_is_never_told_until_the_writing_stops() {
    let folder = FakeFolder::with(&[("lighting.wgsl", "a")]);
    let mut watcher = watching(&folder);
    let mut text = String::from("a");
    // Every look finds the file longer than the look before: however long it goes on, nothing loads.
    for time in (100..=1500).step_by(100) {
        text.push('b');
        folder.write("lighting.wgsl", &text);
        assert_eq!(watcher.poll(at(time)), Poll::Waiting, "at {time} ms");
    }
    assert_eq!(watcher.poll(at(1600)), Poll::Waiting);
    assert_eq!(
        watcher.poll(at(1700)),
        Poll::Changed(vec![Change::Modified(path("lighting.wgsl"))])
    );
}

#[test]
fn files_added_changed_and_removed_are_told_apart_and_in_order() {
    let folder = FakeFolder::with(&[("a", "1"), ("b", "2"), ("c", "3")]);
    let mut watcher = watching(&folder);
    folder.write("b", "22");
    folder.remove("c");
    folder.write("d", "4");
    assert_eq!(watcher.poll(at(100)), Poll::Waiting);
    assert_eq!(watcher.poll(at(200)), Poll::Waiting);
    assert_eq!(
        watcher.poll(at(300)),
        Poll::Changed(vec![
            Change::Modified(path("b")),
            Change::Removed(path("c")),
            Change::Added(path("d")),
        ])
    );
}

#[test]
fn saving_a_file_without_changing_it_reloads_nothing_and_is_not_asked_about_again() {
    let folder = FakeFolder::with(&[("a", "same")]);
    let mut watcher = watching(&folder);
    let digests_at_start = folder.digests();
    folder.touch("a");
    assert_eq!(watcher.poll(at(100)), Poll::Waiting);
    assert_eq!(watcher.poll(at(200)), Poll::Waiting);
    assert_eq!(watcher.poll(at(300)), Poll::Quiet);
    assert_eq!(
        folder.digests(),
        digests_at_start + 1,
        "the touched file was read once, to see that it is the same"
    );
    // The new stamp was accepted, so the touched file is not read at every look.
    for time in [400, 500, 600] {
        assert_eq!(watcher.poll(at(time)), Poll::Quiet);
    }
    assert_eq!(folder.digests(), digests_at_start + 1);
}

#[test]
fn only_files_whose_stamp_moved_are_read() {
    let files: Vec<(String, String)> = (0..50)
        .map(|number| (format!("tile_{number}"), format!("picture {number}")))
        .collect();
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect();
    let folder = FakeFolder::with(&borrowed);
    let mut watcher = watching(&folder);
    let digests_at_start = folder.digests();
    assert_eq!(digests_at_start, 50, "each file is read once at the start");
    folder.write("tile_7", "painted over");
    watcher.poll(at(100));
    watcher.poll(at(200));
    assert!(matches!(watcher.poll(at(300)), Poll::Changed(_)));
    assert_eq!(folder.digests(), digests_at_start + 1);
}

#[test]
fn a_change_that_is_put_back_before_it_settles_reloads_nothing() {
    let folder = FakeFolder::with(&[("a", "original")]);
    let mut watcher = watching(&folder);
    folder.write("a", "edited!");
    assert_eq!(watcher.poll(at(100)), Poll::Waiting);
    folder.write("a", "original");
    assert_eq!(watcher.poll(at(200)), Poll::Waiting);
    assert_eq!(watcher.poll(at(300)), Poll::Waiting);
    assert_eq!(
        watcher.poll(at(400)),
        Poll::Quiet,
        "the content is what was accepted"
    );
}

#[test]
fn a_change_after_a_change_is_told_again() {
    let folder = FakeFolder::with(&[("a", "1")]);
    let mut watcher = watching(&folder);
    folder.write("a", "22");
    watcher.poll(at(100));
    watcher.poll(at(200));
    assert!(matches!(watcher.poll(at(300)), Poll::Changed(_)));
    // The files change while the load of the first change is still going on (the load takes the time it takes): the next look sees it.
    folder.write("a", "333");
    watcher.poll(at(400));
    watcher.poll(at(500));
    assert_eq!(
        watcher.poll(at(600)),
        Poll::Changed(vec![Change::Modified(path("a"))])
    );
}

#[test]
fn a_file_that_goes_between_the_look_and_the_reading_starts_the_waiting_again() {
    // `scan` says the file is there and `digest` finds it gone: the watcher must not tell a change it could not read.
    #[derive(Debug)]
    struct Vanishing {
        scans: usize,
    }
    impl Source for Vanishing {
        fn scan(&mut self) -> Result<BTreeMap<PathBuf, FileStamp>, String> {
            self.scans += 1;
            let length = if self.scans == 1 { 1 } else { 2 };
            Ok(BTreeMap::from([(
                path("a"),
                FileStamp {
                    modified: None,
                    length,
                },
            )]))
        }
        fn digest(&mut self, _path: &Path) -> Result<u64, String> {
            if self.scans <= 1 {
                Ok(1)
            } else {
                Err("gone".to_owned())
            }
        }
    }
    let mut watcher =
        Watcher::new(Vanishing { scans: 0 }, SETTLE, INTERVAL).expect("it can be looked at");
    assert_eq!(watcher.poll(at(100)), Poll::Waiting);
    assert_eq!(watcher.poll(at(200)), Poll::Waiting);
    assert_eq!(
        watcher.poll(at(300)),
        Poll::Waiting,
        "settled, but could not be read"
    );
    assert_eq!(
        watcher.poll(at(400)),
        Poll::Waiting,
        "the waiting began again"
    );
}

#[test]
fn the_source_is_looked_at_no_more_often_than_the_interval() {
    let folder = FakeFolder::with(&[("a", "1")]);
    let mut watcher = watching(&folder);
    let scans_at_start = folder.scans();
    // A frame every 16 ms for two seconds: 125 calls.
    for frame in 0..125 {
        watcher.poll(at(frame * 16));
    }
    let looks = folder.scans() - scans_at_start;
    // The first look that is 100 ms after the last one is the one at 112 ms (a frame is 16 ms): 2 s holds 18 of them, and never more than 20.
    assert_eq!(
        looks, 18,
        "one look per 100 ms or later, not at every frame"
    );
}

#[test]
fn a_poll_between_two_looks_still_says_waiting_while_something_moves() {
    let folder = FakeFolder::with(&[("a", "1")]);
    let mut watcher = watching(&folder);
    folder.write("a", "22");
    assert_eq!(watcher.poll(at(100)), Poll::Waiting);
    assert_eq!(watcher.poll(at(116)), Poll::Waiting);
    assert_eq!(watcher.poll(at(132)), Poll::Waiting);
}

#[test]
fn a_source_that_cannot_be_looked_at_is_told_once_and_recovers() {
    let folder = FakeFolder::with(&[("a", "1")]);
    let mut watcher = watching(&folder);
    folder.make_unreadable(Some("the folder is being replaced"));
    assert_eq!(
        watcher.poll(at(100)),
        Poll::Unreadable("the folder is being replaced".to_owned())
    );
    for time in [200, 300, 400] {
        assert_eq!(watcher.poll(at(time)), Poll::Quiet, "told once, at {time}");
    }
    folder.make_unreadable(Some("another reason"));
    assert_eq!(
        watcher.poll(at(500)),
        Poll::Unreadable("another reason".to_owned()),
        "a different reason is told"
    );
    folder.make_unreadable(None);
    assert_eq!(
        watcher.poll(at(600)),
        Poll::Quiet,
        "the same files as before"
    );
    folder.make_unreadable(Some("another reason"));
    assert_eq!(
        watcher.poll(at(700)),
        Poll::Unreadable("another reason".to_owned()),
        "after recovering, the same reason is a new event"
    );
}

#[test]
fn a_change_made_while_the_source_was_unreadable_is_found_after_it_recovers() {
    let folder = FakeFolder::with(&[("a", "1")]);
    let mut watcher = watching(&folder);
    folder.make_unreadable(Some("busy"));
    folder.write("a", "22");
    watcher.poll(at(100));
    folder.make_unreadable(None);
    assert_eq!(watcher.poll(at(200)), Poll::Waiting);
    assert_eq!(watcher.poll(at(300)), Poll::Waiting);
    assert_eq!(
        watcher.poll(at(400)),
        Poll::Changed(vec![Change::Modified(path("a"))])
    );
}

#[test]
fn a_source_that_cannot_be_looked_at_at_the_start_is_an_error() {
    let folder = FakeFolder::with(&[("a", "1")]);
    folder.make_unreadable(Some("no such folder"));
    let error = Watcher::new(folder, SETTLE, INTERVAL).unwrap_err();
    assert_eq!(error, "no such folder");
}

// ---- loading beside, swapping when good

/// What a reload changes in these tests: the state in use, and a record of what the kind was asked.
#[derive(Debug, Default)]
struct World {
    in_use: String,
    swaps: usize,
}

/// A kind that "loads" the text of a file in a fake folder and puts it in `World::in_use`.
struct TextKind {
    folder: FakeFolder,
    /// Refuses the swap when the text says `refuse`, as a graphics card refuses a shader.
    refuse_swap: bool,
}

impl Reloadable<World> for TextKind {
    type Loaded = String;

    fn load(&self, _world: &World) -> Result<String, String> {
        let text = self.folder.text("a");
        if text.contains("broken") {
            Err("the file does not parse".to_owned())
        } else {
            Ok(text)
        }
    }

    fn swap(&self, world: &mut World, loaded: String) -> Result<Option<String>, String> {
        if self.refuse_swap {
            return Err("the graphics card refused it".to_owned());
        }
        if world.in_use == loaded {
            return Ok(None);
        }
        world.swaps += 1;
        world.in_use = loaded.clone();
        Ok(Some(format!("now `{loaded}`")))
    }
}

fn text_kind(folder: &FakeFolder) -> TextKind {
    TextKind {
        folder: folder.clone(),
        refuse_swap: false,
    }
}

#[test]
fn a_good_reload_puts_what_was_loaded_in_use_and_says_what_it_did() {
    let folder = FakeFolder::with(&[("a", "first")]);
    let mut world = World::default();
    let outcome = reload_once(&text_kind(&folder), &mut world);
    assert!(
        matches!(&outcome, Outcome::Reloaded { detail, .. } if detail == "now `first`"),
        "{outcome:?}"
    );
    assert_eq!(world.in_use, "first");
    assert_eq!(world.swaps, 1);
}

#[test]
fn a_load_that_fails_leaves_the_state_in_use_untouched() {
    let folder = FakeFolder::with(&[("a", "broken")]);
    let mut world = World {
        in_use: "working".to_owned(),
        swaps: 0,
    };
    let outcome = reload_once(&text_kind(&folder), &mut world);
    assert_eq!(
        outcome,
        Outcome::Failed {
            reason: "the file does not parse".to_owned()
        }
    );
    assert_eq!(world.in_use, "working");
    assert_eq!(world.swaps, 0, "nothing was swapped");
}

#[test]
fn a_swap_that_is_refused_is_a_failure_with_its_reason() {
    let folder = FakeFolder::with(&[("a", "fine")]);
    let mut world = World {
        in_use: "working".to_owned(),
        swaps: 0,
    };
    let kind = TextKind {
        refuse_swap: true,
        ..text_kind(&folder)
    };
    assert_eq!(
        reload_once(&kind, &mut world),
        Outcome::Failed {
            reason: "the graphics card refused it".to_owned()
        }
    );
    assert_eq!(world.in_use, "working");
}

#[test]
fn a_report_is_one_line_that_says_what_happened() {
    let good = Report {
        kind: "shader pack".to_owned(),
        outcome: Outcome::Reloaded {
            detail: "cel (2 settings)".to_owned(),
            took: Duration::from_millis(37),
        },
    };
    assert_eq!(
        good.line(),
        "reloaded shader pack: cel (2 settings) (37 ms)"
    );
    let bad = Report {
        kind: "shader pack".to_owned(),
        outcome: Outcome::Failed {
            reason: "line 3: no loops".to_owned(),
        },
    };
    assert_eq!(
        bad.line(),
        "reload of shader pack failed, nothing was changed: line 3: no loops"
    );
}

// ---- the kinds together

fn reloads_with(folder: &FakeFolder, watching: bool) -> Reloads<World> {
    let mut reloads = Reloads::new(watching);
    reloads
        .add("text", folder.clone(), SETTLE, INTERVAL, text_kind(folder))
        .expect("a kind that can be added");
    reloads
}

/// Polls from `from` to `to` ms in steps of the interval and gathers every report.
fn poll_through(
    reloads: &mut Reloads<World>,
    world: &mut World,
    from: u64,
    to: u64,
) -> Vec<Report> {
    (from..=to)
        .step_by(100)
        .flat_map(|time| reloads.poll(at(time), world))
        .collect()
}

#[test]
fn a_watched_change_reloads_by_itself_and_exactly_once() {
    let folder = FakeFolder::with(&[("a", "one")]);
    let mut reloads = reloads_with(&folder, true);
    let mut world = World::default();
    folder.write("a", "two");
    let reports = poll_through(&mut reloads, &mut world, 100, 1000);
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(matches!(reports[0].outcome, Outcome::Reloaded { .. }));
    assert_eq!(world.in_use, "two");
    assert_eq!(world.swaps, 1);
}

#[test]
fn without_watching_nothing_is_looked_at_and_nothing_reloads() {
    let folder = FakeFolder::with(&[("a", "one")]);
    let mut reloads = reloads_with(&folder, false);
    let mut world = World::default();
    let scans_at_start = folder.scans();
    folder.write("a", "two");
    assert_eq!(poll_through(&mut reloads, &mut world, 100, 1000), vec![]);
    assert_eq!(folder.scans(), scans_at_start, "no file was looked at");
    assert_eq!(world.swaps, 0);
    // A reload by hand works all the same.
    let reports = reloads.reload(None, &mut world).unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(world.in_use, "two");
}

#[test]
fn watching_can_be_turned_on_later() {
    let folder = FakeFolder::with(&[("a", "one")]);
    let mut reloads = reloads_with(&folder, false);
    let mut world = World::default();
    reloads.set_watching(true);
    assert!(reloads.watching());
    folder.write("a", "two");
    assert_eq!(poll_through(&mut reloads, &mut world, 100, 600).len(), 1);
}

#[test]
fn a_failed_reload_keeps_the_old_state_and_is_not_tried_again_until_the_files_change() {
    let folder = FakeFolder::with(&[("a", "working")]);
    let mut reloads = reloads_with(&folder, true);
    let mut world = World {
        in_use: "working".to_owned(),
        swaps: 0,
    };
    folder.write("a", "broken");
    let reports = poll_through(&mut reloads, &mut world, 100, 2000);
    assert_eq!(reports.len(), 1, "told once: {reports:?}");
    assert_eq!(
        reports[0].line(),
        "reload of text failed, nothing was changed: the file does not parse"
    );
    assert_eq!(world.in_use, "working");
    // Fixed by the owner: the next change reloads.
    folder.write("a", "fixed");
    let reports = poll_through(&mut reloads, &mut world, 2100, 3000);
    assert_eq!(reports.len(), 1);
    assert!(matches!(reports[0].outcome, Outcome::Reloaded { .. }));
    assert_eq!(world.in_use, "fixed");
}

#[test]
fn an_unreadable_source_is_reported_as_a_failed_reload_once() {
    let folder = FakeFolder::with(&[("a", "one")]);
    let mut reloads = reloads_with(&folder, true);
    let mut world = World::default();
    folder.make_unreadable(Some("busy"));
    let reports = poll_through(&mut reloads, &mut world, 100, 1000);
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].line(),
        "reload of text failed, nothing was changed: its files cannot be looked at: busy"
    );
}

#[test]
fn a_reload_by_name_touches_only_that_kind_and_one_without_a_name_touches_all() {
    let first = FakeFolder::with(&[("a", "first")]);
    let second = FakeFolder::with(&[("a", "second")]);
    let mut reloads = reloads_with(&first, false);
    reloads
        .add(
            "other",
            second.clone(),
            SETTLE,
            INTERVAL,
            text_kind(&second),
        )
        .unwrap();
    let mut world = World::default();
    let reports = reloads.reload(Some("other"), &mut world).unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].kind, "other");
    assert_eq!(world.in_use, "second");
    let reports = reloads.reload(None, &mut world).unwrap();
    assert_eq!(
        reports.iter().map(|r| r.kind.as_str()).collect::<Vec<_>>(),
        ["text", "other"],
        "in the order they were added"
    );
}

#[test]
fn a_kind_that_is_not_there_is_an_error_that_names_the_ones_that_are() {
    let folder = FakeFolder::with(&[("a", "one")]);
    let mut reloads = reloads_with(&folder, false);
    let mut world = World::default();
    let error = reloads.reload(Some("pictures"), &mut world).unwrap_err();
    assert_eq!(
        error,
        "there is nothing to reload called `pictures`; there is: text"
    );
    assert_eq!(world.swaps, 0, "an unknown name reloads nothing");
    let mut empty: Reloads<World> = Reloads::new(false);
    assert!(empty.reload(Some("x"), &mut world).is_err());
    assert_eq!(empty.reload(None, &mut world), Ok(vec![]));
}

#[test]
fn a_name_cannot_be_used_twice_and_a_source_that_cannot_be_read_is_not_added() {
    let folder = FakeFolder::with(&[("a", "one")]);
    let mut reloads = reloads_with(&folder, false);
    let error = reloads
        .add("text", folder.clone(), SETTLE, INTERVAL, text_kind(&folder))
        .unwrap_err();
    assert!(error.contains("already there"), "{error}");
    let broken = FakeFolder::default();
    broken.make_unreadable(Some("no such folder"));
    let error = reloads
        .add(
            "pictures",
            broken.clone(),
            SETTLE,
            INTERVAL,
            text_kind(&broken),
        )
        .unwrap_err();
    assert!(error.contains("no such folder"), "{error}");
    assert_eq!(reloads.names(), ["text"]);
}

// ---- files on disk

fn disk_watcher(root: &Path) -> Watcher<DiskSource> {
    Watcher::new(DiskSource::new(root), SETTLE, INTERVAL).expect("a folder that exists")
}

fn settle_and_poll(watcher: &mut Watcher<DiskSource>, from: u64) -> Poll {
    watcher.poll(at(from));
    watcher.poll(at(from + 100));
    watcher.poll(at(from + 200))
}

#[test]
fn a_folder_on_disk_is_scanned_through_its_folders_and_without_what_editors_leave() {
    let folder = Folder::new("reload_scan");
    folder.write("pack.json", "{}");
    folder.write("textures/stone.png", "p");
    folder.write("textures/deep/dirt.png", "q");
    folder.write(".pack.json.swp", "swap");
    folder.write("pack.json~", "backup");
    folder.write("textures/stone.png.tmp", "half");
    folder.write("#pack.json#", "lock");
    let mut source = DiskSource::new(folder.path());
    let names: Vec<String> = source
        .scan()
        .unwrap()
        .keys()
        .map(|path| {
            path.strip_prefix(folder.path())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(
        names,
        ["pack.json", "textures/deep/dirt.png", "textures/stone.png"]
    );
}

#[test]
fn a_single_file_is_a_source_and_a_missing_one_is_an_error() {
    let folder = Folder::new("reload_file");
    folder.write("lighting.wgsl", "fn pack_shade() {}");
    let mut source = DiskSource::new(folder.path().join("lighting.wgsl"));
    let files = source.scan().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[&folder.path().join("lighting.wgsl")].length,
        "fn pack_shade() {}".len() as u64
    );
    let mut missing = DiskSource::new(folder.path().join("nothing_here"));
    let error = missing.scan().unwrap_err();
    assert!(error.contains("nothing_here"), "{error}");
}

#[test]
fn the_digest_is_the_same_for_the_same_content_and_not_for_other_content() {
    let folder = Folder::new("reload_digest");
    folder.write("a", "same");
    folder.write("b", "same");
    folder.write("c", "samf");
    let mut source = DiskSource::new(folder.path());
    let a = source.digest(&folder.path().join("a")).unwrap();
    assert_eq!(a, source.digest(&folder.path().join("b")).unwrap());
    assert_ne!(a, source.digest(&folder.path().join("c")).unwrap());
    assert!(
        source
            .digest(&folder.path().join("missing"))
            .unwrap_err()
            .contains("missing")
    );
}

#[test]
fn a_file_written_on_disk_is_noticed_after_it_settles() {
    let folder = Folder::new("reload_write");
    folder.write("lighting.wgsl", "one");
    let mut watcher = disk_watcher(folder.path());
    folder.write("lighting.wgsl", "longer than one");
    folder.write("added.png", "p");
    let Poll::Changed(changes) = settle_and_poll(&mut watcher, 100) else {
        panic!("the files changed and were still");
    };
    assert_eq!(
        changes,
        vec![
            Change::Added(folder.path().join("added.png")),
            Change::Modified(folder.path().join("lighting.wgsl")),
        ]
    );
}

#[test]
fn on_disk_a_new_modification_time_with_the_same_content_is_not_a_change_and_the_same_length_with_other_content_is()
 {
    let folder = Folder::new("reload_stamp");
    folder.write("a", "abcd");
    let file = folder.path().join("a");
    let mut watcher = disk_watcher(folder.path());
    let later = SystemTime::now() + Duration::from_secs(60);
    let set_modified = |when: SystemTime| {
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(when)
            .unwrap();
    };
    set_modified(later);
    assert_eq!(settle_and_poll(&mut watcher, 100), Poll::Quiet);
    // Another content of the same length, written at another time.
    std::fs::write(&file, "abce").unwrap();
    set_modified(later + Duration::from_secs(60));
    assert_eq!(
        settle_and_poll(&mut watcher, 400),
        Poll::Changed(vec![Change::Modified(file)])
    );
}

#[test]
fn a_folder_that_goes_away_is_unreadable_and_comes_back() {
    let folder = Folder::new("reload_gone");
    folder.write("sub/a", "1");
    let sub = folder.path().join("sub");
    let mut watcher = disk_watcher(&sub);
    std::fs::remove_dir_all(&sub).unwrap();
    assert!(matches!(watcher.poll(at(100)), Poll::Unreadable(_)));
    // (Another length: two writes of one length within the clock's step have one stamp, which `FileStamp` says.)
    folder.write("sub/a", "22");
    assert_eq!(
        watcher.poll(at(200)),
        Poll::Waiting,
        "a rewritten file has a new stamp: it is looked at again"
    );
}

#[test]
fn what_is_in_use_already_is_unchanged_silent_when_watching_and_said_when_asked() {
    let folder = FakeFolder::with(&[("a", "one")]);
    let mut reloads = reloads_with(&folder, true);
    let mut world = World {
        in_use: "one".to_owned(),
        swaps: 0,
    };
    // The file changes, and what is loaded is what is in use already (the game wrote it itself).
    folder.write("a", "two");
    world.in_use = "two".to_owned();
    assert_eq!(poll_through(&mut reloads, &mut world, 100, 1000), vec![]);
    let reports = reloads.reload(None, &mut world).unwrap();
    assert_eq!(reports[0].outcome, Outcome::Unchanged);
    assert_eq!(reports[0].line(), "text: nothing has changed");
    assert_eq!(world.swaps, 0);
}

#[test]
fn several_folders_are_one_source_and_each_file_is_read_in_its_own() {
    let first = Folder::new("reload_multi_a");
    let second = Folder::new("reload_multi_b");
    first.write("one", "1");
    second.write("two", "22");
    let mut source = DiskSources::new(vec![
        DiskSource::new(first.path()),
        DiskSource::new(second.path()),
    ]);
    let files = source.scan().unwrap();
    assert_eq!(files.len(), 2);
    let one = source.digest(&first.path().join("one")).unwrap();
    assert_ne!(one, source.digest(&second.path().join("two")).unwrap());
    assert!(source.digest(Path::new("elsewhere")).is_err());
}
