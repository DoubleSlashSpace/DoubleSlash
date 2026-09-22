//! The folder the Ollama assistant may share files from.
//!
//! Opt-in under Settings > AI, next to re-sharing chat attachments. Whoever
//! chats with the assistant can ask it for any file, so the folder is enforced
//! on the *resolved* path: `..`, symlinks, and absolute paths that lead outside
//! it are refused. A folder that overlaps the DoubleSlash profile — which holds
//! the identity keys and the chat database — is refused outright.

use std::path::{Path, PathBuf};

use crate::file_transfer::MAX_TRANSFER_SIZE;

/// Deepest subfolder level that is listed and searched by bare file name.
const MAX_DEPTH: usize = 4;
/// Most files one listing returns.
const MAX_LISTED: usize = 200;

/// A configured share folder that passed validation.
#[derive(Debug, Clone)]
pub struct ShareFolder {
    /// Canonical, so containment checks compare like with like.
    root: PathBuf,
}

/// One file in the share folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFile {
    /// Path relative to the folder, `/`-separated.
    pub rel: String,
    pub size: u64,
}

impl ShareFolder {
    /// Folders a share folder must not overlap: the profile root, which holds
    /// every profile's keys and history, and the profile in use.
    pub fn protected_dirs() -> Vec<PathBuf> {
        vec![
            crate::identity::Identity::default_profile_root(),
            crate::identity::Identity::default_key_dir(),
        ]
    }

    /// Validate the configured folder; `Ok(None)` when none is set.
    ///
    /// Refused when it contains, or sits inside, any of `protected`.
    pub fn open(setting: &str, protected: &[PathBuf]) -> Result<Option<Self>, String> {
        let setting = setting.trim();
        if setting.is_empty() {
            return Ok(None);
        }
        let root = std::fs::canonicalize(setting)
            .map_err(|e| format!("the shared folder is unavailable: {e}"))?;
        if !root.is_dir() {
            return Err("the shared folder is not a folder".into());
        }
        // A folder that does not exist yet cannot overlap anything.
        for dir in protected {
            let Ok(dir) = std::fs::canonicalize(dir) else {
                continue;
            };
            if dir.starts_with(&root) || root.starts_with(&dir) {
                return Err(
                    "the shared folder overlaps the DoubleSlash profile folder, \
                     which holds your keys and chat history; pick a folder that \
                     contains only files you want to share"
                        .into(),
                );
            }
        }
        Ok(Some(Self { root }))
    }

    /// Files under the folder, sorted by path. `true` when the list was cut
    /// short at [`MAX_LISTED`].
    ///
    /// Symlinks and dot-entries are skipped; the walk stops at [`MAX_DEPTH`].
    pub fn list(&self) -> (Vec<SharedFile>, bool) {
        let mut out = Vec::new();
        let truncated = walk(&self.root, "", 0, &mut out);
        (out, truncated)
    }

    /// Resolve a model-supplied name to a file inside the folder.
    ///
    /// Accepts a path relative to the folder (`docs/a.pdf`), an absolute path
    /// inside it, or a bare file name that names exactly one listed file
    /// (case-insensitive). Returns the path to hand to the transfer.
    pub fn resolve(&self, query: &str) -> Result<PathBuf, String> {
        let q = query.trim();
        if q.is_empty() {
            return Err("empty file name".into());
        }
        let asked = Path::new(q);
        let joined = if asked.is_absolute() {
            asked.to_path_buf()
        } else {
            self.root.join(asked)
        };
        if let Ok(real) = std::fs::canonicalize(&joined) {
            if !real.starts_with(&self.root) {
                return Err(format!("'{q}' is outside the shared folder"));
            }
            return checked_file(real, q);
        }
        if asked.components().count() == 1 {
            let key = q.to_lowercase();
            let (files, _) = self.list();
            let hits: Vec<&SharedFile> = files
                .iter()
                .filter(|f| f.rel.rsplit('/').next().unwrap_or(&f.rel).to_lowercase() == key)
                .collect();
            match hits.as_slice() {
                [one] => return self.resolve(&one.rel),
                [] => {}
                many => {
                    let names: Vec<&str> = many.iter().map(|f| f.rel.as_str()).collect();
                    return Err(format!(
                        "{} files are named '{q}' ({}); pass the path",
                        many.len(),
                        names.join(", ")
                    ));
                }
            }
        }
        Err(format!("no file '{q}' in the shared folder"))
    }
}

/// Collect files below `dir` into `out`; `true` if the cap cut the walk short.
fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<SharedFile>) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        // `DirEntry::file_type` does not follow links, so a link is never
        // listed — even one that points back inside the folder.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if kind.is_dir() {
            if depth + 1 < MAX_DEPTH && walk(&entry.path(), &rel, depth + 1, out) {
                return true;
            }
        } else if kind.is_file() {
            if out.len() >= MAX_LISTED {
                return true;
            }
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(SharedFile { rel, size });
        }
    }
    false
}

