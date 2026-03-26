mod common;

use common::{gitsift, parse_json, setup_repo, stdout_str};
use std::fs;

// ===== diff subcommand =====

#[test]
fn diff_json_output_valid() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "line 1\nchanged\nline 3\n").unwrap();

    let output =
        gitsift().args(["diff", "--format", "json", "--repo"]).arg(dir.path()).output().unwrap();

    assert!(output.status.success());

    let val = parse_json(&output.stdout);
    assert_eq!(val["ok"], true);
    assert_eq!(val["version"], 1);
    assert!(val["data"]["files"].is_array());
    assert!(val["data"]["total_hunks"].as_u64().unwrap() > 0);
}

#[test]
fn diff_json_no_changes() {
    let dir = setup_repo();

    let output =
        gitsift().args(["diff", "--format", "json", "--repo"]).arg(dir.path()).output().unwrap();

    assert!(output.status.success());
    let val = parse_json(&output.stdout);
    assert_eq!(val["data"]["total_hunks"], 0);
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
    let val = parse_json(&output.stdout);
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
    let diff_output =
        gitsift().args(["diff", "--format", "json", "--repo"]).arg(dir.path()).output().unwrap();
    let diff_val = parse_json(&diff_output.stdout);
    let hunk_id = diff_val["data"]["files"][0]["hunks"][0]["id"].as_str().unwrap();

    // Stage it
    let stage_output = gitsift()
        .args(["stage", "--hunk-ids", hunk_id, "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(stage_output.status.success());
    let stage_val = parse_json(&stage_output.stdout);
    assert_eq!(stage_val["ok"], true);
    assert_eq!(stage_val["data"]["staged"], 1);
    assert_eq!(stage_val["data"]["failed"], 0);

    // Verify status shows staged
    let status_output =
        gitsift().args(["status", "--format", "json", "--repo"]).arg(dir.path()).output().unwrap();
    let status_val = parse_json(&status_output.stdout);
    assert_eq!(status_val["data"]["staged_hunks"], 1);
    assert_eq!(status_val["data"]["unstaged_hunks"], 0);
}

#[test]
fn stage_invalid_id() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let output = gitsift()
        .args(["stage", "--hunk-ids", "bogus", "--format", "json", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let val = parse_json(&output.stdout);
    assert_eq!(val["data"]["staged"], 0);
    assert_eq!(val["data"]["failed"], 1);
}

// ===== status subcommand =====

#[test]
fn status_json_output() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let output =
        gitsift().args(["status", "--format", "json", "--repo"]).arg(dir.path()).output().unwrap();

    assert!(output.status.success());
    let val = parse_json(&output.stdout);
    assert_eq!(val["ok"], true);
    assert_eq!(val["data"]["unstaged_files"], 1);
    assert_eq!(val["data"]["staged_files"], 0);
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
    let diff_out =
        gitsift().args(["diff", "--format", "json", "--repo"]).arg(dir.path()).output().unwrap();
    assert!(diff_out.status.success());
    let diff_val = parse_json(&diff_out.stdout);
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
    let stage_val = parse_json(&stage_out.stdout);
    assert_eq!(stage_val["data"]["staged"], 1);

    // 3. Status — should show staged
    let status_out =
        gitsift().args(["status", "--format", "json", "--repo"]).arg(dir.path()).output().unwrap();
    let status_val = parse_json(&status_out.stdout);
    assert!(status_val["data"]["staged_hunks"].as_u64().unwrap() > 0);

    // 4. Diff again — should show no remaining unstaged hunks for this file
    let diff2_out = gitsift()
        .args(["diff", "--format", "json", "--file", "hello.txt", "--repo"])
        .arg(dir.path())
        .output()
        .unwrap();
    let diff2_val = parse_json(&diff2_out.stdout);
    assert_eq!(diff2_val["data"]["total_hunks"], 0);
}

// ===== Compact (TOON-like) format =====

#[test]
fn diff_toon_has_expected_structure() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "line 1\nchanged\nline 3\n").unwrap();

    let output = gitsift().args(["diff", "--repo"]).arg(dir.path()).output().unwrap();

    assert!(output.status.success());
    let out = stdout_str(&output.stdout);
    assert!(out.contains("version: 1"));
    assert!(out.contains("ok: true"));
    assert!(out.contains("total_hunks:"));
    assert!(out.contains("files["));
    assert!(out.contains("path: hello.txt"));
}

#[test]
fn diff_toon_no_context_lines() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "line 1\nchanged\nline 3\n").unwrap();

    let output = gitsift().args(["diff", "--repo"]).arg(dir.path()).output().unwrap();

    let out = stdout_str(&output.stdout);
    assert!(!out.contains("line 1\\n"), "context line 'line 1' should be stripped");
    assert!(!out.contains("line 3\\n"), "context line 'line 3' should be stripped");
}

#[test]
fn diff_toon_tabular_header() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "line 1\nchanged\nline 3\n").unwrap();

    let output = gitsift().args(["diff", "--repo"]).arg(dir.path()).output().unwrap();

    let out = stdout_str(&output.stdout);
    assert!(out.contains("{tag,content,old,new}:"), "should have tabular header for lines");
}

#[test]
fn diff_toon_hunk_ids_match_json() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let json_out =
        gitsift().args(["diff", "--format", "json", "--repo"]).arg(dir.path()).output().unwrap();
    let toon_out = gitsift().args(["diff", "--repo"]).arg(dir.path()).output().unwrap();

    let json_val = parse_json(&json_out.stdout);
    let json_id = json_val["data"]["files"][0]["hunks"][0]["id"].as_str().unwrap();

    let toon_str = stdout_str(&toon_out.stdout);
    assert!(toon_str.contains(&format!("id: {json_id}")));
}

#[test]
fn status_toon_output() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let output = gitsift().args(["status", "--repo"]).arg(dir.path()).output().unwrap();

    assert!(output.status.success());
    let out = stdout_str(&output.stdout);
    assert!(out.contains("ok: true"));
    assert!(out.contains("unstaged_files: 1"));
    assert!(out.contains("staged_files: 0"));
}

#[test]
fn default_format_is_toon() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let output = gitsift().args(["diff", "--repo"]).arg(dir.path()).output().unwrap();

    assert!(output.status.success());
    let out = stdout_str(&output.stdout);
    assert!(!out.starts_with('{'), "default should be toon, not JSON");
    assert!(out.contains("version: 1"));
}

#[test]
fn json_format_preserves_context_lines() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "line 1\nchanged\nline 3\n").unwrap();

    let output =
        gitsift().args(["diff", "--format", "json", "--repo"]).arg(dir.path()).output().unwrap();

    assert!(output.status.success());
    let val = parse_json(&output.stdout);
    let lines = val["data"]["files"][0]["hunks"][0]["lines"].as_array().unwrap();
    let has_equal = lines.iter().any(|l| l["tag"] == "equal");
    assert!(has_equal, "JSON format should preserve context lines");
}
