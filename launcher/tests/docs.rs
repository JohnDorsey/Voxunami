//! The documents refer to each other and the code refers to them (a comment says "see `docs/specs/lod-spec.md`"). A reference to a
//! file that is not there sends the next reader, who is often a program, looking for nothing; so every one is checked.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Every file of the repository that may mention a document, as (path relative to the root, text).
fn texts() -> Vec<(String, String)> {
    fn walk(directory: &Path, root: &Path, found: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(directory).unwrap().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                // Not ours: builds, the version control's own files, other checkouts, and the test worlds of a person.
                if matches!(
                    name.as_str(),
                    "target" | ".git" | ".claude" | "node_modules"
                ) || name.starts_with("testworld")
                {
                    continue;
                }
                walk(&path, root, found);
            } else if [".rs", ".md", ".toml", ".yml"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
                && let Ok(text) = fs::read_to_string(&path)
            {
                let relative = path.strip_prefix(root).unwrap().to_string_lossy();
                found.push((relative.replace('\\', "/"), text));
            }
        }
    }
    let root = root();
    let mut found = Vec::new();
    walk(&root, &root, &mut found);
    found
}

/// The words of `text` that start with `docs/` and end in a document or picture, as written.
fn document_paths(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (start, _) in text.match_indices("docs/") {
        // `docs/` must begin a path: not the end of another word (`mydocs/`).
        if text[..start]
            .chars()
            .next_back()
            .is_some_and(|before| before.is_alphanumeric() || before == '_' || before == '-')
        {
            continue;
        }
        let tail = &text[start..];
        let length = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || "_-./".contains(c)))
            .unwrap_or(tail.len());
        let word = tail[..length].trim_end_matches('.');
        if word.ends_with(".md") || word.ends_with(".png") {
            found.push(word.to_owned());
        }
    }
    found
}

#[test]
fn every_document_the_code_and_the_documents_name_by_path_exists() {
    let root = root();
    let mut missing = Vec::new();
    for (file, text) in texts() {
        for path in document_paths(&text) {
            // A path with a placeholder (`docs/NAME.md`) or a glob is a pattern, not a reference.
            if path.contains("NAME") || path.contains('*') {
                continue;
            }
            if !root.join(&path).is_file() {
                missing.push(format!("{file}: {path}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "references to documents that are not there:\n{}",
        missing.join("\n")
    );
}

#[test]
fn every_document_named_by_its_file_name_alone_is_somewhere_under_docs_and_only_once() {
    // The documents say "see `lod-spec.md`" without the folder, which stays true when the folders change, as long as the name is one document's.
    let root = root();
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (file, _) in texts() {
        if let Some(rest) = file.strip_prefix("docs/")
            && file.ends_with(".md")
        {
            let name = rest.rsplit('/').next().unwrap().to_owned();
            by_name.entry(name).or_default().push(file);
        }
    }
    let twice: Vec<_> = by_name
        .iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(name, files)| format!("{name}: {}", files.join(", ")))
        .collect();
    assert!(
        twice.is_empty(),
        "two documents with one name:\n{}",
        twice.join("\n")
    );

    let outside_docs = ["README.md", "CLAUDE.md"];
    let mut missing = Vec::new();
    for (file, text) in texts() {
        if !file.ends_with(".md") {
            continue;
        }
        for token in text.split('`').skip(1).step_by(2) {
            let token = token.trim();
            let is_bare_document = token.ends_with(".md")
                && !token.contains('/')
                && !token.contains(' ')
                && !token.contains('*')
                && !token.contains("NAME")
                && token
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.".contains(c));
            if is_bare_document
                && !by_name.contains_key(token)
                && !outside_docs.contains(&token)
                && !root.join(token).is_file()
            {
                missing.push(format!("{file}: `{token}`"));
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "documents named by file name that are nowhere:\n{}",
        missing.join("\n")
    );
}