fn checked_file(real: PathBuf, asked: &str) -> Result<PathBuf, String> {
    let meta = std::fs::metadata(&real).map_err(|e| format!("cannot read '{asked}': {e}"))?;
    if !meta.is_file() {
        return Err(format!("'{asked}' is not a file"));
    }
    if meta.len() > MAX_TRANSFER_SIZE as u64 {
        return Err(format!(
            "'{asked}' is {} bytes, over the {MAX_TRANSFER_SIZE}-byte transfer limit",
            meta.len()
        ));
    }
    Ok(plain_path(real))
}

/// Drop the `\\?\` prefix Windows puts on canonical paths, so the chat record
/// holds an ordinary path the UI can open.
fn plain_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    if let Some(s) = path.to_str() {
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            if rest.as_bytes().get(1) == Some(&b':') {
                return PathBuf::from(rest);
            }
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder_with_files() -> (tempfile::TempDir, ShareFolder) {
        let dir = tempfile::tempdir().unwrap();
        let share = dir.path().join("share");
        std::fs::create_dir_all(share.join("docs")).unwrap();
        std::fs::create_dir_all(share.join(".git")).unwrap();
        std::fs::write(share.join("notes.txt"), b"hello").unwrap();
        std::fs::write(share.join("docs").join("Report Q3.pdf"), b"pdf").unwrap();
        std::fs::write(share.join(".git").join("config"), b"secret").unwrap();
        std::fs::write(dir.path().join("outside.txt"), b"nope").unwrap();
        let folder = ShareFolder::open(
            share.to_str().unwrap(),
            &[dir.path().join("profile-not-created")],
        )
        .unwrap()
        .unwrap();
        (dir, folder)
    }

    #[test]
    fn no_folder_configured_is_not_an_error() {
        assert!(ShareFolder::open("  ", &[PathBuf::from("/nowhere")])
            .unwrap()
            .is_none());
    }

    #[test]
    fn lists_files_but_not_hidden_ones() {
        let (_dir, folder) = folder_with_files();
        let (files, truncated) = folder.list();
        let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["docs/Report Q3.pdf", "notes.txt"]);
        assert_eq!(files[1].size, 5);
        assert!(!truncated);
    }

    #[test]
    fn resolves_relative_paths_and_unique_bare_names() {
        let (_dir, folder) = folder_with_files();
        let by_path = folder.resolve("docs/Report Q3.pdf").unwrap();
        assert!(by_path.ends_with("Report Q3.pdf"));
        let by_name = folder.resolve("report q3.PDF").unwrap();
        assert_eq!(by_name, by_path, "a bare name finds the one file");
        assert!(folder.resolve("missing.bin").is_err());
    }

    /// The model is steered by whoever it chats with; nothing outside the
    /// folder may be reachable through it.
    #[test]
    fn refuses_anything_outside_the_folder() {
        let (dir, folder) = folder_with_files();
        let err = folder.resolve("../outside.txt").unwrap_err();
        assert!(err.contains("outside the shared folder"), "{err}");
        let abs = dir.path().join("outside.txt");
        let err = folder.resolve(abs.to_str().unwrap()).unwrap_err();
        assert!(err.contains("outside the shared folder"), "{err}");
        assert!(folder.resolve("docs").is_err(), "a folder is not a file");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_folder_is_refused() {
        let (dir, folder) = folder_with_files();
        let link = dir.path().join("share").join("escape.txt");
        std::os::unix::fs::symlink(dir.path().join("outside.txt"), &link).unwrap();
        assert!(folder.resolve("escape.txt").is_err());
        assert!(folder.list().0.iter().all(|f| f.rel != "escape.txt"));
    }

    #[test]
    fn a_folder_overlapping_the_profile_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("doubleslash");
        let active = root.join("profiles").join("active");
        let sibling = root.join("profiles").join("other");
        std::fs::create_dir_all(active.join("attachments")).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let protected = [root.clone(), active.clone()];
        // A parent would expose the keys; a subfolder whatever the client
        // keeps there; a sibling profile its own identity.
        for share in [
            dir.path().to_path_buf(),
            active.join("attachments"),
            sibling,
        ] {
            let err = ShareFolder::open(share.to_str().unwrap(), &protected).unwrap_err();
            assert!(err.contains("profile"), "{err}");
        }
        let unrelated = dir.path().join("share");
        std::fs::create_dir_all(&unrelated).unwrap();
        assert!(ShareFolder::open(unrelated.to_str().unwrap(), &protected)
            .unwrap()
            .is_some());
    }

    #[cfg(windows)]
    #[test]
    fn resolved_paths_drop_the_verbatim_prefix() {
        let (_dir, folder) = folder_with_files();
        let path = folder.resolve("notes.txt").unwrap();
        assert!(!path.to_string_lossy().starts_with(r"\\?\"), "{path:?}");
    }
}
