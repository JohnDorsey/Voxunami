//! Data packs at the shell: loading a directory of them into a game's content when a server starts, and the two
//! commands for people who write them: `check-pack` (what the server would say about these packs) and `pack-schema`
//! (the JSON Schemas editors use).

use std::path::{Path, PathBuf};

use data_packs::{ReadPack, pack_identities, pack_schemas, read_manifest, read_pack_directory};
use server::ServedPack;

use crate::definition::{GameContent, GameDefinition};

/// A new copy of game `G`'s content with the packs in `packs` (a directory of pack folders) loaded, if there are
/// any. What a pack skipped on purpose is printed as a warning; any other problem is an error that lists all of
/// them, and the caller does not start.
pub fn content_with_packs<G: GameDefinition>(packs: Option<&Path>) -> Result<G::Content, String> {
    let mut content = G::new_content();
    if let Some(directory) = packs {
        load_into(&mut content, directory)?;
    }
    Ok(content)
}

/// What loading a directory of packs did.
struct Summary {
    packs: usize,
    blocks_added: usize,
    bodies_added: usize,
    patches: usize,
    warnings: Vec<String>,
}

/// The packs among `packs` that clients keep as files (every one whose manifest does not say `"replicate": false`), as a server serves them: the whole archive of
/// each, every file sent except those in the pack's `server` folder (`data_packs::is_sent_to_clients`: the files a server keeps to itself, which are in the
/// index and so in the pack's identity, and which no client is sent), in the order of `packs`.
pub(crate) fn served_packs(packs: &[ReadPack]) -> Result<Vec<ServedPack>, String> {
    let mut served = Vec::new();
    for pack in packs {
        let file = format!("{}/pack.json", pack.raw.folder);
        let manifest =
            read_manifest(&file, &pack.raw.manifest).map_err(|errors| errors.to_string())?;
        if !manifest.replicate {
            continue;
        }
        let one = ServedPack::new(&manifest.id, &pack.archive, data_packs::is_sent_to_clients)
            .map_err(|error| format!("{file}: {error}"))?;
        served.push(one);
    }
    Ok(served)
}

