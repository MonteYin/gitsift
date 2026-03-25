use assert_cmd::Command;
use git2::{Repository, Signature};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Create a temp repo with an initial commit and return dir + path.
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

// ===== diff subcommand =====

#[test]
fn diff_json_output_valid() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "line 1\nchanged\nline 3\n").unwrap();

    let output = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["ok"], true);
    assert_eq!(val["version"], 1);
    assert!(val["data"]["files"].is_array());
    assert!(val["data"]["total_hunks"].as_u64().unwrap() > 0);
}

#[test]
fn diff_json_no_changes() {
    let dir = setup_repo();

    let output = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["data"]["total_hunks"], 0);
}

#[test]
fn diff_human_output_contains_hunk_ids() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let output = gitsift()
        .args(["diff", "--format", "human", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    // Human output shows hunk IDs in brackets and @@ headers
    assert!(stdout.contains("@@"));
    assert!(stdout.contains("["));
}

#[test]
fn diff_file_filter() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();
    fs::write(dir.path().join("other.txt"), "new file\n").unwrap();

    let output = gitsift()
        .args(["diff", "--format", "json", "--file", "hello.txt", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let files = val["data"]["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], "hello.txt");
}

#[test]
fn diff_invalid_repo_fails() {
    let output = gitsift()
        .args(["diff", "--format", "json", "--repo", "/nonexistent/path"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("failed to open git repository"));
}

// ===== stage subcommand =====

#[test]
fn stage_hunk_by_id() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    // Get hunk ID from diff
    let diff_output = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let diff_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(diff_output.stdout).unwrap()).unwrap();
    let hunk_id = diff_val["data"]["files"][0]["hunks"][0]["id"]
        .as_str()
        .unwrap();

    // Stage it
    let stage_output = gitsift()
        .args(["stage", "--hunk-ids", hunk_id, "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(stage_output.status.success());
    let stage_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(stage_output.stdout).unwrap()).unwrap();
    assert_eq!(stage_val["ok"], true);
    assert_eq!(stage_val["data"]["staged"], 1);
    assert_eq!(stage_val["data"]["failed"], 0);

    // Verify status shows staged
    let status_output = gitsift()
        .args(["status", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let status_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(status_output.stdout).unwrap()).unwrap();
    assert_eq!(status_val["data"]["staged_hunks"], 1);
    assert_eq!(status_val["data"]["unstaged_hunks"], 0);
}

#[test]
fn stage_invalid_id() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let output = gitsift()
        .args([
            "stage",
            "--hunk-ids",
            "bogus",
            "--format",
            "json",
            "--repo",
        ])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(output.stdout).unwrap()).unwrap();
    assert_eq!(val["data"]["staged"], 0);
    assert_eq!(val["data"]["failed"], 1);
}

// ===== status subcommand =====

#[test]
fn status_json_output() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let output = gitsift()
        .args(["status", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(output.stdout).unwrap()).unwrap();
    assert_eq!(val["ok"], true);
    assert_eq!(val["data"]["unstaged_files"], 1);
    assert_eq!(val["data"]["staged_files"], 0);
}

#[test]
fn status_human_output() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let output = gitsift()
        .args(["status", "--format", "human", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Staged:"));
    assert!(stdout.contains("Unstaged:"));
}

// ===== help =====

#[test]
fn help_shows_subcommands() {
    let output = gitsift().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("diff"));
    assert!(stdout.contains("stage"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("protocol"));
}

// ===== full workflow E2E =====

#[test]
fn full_workflow_diff_stage_status() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "line 1\nchanged\nline 3\nnew line\n").unwrap();

    // 1. Diff
    let diff_out = gitsift()
        .args(["diff", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(diff_out.status.success());
    let diff_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(diff_out.stdout).unwrap()).unwrap();
    let hunks = diff_val["data"]["files"][0]["hunks"].as_array().unwrap();
    assert!(!hunks.is_empty());
    let hunk_id = hunks[0]["id"].as_str().unwrap();

    // 2. Stage
    let stage_out = gitsift()
        .args(["stage", "--hunk-ids", hunk_id, "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(stage_out.status.success());
    let stage_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(stage_out.stdout).unwrap()).unwrap();
    assert_eq!(stage_val["data"]["staged"], 1);

    // 3. Status — should show staged
    let status_out = gitsift()
        .args(["status", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let status_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(status_out.stdout).unwrap()).unwrap();
    assert!(status_val["data"]["staged_hunks"].as_u64().unwrap() > 0);

    // 4. Diff again — should show no remaining unstaged hunks for this file
    let diff2_out = gitsift()
        .args(["diff", "--format", "json", "--file", "hello.txt", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let diff2_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(diff2_out.stdout).unwrap()).unwrap();
    assert_eq!(diff2_val["data"]["total_hunks"], 0);
}
