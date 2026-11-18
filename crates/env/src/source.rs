//! Where the variables come from, which is a deployment's decision and not a
//! `main`'s.
//!
//! The process environment is always read. Whether a local `.env` fills what it
//! leaves out is what changes between a checkout and a container, and
//! [`Source::from_env`] is where that is settled — from a variable, so the same
//! binary does both.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::Env;

/// Where a process's variables are read from. Owned here rather than by a
/// `main` that mutates the environment before anything reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The process environment alone, which is what a deployment injects. A
    /// `.env` sitting on disk is not consulted.
    Process,
    /// The process environment, with `.env` files filling in what it leaves out
    /// and never replacing it — so a file is a default, not an override.
    ProcessAndFile,
}

impl Source {
    /// What `APP_ENV` asks for. The one variable this crate will not read from
    /// a file, because it decides whether there is a file to read.
    ///
    /// Deliberately stricter than the TypeScript, which skips the file only
    /// under `NODE_ENV=test`: the guarantee moves out of `.dockerignore` and
    /// into the process.
    #[must_use]
    pub fn from_env() -> Self {
        Self::pick(std::env::var("APP_ENV").ok().as_deref())
    }

    /// Every variable this process can see, by whichever route it allows.
    ///
    /// `crate_dir` is the binary's `CARGO_MANIFEST_DIR`. Its own `.env` is read
    /// before the nearest one above it, so a variable only that binary reads
    /// needs no prefix to keep it apart from another binary's.
    #[must_use]
    pub fn read(self, crate_dir: &str) -> Env {
        let process = std::env::vars().collect();

        let vars = match self {
            Self::Process => process,
            Self::ProcessAndFile => fill(process, Path::new(crate_dir)),
        };

        vars.into_iter().collect()
    }

    /// Absent is local, as the TypeScript's `NODE_ENV` also has it.
    ///
    /// An unrecognised spelling reads the file too, which is the safe side: a
    /// deployment that fat-fingers it has no file to find.
    fn pick(value: Option<&str>) -> Self {
        match value {
            Some("production" | "test") => Self::Process,
            _ => Self::ProcessAndFile,
        }
    }
}

fn fill(vars: HashMap<String, String>, crate_dir: &Path) -> HashMap<String, String> {
    files(crate_dir).fold(vars, |vars, file| {
        // The usual error is that there is no file. A malformed one is silent
        // too, which is a local problem with a local symptom.
        match dotenvy::from_path_iter(file) {
            Ok(pairs) => merge(vars, pairs.flatten()),
            Err(_) => vars,
        }
    })
}

/// The binary's own file, then the shared one: the nearest `.env` above it.
fn files(crate_dir: &Path) -> impl Iterator<Item = PathBuf> {
    let shared = crate_dir
        .ancestors()
        .skip(1)
        .map(|dir| dir.join(".env"))
        .find(|file| file.is_file());

    std::iter::once(crate_dir.join(".env")).chain(shared)
}

/// The file fills gaps and nothing else.
fn merge(
    mut process: HashMap<String, String>,
    file: impl Iterator<Item = (String, String)>,
) -> HashMap<String, String> {
    for (key, value) in file {
        process.entry(key).or_insert(value);
    }
    process
}

#[cfg(test)]
mod tests {
    //! Which source a deployment asked for, and what merging leaves alone.
    use super::*;

    fn pairs(items: &[(&str, &str)]) -> HashMap<String, String> {
        items
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn a_deployments_variables_beat_the_file() {
        // Why `merge` inserts rather than extends: an injected variable is
        // never replaced by a file that reached the image.
        let merged = merge(
            pairs(&[("PORT", "3000")]),
            pairs(&[("PORT", "9999")]).into_iter(),
        );

        assert_eq!(merged["PORT"], "3000");
    }

    #[test]
    fn the_file_fills_only_what_the_process_left_out() {
        let merged = merge(
            pairs(&[("PORT", "3000")]),
            pairs(&[("PORT", "9999"), ("LOG_LEVEL", "debug")]).into_iter(),
        );

        assert_eq!(merged["PORT"], "3000");
        assert_eq!(merged["LOG_LEVEL"], "debug", "the gap was not filled");
    }

    /// A workspace in the temporary directory: a shared `.env` at its root, and
    /// one of the binary's own when `own` is given.
    fn workspace(name: &str, shared: &str, own: Option<&str>) -> PathBuf {
        let root = std::env::temp_dir().join(format!("env-{}-{name}", std::process::id()));
        let crate_dir = root.join("bins").join("bin");
        std::fs::create_dir_all(&crate_dir).unwrap();
        std::fs::write(root.join(".env"), shared).unwrap();
        if let Some(own) = own {
            std::fs::write(crate_dir.join(".env"), own).unwrap();
        }
        crate_dir
    }

    #[test]
    fn the_binarys_own_file_beats_the_shared_one() {
        let crate_dir = workspace("own", "PORT=9\nLOG_LEVEL=debug\n", Some("PORT=3101\n"));

        let filled = fill(HashMap::new(), &crate_dir);

        assert_eq!(filled["PORT"], "3101");
        assert_eq!(filled["LOG_LEVEL"], "debug", "the shared file was not read");
    }

    #[test]
    fn a_deployments_variables_beat_both_files() {
        let crate_dir = workspace("process", "PORT=9\n", Some("PORT=3101\n"));

        let filled = fill(pairs(&[("PORT", "3000")]), &crate_dir);

        assert_eq!(filled["PORT"], "3000");
    }

    #[test]
    fn a_binary_with_no_file_of_its_own_reads_the_shared_one() {
        let crate_dir = workspace("shared", "LOG_LEVEL=debug\n", None);

        assert_eq!(fill(HashMap::new(), &crate_dir)["LOG_LEVEL"], "debug");
    }

    #[test]
    fn a_deployment_reads_no_file() {
        assert_eq!(Source::pick(Some("production")), Source::Process);
        assert_eq!(Source::pick(Some("test")), Source::Process);
    }

    #[test]
    fn anything_else_reads_one() {
        // Including an unrecognised spelling, the safe side of the mistake.
        for value in [None, Some("development"), Some("prodution"), Some("")] {
            assert_eq!(Source::pick(value), Source::ProcessAndFile, "{value:?}");
        }
    }
}
