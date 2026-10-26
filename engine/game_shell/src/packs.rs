//! Data packs at the shell: loading a directory of them into a game's content when a server starts, and the two
//! commands for people who write them: `check-pack` (what the server would say about these packs) and `pack-schema`
//! (the JSON Schemas editors use).

use std::path::{Path, PathBuf};

use data_packs::{pack_schemas, read_directory};

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

fn load_into(content: &mut impl GameContent, directory: &Path) -> Result<Summary, String> {
    let raw = read_directory(directory).map_err(|errors| problems(&errors.to_string(), None))?;
    let blocks_before = content.package_data().blocks.len();
    let bodies_before = content.body_names().len();
    let warnings = content
        .apply_packs(&raw)
        .map_err(|errors| problems(&errors.to_string(), Some(raw.len())))?;
    for warning in &warnings {
        eprintln!("warning: {warning}");
    }
    Ok(Summary {
        packs: raw.len(),
        blocks_added: content.package_data().blocks.len() - blocks_before,
        bodies_added: content.body_names().len() - bodies_before,
        patches: raw.iter().map(|pack| pack.patches.len()).sum(),
        warnings,
    })
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
  DIRECTORY holds a folder for each pack, each with a pack.json and the folders blocks, patches and bodies.
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