/// How many files of each pack in `directory` clients are not sent (those in its `server` folder), for the packs that have any. Empty if the directory cannot be
/// read, which the check says elsewhere.
fn files_kept_back(directory: &Path) -> std::collections::BTreeMap<String, usize> {
    read_pack_directory(directory)
        .map(|packs| {
            packs
                .into_iter()
                .filter_map(|pack| {
                    let count = pack
                        .archive
                        .files()
                        .filter(|(path, _)| !data_packs::is_sent_to_clients(path))
                        .count();
                    (count > 0).then_some((pack.raw.folder, count))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn load_into(content: &mut impl GameContent, directory: &Path) -> Result<Summary, String> {
    let read =
        read_pack_directory(directory).map_err(|errors| problems(&errors.to_string(), None))?;
    let raw: Vec<data_packs::RawPack> = read.iter().map(|pack| pack.raw.clone()).collect();
    let served = served_packs(&read).map_err(|text| problems(&text, None))?;
    let blocks_before = content.package_data().blocks.len();
    let bodies_before = content.body_names().len();
    let warnings = content
        .apply_packs(&raw)
        .map_err(|errors| problems(&errors.to_string(), Some(raw.len())))?;
    for warning in &warnings {
        eprintln!("warning: {warning}");
    }
    content.set_replicated_packs(served);
    Ok(Summary {
        packs: raw.len(),
        blocks_added: content.package_data().blocks.len() - blocks_before,
        bodies_added: content.body_names().len() - bodies_before,
        patches: raw.iter().map(|pack| pack.patches.len()).sum(),
        warnings,
    })
}

/// Reads the packs in `directory` again and replaces the package of the server that runs on `content` (`package_reload::reload_packs`), between two of its ticks.
/// What it did, or why it did not, in words for whoever runs the server: a pack that is no good changes nothing.
pub(crate) fn reload_from_directory(
    content: &mut impl GameContent,
    directory: &Path,
    package: &crate::SharedPackage,
    server: &mut server::Server,
) -> Result<String, String> {
    let read =
        read_pack_directory(directory).map_err(|errors| problems(&errors.to_string(), None))?;
    let raw: Vec<data_packs::RawPack> = read.iter().map(|pack| pack.raw.clone()).collect();
    let served = served_packs(&read).map_err(|text| problems(&text, None))?;
    let reloaded = crate::package_reload::reload_packs(content, &raw, served, package, server)
        .map_err(|error| problems(&error.to_string(), Some(raw.len())))?;
    for warning in &reloaded.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(format!(
        "{} pack{} read again, {} block{} added; the package has been replaced {} time{}",
        raw.len(),
        plural(raw.len()),
        reloaded.blocks_added,
        plural(reloaded.blocks_added),
        reloaded.version,
        plural(reloaded.version as usize),
    ))
}

/// Watches a folder of packs for a server that reads it again by itself (`--watch`): the files are looked at a few times a second, a change counts when the files have
/// been still for a moment (an editor's two-step save never loads half a file) and their content differs, and then the folder is read again as `/reload` does (a pack
/// that is no good changes nothing, and is not tried again until the files change). Every file counts, a note and the files of the `server` folder too, because each is
/// in the pack's identity.
pub(crate) struct PackWatch {
    directory: PathBuf,
    watcher: crate::reload::Watcher<crate::reload::DiskSource>,
    started: std::time::Instant,
}

impl PackWatch {
    /// Takes the files of `directory` as they are now as seen.
    pub(crate) fn new(directory: &Path) -> Result<Self, String> {
        let watcher = crate::reload::Watcher::new(
            crate::reload::DiskSource::new(directory),
            crate::reload::Watcher::<crate::reload::DiskSource>::DEFAULT_SETTLE,
            crate::reload::Watcher::<crate::reload::DiskSource>::DEFAULT_INTERVAL,
        )
        .map_err(|reason| format!("cannot watch {}: {reason}", directory.display()))?;
        Ok(Self {
            directory: directory.to_owned(),
            watcher,
            started: std::time::Instant::now(),
        })
    }

    /// Looks at the folder, and if its files changed and are whole reads the packs again and replaces the server's package and list. `None` when nothing changed; else
    /// what was done, or why nothing was. Call between two ticks of the server (`reload_from_directory` says why).
    pub(crate) fn poll(
        &mut self,
        content: &mut impl GameContent,
        package: &crate::SharedPackage,
        server: &mut server::Server,
    ) -> Option<Result<String, String>> {
        match self.watcher.poll(self.started.elapsed()) {
            crate::reload::Poll::Quiet | crate::reload::Poll::Waiting => None,
            crate::reload::Poll::Unreadable(reason) => Some(Err(format!(
                "the packs folder cannot be looked at: {reason}"
            ))),
            crate::reload::Poll::Changed(_) => Some(reload_from_directory(
                content,
                &self.directory,
                package,
                server,
            )),
        }
    }
}

fn problems(text: &str, packs: Option<usize>) -> String {
    let count = text.lines().count();
    let in_packs = packs.map_or(String::new(), |packs| {
        format!(" in {packs} pack{}", plural(packs))
    });
    format!(
        "{text}\n{count} problem{}{in_packs}; nothing was loaded",
        if count == 1 { "" } else { "s" }
    )
}

const CHECK_USAGE: &str = "Check data packs the way a server loads them.

Usage: voxunami check-pack GAME DIRECTORY
  DIRECTORY holds a folder or a canonical .zip for each pack, each with a pack.json and the folders blocks, patches, bodies and textures.
  After the check each pack's root hash is printed: the pack's identity, the same for a folder and for its zip.
  Every problem found is printed with its file and the place in it; the exit status says whether there were any.
  `voxunami pack-schema GAME DIRECTORY` writes the JSON Schemas that editors use to check the files as you type.";

const SCHEMA_USAGE: &str = "Write the JSON Schemas of pack files for a game.

Usage: voxunami pack-schema GAME DIRECTORY
  Writes pack.schema.json, block.schema.json, patch.schema.json and body.schema.json into DIRECTORY (made if it is not there),
  made from the properties, families of generators and levels the game declares. Name them in the `$schema` key of a pack's files, or give them to
  an editor, to complete and check the files as you type.";

/// Checks the packs in the directory named by the arguments against game `G`. `arguments` are the command line after
/// the game's name.
pub fn run_check_packs<G: GameDefinition>(
    arguments: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    let Some(directory) = one_directory(arguments, CHECK_USAGE)? else {
        return Ok(());
    };
    let mut content = G::new_content();
    let summary = load_into(&mut content, &directory)?;
    println!(
        "{}: {} pack{}, {} block{} added, {} bod{} added, {} patch file{}{}",
        G::command(),
        summary.packs,
        plural(summary.packs),
        summary.blocks_added,
        plural(summary.blocks_added),
        summary.bodies_added,
        if summary.bodies_added == 1 {
            "y"
        } else {
            "ies"
        },
        summary.patches,
        plural(summary.patches),
        if summary.warnings.is_empty() {
            ", no problems".to_owned()
        } else {
            format!(
                ", {} skipped on purpose (see the warnings above)",
                summary.warnings.len()
            )
        }
    );
    // what a pack is, whatever its form (the root hash of its file index): two servers run the same pack when these are equal
    let kept_back = files_kept_back(&directory);
    if let Ok(identities) = pack_identities(&directory) {
        for (name, identity) in identities {
            match identity {
                Ok(root) => match kept_back.get(&name).copied().unwrap_or(0) {
                    0 => println!("  {name}: root {root}"),
                    count => println!(
                        "  {name}: root {root} ({count} file{} in `server` kept back: in the root, not sent to clients)",
                        plural(count)
                    ),
                },
                Err(why) => {
                    println!("  {name}: no root hash yet, it cannot be made into an archive: {why}")
                }
            }
        }
    }
    Ok(())
}

/// Writes the schemas of game `G`'s pack files into the directory named by the arguments.
pub fn run_pack_schemas<G: GameDefinition>(
    arguments: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    let Some(directory) = one_directory(arguments, SCHEMA_USAGE)? else {
        return Ok(());
    };
    let content = G::new_content();
    let schemas = pack_schemas(
        &content.package_data().properties,
        &content.body_support().context(),
    );
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot make {}: {error}", directory.display()))?;
    for (name, text) in schemas.files() {
        let path = directory.join(name);
        std::fs::write(&path, text)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// The one directory on a command line, or `None` after printing the help.
fn one_directory(
    arguments: impl IntoIterator<Item = String>,
    usage: &str,
) -> Result<Option<PathBuf>, String> {
    let arguments: Vec<String> = arguments.into_iter().collect();
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!("{usage}");
        return Ok(None);
    }
    match arguments.as_slice() {
        [directory] if !directory.starts_with('-') => Ok(Some(PathBuf::from(directory))),
        _ => Err(format!("give one directory\n\n{usage}")),
    }
}

#[cfg(test)]
mod tests {
    use data_packs::{ReadPack, read_archive};
    use pack_archive::{Archive, FileIndex};

    use super::served_packs;

    fn read_pack(pack_id: &str, manifest: &str, extra: &[(&str, &[u8])]) -> ReadPack {
        let mut files = vec![
            ("pack.json".to_owned(), manifest.as_bytes().to_vec()),
            (
                "blocks/a.json".to_owned(),
                br#"{ "color": [1, 0, 0] }"#.to_vec(),
            ),
        ];
        files.extend(
            extra
                .iter()
                .map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec())),
        );
        let archive = Archive::from_files(files).unwrap();
        let raw = read_archive(pack_id, &archive).unwrap();
        ReadPack { raw, archive }
    }

    #[test]
    fn a_pack_is_replicated_with_all_its_files_unless_its_manifest_says_it_is_not() {
        let kept = read_pack(
            "kept",
            r#"{ "id": "kept" }"#,
            &[("readme.md", b"a note"), ("textures/p.png", b"x")],
        );
        let private = read_pack("private", r#"{ "id": "private", "replicate": false }"#, &[]);
        let explicit = read_pack(
            "explicit",
            r#"{ "id": "explicit", "replicate": true }"#,
            &[],
        );
        let served = served_packs(&[kept.clone(), private, explicit]).unwrap();
        let ids: Vec<&str> = served
            .iter()
            .map(|pack| pack.listing().id.as_str())
            .collect();
        assert_eq!(
            ids,
            ["kept", "explicit"],
            "in the order of the directory, those that say no left out"
        );
        // all the files are sent, a note too, and the identity is that of the index of the whole archive
        let listing = served[0].listing();
        assert!(listing.index.entries().iter().all(|entry| entry.sent));
        assert_eq!(
            listing.root(),
            FileIndex::of(&kept.archive, |_| true).root_hash()
        );
        assert_eq!(served[0].archive_bytes(), kept.archive.to_bytes());
        assert!(
            Archive::from_bytes(served[0].archive_bytes(), &Default::default())
                .unwrap()
                .get("readme.md")
                .is_some()
        );
    }

    #[test]
    fn what_is_in_the_server_folder_is_in_the_index_and_in_the_identity_and_is_not_sent() {
        let extra = |code: &'static [u8]| {
            read_pack(
                "mine",
                r#"{ "id": "mine" }"#,
                &[("server/rules.py", code), ("readme.md", b"a note")],
            )
        };
        let pack = extra(b"print(1)");
        let served = served_packs(std::slice::from_ref(&pack)).unwrap();
        let listing = served[0].listing();
        let not_sent: Vec<&str> = listing
            .index
            .entries()
            .iter()
            .filter(|entry| !entry.sent)
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(not_sent, ["server/rules.py"]);
        // a client is sent the archive without it, and its size says so
        let sent = Archive::from_bytes(served[0].archive_bytes(), &Default::default()).unwrap();
        assert!(sent.get("server/rules.py").is_none());
        assert!(sent.get("readme.md").is_some());
        assert_eq!(
            listing.archive_size as usize,
            served[0].archive_bytes().len()
        );
        assert!(served[0].archive_bytes().len() < pack.archive.to_bytes().len());
        // what the client holds, checked as a client checks what it was sent
        assert!(listing.index.verify_received(&sent).is_ok());
        // the identity is the one tools print: the same rule, so they cannot differ
        assert_eq!(
            listing.root(),
            FileIndex::of(&pack.archive, data_packs::is_sent_to_clients).root_hash()
        );
        // one byte of code is another pack, and the same files are the same pack
        assert_ne!(
            served_packs(&[extra(b"print(2)")]).unwrap()[0].root(),
            served[0].root()
        );
        assert_eq!(
            served_packs(&[extra(b"print(1)")]).unwrap()[0].root(),
            served[0].root()
        );
    }

    #[test]
    fn check_pack_counts_the_files_kept_back_of_each_pack_that_has_some() {
        let directory =
            std::env::temp_dir().join(format!("files_kept_back_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        for (path, text) in [
            ("a/pack.json", r#"{ "id": "a" }"#),
            ("a/server/one.py", "1"),
            ("a/server/deeper/two.py", "2"),
            ("b/pack.json", r#"{ "id": "b" }"#),
            ("b/readme.md", "a note"),
        ] {
            let file = directory.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        let kept = super::files_kept_back(&directory);
        std::fs::remove_dir_all(&directory).unwrap();
        assert_eq!(kept.into_iter().collect::<Vec<_>>(), [("a".to_owned(), 2)]);
    }

    #[test]
    fn the_pack_is_named_by_its_manifest_and_a_manifest_that_cannot_be_read_is_an_error() {
        // (The folder's name must be the pack's id, which the loader checks; the manifest is what names the pack.)
        let pack = read_pack("whatever", r#"{ "id": "named_by_manifest" }"#, &[]);
        assert_eq!(
            served_packs(&[pack]).unwrap()[0].listing().id,
            "named_by_manifest"
        );
        let mut broken = read_pack("broken", r#"{ "id": "broken" }"#, &[]);
        broken.raw.manifest = r#"{ "id": 5 }"#.to_owned();
        let error = served_packs(&[broken]).unwrap_err();
        assert!(error.contains("pack.json"), "{error}");
    }
}
