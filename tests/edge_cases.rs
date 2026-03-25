//! Edge case and cross-cutting integration tests for gitsift.
//! Covers scenarios not tested in individual module tests.

use assert_cmd::Command;
use git2::{Repository, Signature};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn setup_repo() -> TempDir {
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
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .unwrap();
    }

    dir
}

fn gitsift() -> Command {
    Command::cargo_bin("gitsift").unwrap()
}

fn parse_json(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_str(&String::from_utf8(stdout.to_vec()).unwrap()).unwrap()
}

// ===== Binary files =====

#[test]
fn binary_file_skipped_in_diff() {
    let dir = setup_repo();

    // Create a binary file (contains null bytes)
    fs::write(dir.path().join("image.png"), b"\x89PNG\r\n\x1a\n\x00\x00").unwrap();

    let output = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let val = parse_json(&output.stdout);
    let files = val["data"]["files"].as_array().unwrap();

    // Binary file should NOT appear in the diff output
    let binary = files.iter().find(|f| f["path"] == "image.png");
    assert!(binary.is_none(), "binary files should be skipped");
}

#[test]
fn binary_file_does_not_crash_stage() {
    let dir = setup_repo();

    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();
    fs::write(dir.path().join("data.bin"), b"\x00\x01\x02\x03").unwrap();

    // Get diff filtered to hello.txt
    let diff_out = gitsift()
        .args(["diff", "--format", "json", "--file", "hello.txt", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let diff_val = parse_json(&diff_out.stdout);
    let files = diff_val["data"]["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    let hunk_id = files[0]["hunks"][0]["id"].as_str().unwrap();

    // Stage the text file hunk — should work fine despite binary file existing
    let stage_out = gitsift()
        .args(["stage", "--hunk-ids", hunk_id, "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(stage_out.status.success());
    let stage_val = parse_json(&stage_out.stdout);
    assert_eq!(stage_val["data"]["staged"], 1);
}

// ===== Deleted files =====

#[test]
fn deleted_file_diff_and_status() {
    let dir = setup_repo();

    fs::remove_file(dir.path().join("hello.txt")).unwrap();

    // Diff should show deleted file
    let diff_out = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let diff_val = parse_json(&diff_out.stdout);
    let file = &diff_val["data"]["files"][0];
    assert_eq!(file["status"], "deleted");
    assert_eq!(file["path"], "hello.txt");

    // Status should show 1 unstaged file
    let status_out = gitsift()
        .args(["status", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let status_val = parse_json(&status_out.stdout);
    assert_eq!(status_val["data"]["unstaged_files"], 1);
}

// ===== New (untracked) files =====

#[test]
fn new_file_diff_shows_added() {
    let dir = setup_repo();

    fs::write(dir.path().join("new_feature.rs"), "fn main() {}\n").unwrap();

    let output = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let val = parse_json(&output.stdout);
    let new_file = val["data"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "new_feature.rs")
        .unwrap();
    assert_eq!(new_file["status"], "added");
    assert!(
        !new_file["hunks"].as_array().unwrap().is_empty(),
        "added file should have hunks with content"
    );
}

// ===== --from-stdin for stage =====

#[test]
fn stage_from_stdin_json() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    // Get hunk ID
    let diff_out = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let diff_val = parse_json(&diff_out.stdout);
    let hunk_id = diff_val["data"]["files"][0]["hunks"][0]["id"]
        .as_str()
        .unwrap();

    // Stage via --from-stdin with JSON payload
    let stdin_json = format!(r#"{{"hunk_ids": ["{hunk_id}"]}}"#);
    let stage_out = gitsift()
        .args(["stage", "--from-stdin", "--format", "json", "--repo"])
        .arg(dir.path())
        .write_stdin(stdin_json)
        .output()
        .unwrap();

    assert!(stage_out.status.success());
    let stage_val = parse_json(&stage_out.stdout);
    assert_eq!(stage_val["data"]["staged"], 1);
}

// ===== Multi-file workflow =====

#[test]
fn multi_file_selective_staging() {
    let dir = setup_repo();
    let repo = Repository::open(dir.path()).unwrap();
    let sig = Signature::now("test", "test@test.com").unwrap();

    // Add a second file
    fs::write(dir.path().join("second.txt"), "original\n").unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("second.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add second", &tree, &[&head])
            .unwrap();
    }

    // Modify both files
    fs::write(dir.path().join("hello.txt"), "hello changed\n").unwrap();
    fs::write(dir.path().join("second.txt"), "second changed\n").unwrap();

    // Get diff — should have 2 files
    let diff_out = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let diff_val = parse_json(&diff_out.stdout);
    let files = diff_val["data"]["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);

    // Stage only hello.txt's hunk
    let hello_file = files.iter().find(|f| f["path"] == "hello.txt").unwrap();
    let hunk_id = hello_file["hunks"][0]["id"].as_str().unwrap();

    let stage_out = gitsift()
        .args(["stage", "--hunk-ids", hunk_id, "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let stage_val = parse_json(&stage_out.stdout);
    assert_eq!(stage_val["data"]["staged"], 1);

    // Status: 1 staged (hello.txt), 1 unstaged (second.txt)
    let status_out = gitsift()
        .args(["status", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let status_val = parse_json(&status_out.stdout);
    assert_eq!(status_val["data"]["staged_files"], 1);
    assert_eq!(status_val["data"]["unstaged_files"], 1);

    // Diff should only show second.txt remaining
    let diff2_out = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let diff2_val = parse_json(&diff2_out.stdout);
    let remaining = diff2_val["data"]["files"].as_array().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["path"], "second.txt");
}

// ===== Protocol multi-request session =====

#[test]
fn protocol_session_diff_stage_status_diff() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    // Step 1: diff to get hunk ID
    let diff_out = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin("{\"method\": \"diff\"}\n")
        .output()
        .unwrap();
    let diff_lines: Vec<serde_json::Value> = String::from_utf8(diff_out.stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let hunk_id = diff_lines[0]["data"]["files"][0]["hunks"][0]["id"]
        .as_str()
        .unwrap();

    // Step 2: stage via protocol
    let stage_stdin = format!(
        "{{\"method\": \"stage\", \"params\": {{\"hunk_ids\": [\"{hunk_id}\"]}}}}\n"
    );
    let stage_out = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin(stage_stdin)
        .output()
        .unwrap();
    let stage_lines: Vec<serde_json::Value> = String::from_utf8(stage_out.stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(stage_lines[0]["data"]["staged"], 1);

    // Step 3: status via protocol
    let status_out = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin("{\"method\": \"status\"}\n")
        .output()
        .unwrap();
    let status_lines: Vec<serde_json::Value> = String::from_utf8(status_out.stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(status_lines[0]["data"]["staged_hunks"], 1);
    assert_eq!(status_lines[0]["data"]["unstaged_hunks"], 0);

    // Step 4: diff again — should be empty
    let diff2_out = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin("{\"method\": \"diff\"}\n")
        .output()
        .unwrap();
    let diff2_lines: Vec<serde_json::Value> = String::from_utf8(diff2_out.stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(diff2_lines[0]["data"]["total_hunks"], 0);
}

// ===== Empty repo =====

#[test]
fn empty_repo_all_commands_work() {
    let dir = TempDir::new().unwrap();
    let _repo = Repository::init(dir.path()).unwrap();

    // diff
    let diff_out = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(diff_out.status.success());
    let diff_val = parse_json(&diff_out.stdout);
    assert_eq!(diff_val["data"]["total_hunks"], 0);

    // status
    let status_out = gitsift()
        .args(["status", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(status_out.status.success());
    let status_val = parse_json(&status_out.stdout);
    assert_eq!(status_val["data"]["staged_files"], 0);
    assert_eq!(status_val["data"]["unstaged_files"], 0);
}

// ===== No changes =====

#[test]
fn clean_repo_diff_returns_empty() {
    let dir = setup_repo();

    let output = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let val = parse_json(&output.stdout);
    assert_eq!(val["data"]["total_hunks"], 0);
    assert_eq!(val["data"]["files"].as_array().unwrap().len(), 0);
}
