//! Where the variables come from, which is a deployment's decision and not a
//! `main`'s.
//!
//! The process environment is always read. Whether a local `.env` fills what it
//! leaves out is what changes between a checkout and a container, and
//! [`Source::from_env`] is where that is settled — from a variable, so the same
//! binary does both.

use std::collections::HashMap;
use std::path::Path;

use crate::Env;

/// Where a process's variables are read from. Owned here rather than by a
/// `main` that mutates the environment before anything reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The process environment alone, which is what a deployment injects. A
    /// `.env` sitting on disk is not consulted.
    Process,
    /// The process environment, with the binary's `.env` filling in what it
    /// leaves out and never replacing it — so the file is a default, not an
    /// override.
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
    /// `crate_dir` is the binary's `CARGO_MANIFEST_DIR`, and the file is the
    /// `.env` in it: found from the build rather than the working directory.
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
    // The usual error is that there is no file. A malformed one is silent too,
    // which is a local problem with a local symptom.
    match dotenvy::from_path_iter(crate_dir.join(".env")) {
        Ok(pairs) => merge(vars, pairs.flatten()),
        Err(_) => vars,
    }
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

    /// A workspace in the temporary directory, with a `.env` at its root and
    /// `own` beside the binary's manifest.
    fn workspace(name: &str, root: &str, own: &str) -> std::path::PathBuf {
        let workspace = std::env::temp_dir().join(format!("env-{}-{name}", std::process::id()));
        let crate_dir = workspace.join("bins").join("bin");
        std::fs::create_dir_all(&crate_dir).unwrap();
        std::fs::write(workspace.join(".env"), root).unwrap();
        std::fs::write(crate_dir.join(".env"), own).unwrap();
        crate_dir
    }

    #[test]
    fn reads_the_file_beside_the_binarys_manifest() {
        let crate_dir = workspace("own", "", "PORT=3101\n");

        assert_eq!(fill(HashMap::new(), &crate_dir)["PORT"], "3101");
    }

    #[test]
    fn a_deployments_variables_beat_the_binarys_file() {
        let crate_dir = workspace("process", "", "PORT=3101\n");

        let filled = fill(pairs(&[("PORT", "3000")]), &crate_dir);

        assert_eq!(filled["PORT"], "3000");
    }

    #[test]
    fn a_file_above_the_binary_is_not_read() {
        let crate_dir = workspace("above", "RPC_URLS=https://stale.example\n", "PORT=3101\n");

        assert!(!fill(HashMap::new(), &crate_dir).contains_key("RPC_URLS"));
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
