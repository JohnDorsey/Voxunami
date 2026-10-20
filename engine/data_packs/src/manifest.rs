//! A pack's `pack.json`: who it is and what it needs.

use namespaced_id::NamespacedId;
use serde_json::Value;

use crate::error::PackErrors;
use crate::walk::{Path, Walk};

/// The namespaces no pack may use: the engine's own.
const RESERVED: [&str; 1] = ["engine"];

/// What `pack.json` says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackManifest {
    /// The pack's name, which is also the namespace of the blocks it adds (`my_pack` for `my_pack:marble`).
    pub id: String,
    /// Packs (or plugins) that load before this one, by their ids.
    pub depends_on: Vec<String>,
}

/// Whether `text` can be a pack's id: it is the namespace of an identifier.
pub(crate) fn check_pack_id(walk: &mut Walk, text: &str, path: &Path) -> bool {
    if let Err(error) = NamespacedId::new(text, "x") {
        walk.fail(path, format!("\"{text}\" is not a pack name: {error}"));
        return false;
    }
    if RESERVED.contains(&text) {
        walk.fail(path, format!("\"{text}\" is reserved for the engine"));
        return false;
    }
    true
}

pub fn read_manifest(file: &str, text: &str) -> Result<PackManifest, PackErrors> {
    let (mut walk, value) = Walk::parse(file, text)?;
    let root = Path::root();
    let manifest = read(&mut walk, &value, &root);
    match manifest {
        Some(manifest) => walk.finish(manifest),
        None => Err(walk
            .finish(())
            .expect_err("a manifest that was not read had a problem")),
    }
}

fn read(walk: &mut Walk, value: &Value, path: &Path) -> Option<PackManifest> {
    let object = walk.object(value, path, &["id", "depends_on"])?;
    let id_path = path.key("id");
    let id = walk
        .require(object, "id", path)
        .and_then(|id| walk.string(id, &id_path))
        .filter(|id| check_pack_id(walk, id, &id_path))
        .map(str::to_owned);
    let dependencies_path = path.key("depends_on");
    let depends_on = match object.get("depends_on") {
        None => Some(Vec::new()),
        Some(list) => walk.array(list, &dependencies_path).and_then(|list| {
            let mut names = Vec::<String>::new();
            let mut good = true;
            for (index, entry) in list.iter().enumerate() {
                let entry_path = dependencies_path.index(index);
                let Some(name) = walk.string(entry, &entry_path) else {
                    good = false;
                    continue;
                };
                if !check_pack_id(walk, name, &entry_path) {
                    good = false;
                } else if names.iter().any(|earlier| earlier == name) {
                    walk.fail(&entry_path, format!("\"{name}\" is listed twice"));
                    good = false;
                } else if Some(name) == id.as_deref() {
                    walk.fail(&entry_path, "a pack cannot depend on itself");
                    good = false;
                } else {
                    names.push(name.to_owned());
                }
            }
            good.then_some(names)
        }),
    };
    Some(PackManifest {
        id: id?,
        depends_on: depends_on?,
    })
}
