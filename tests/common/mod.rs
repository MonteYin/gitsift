#![allow(dead_code)]

use assert_cmd::Command;
use git2::{Repository, Signature};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Create a temp repo with an initial commit containing `hello.txt`.
pub fn setup_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let sig = Signature::now("test", "test@test.com").unwrap();

    fs::write(dir.path().join("hello.txt"), "line 1\nline 2\nline 3\n").unwrap();

    {
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("hello.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[]).unwrap();
    }

    dir
}

/// Build a `Command` for the gitsift binary.
pub fn gitsift() -> Command {
    Command::cargo_bin("gitsift").unwrap()
}

/// Parse stdout bytes as a single JSON value.
pub fn parse_json(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).unwrap()
}

/// Convert stdout bytes to a string for compact format assertions.
pub fn stdout_str(stdout: &[u8]) -> String {
    String::from_utf8(stdout.to_vec()).unwrap()
}
