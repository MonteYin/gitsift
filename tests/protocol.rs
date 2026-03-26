mod common;

use common::{gitsift, setup_repo};
use std::fs;

/// Parse each line of stdout as a JSON value.
fn parse_response_lines(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|_| panic!("invalid JSON line: {l}")))
        .collect()
}

#[test]
fn protocol_diff_request() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let output = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin("{\"method\": \"diff\"}\n")
        .output()
        .unwrap();

    assert!(output.status.success());
    let responses = parse_response_lines(&String::from_utf8(output.stdout).unwrap());
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["ok"], true);
    assert_eq!(responses[0]["version"], 1);
    assert!(responses[0]["data"]["total_hunks"].as_u64().unwrap() > 0);
}

#[test]
fn protocol_diff_with_file_filter() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();
    fs::write(dir.path().join("other.txt"), "new\n").unwrap();

    let output = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin("{\"method\": \"diff\", \"params\": {\"file\": \"hello.txt\"}}\n")
        .output()
        .unwrap();

    let responses = parse_response_lines(&String::from_utf8(output.stdout).unwrap());
    assert_eq!(responses.len(), 1);
    let files = responses[0]["data"]["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], "hello.txt");
}

#[test]
fn protocol_status_request() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let output = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin("{\"method\": \"status\"}\n")
        .output()
        .unwrap();

    let responses = parse_response_lines(&String::from_utf8(output.stdout).unwrap());
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["ok"], true);
    assert_eq!(responses[0]["data"]["unstaged_files"], 1);
    assert_eq!(responses[0]["data"]["staged_files"], 0);
}

#[test]
fn protocol_invalid_json() {
    let dir = setup_repo();

    let output = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin("this is not json\n")
        .output()
        .unwrap();

    // Should NOT crash
    assert!(output.status.success());
    let responses = parse_response_lines(&String::from_utf8(output.stdout).unwrap());
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["ok"], false);
    assert!(responses[0]["error"].as_str().unwrap().contains("invalid request"));
}

#[test]
fn protocol_unknown_method() {
    let dir = setup_repo();

    let output = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin("{\"method\": \"unknown\"}\n")
        .output()
        .unwrap();

    assert!(output.status.success());
    let responses = parse_response_lines(&String::from_utf8(output.stdout).unwrap());
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["ok"], false);
}

#[test]
fn protocol_empty_lines_ignored() {
    let dir = setup_repo();

    let output = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin("\n\n{\"method\": \"status\"}\n\n")
        .output()
        .unwrap();

    assert!(output.status.success());
    let responses = parse_response_lines(&String::from_utf8(output.stdout).unwrap());
    // Only one response for the one real request
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["ok"], true);
}

#[test]
fn protocol_multiple_requests() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let stdin = [
        r#"{"method": "diff"}"#,
        r#"{"method": "status"}"#,
        r#"{"method": "diff", "params": {"file": "hello.txt"}}"#,
    ]
    .join("\n")
        + "\n";

    let output =
        gitsift().args(["protocol", "--repo"]).arg(dir.path()).write_stdin(stdin).output().unwrap();

    assert!(output.status.success());
    let responses = parse_response_lines(&String::from_utf8(output.stdout).unwrap());
    assert_eq!(responses.len(), 3);
    // All should be ok
    for resp in &responses {
        assert_eq!(resp["ok"], true);
    }
}

#[test]
fn protocol_full_workflow_diff_stage_status() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    // Step 1: diff to get hunk ID
    let diff_output = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin("{\"method\": \"diff\"}\n")
        .output()
        .unwrap();
    let diff_resp = parse_response_lines(&String::from_utf8(diff_output.stdout).unwrap());
    let hunk_id = diff_resp[0]["data"]["files"][0]["hunks"][0]["id"].as_str().unwrap();

    // Step 2: stage that hunk
    let stage_stdin =
        format!("{{\"method\": \"stage\", \"params\": {{\"hunk_ids\": [\"{hunk_id}\"]}}}}\n");
    let stage_output = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin(stage_stdin)
        .output()
        .unwrap();
    let stage_resp = parse_response_lines(&String::from_utf8(stage_output.stdout).unwrap());
    assert_eq!(stage_resp[0]["ok"], true);
    assert_eq!(stage_resp[0]["data"]["staged"], 1);

    // Step 3: status to verify
    let status_output = gitsift()
        .args(["protocol", "--repo"])
        .arg(dir.path())
        .write_stdin("{\"method\": \"status\"}\n")
        .output()
        .unwrap();
    let status_resp = parse_response_lines(&String::from_utf8(status_output.stdout).unwrap());
    assert_eq!(status_resp[0]["data"]["staged_hunks"], 1);
    assert_eq!(status_resp[0]["data"]["unstaged_hunks"], 0);
}

#[test]
fn protocol_each_response_is_one_line() {
    let dir = setup_repo();
    fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

    let stdin = "{\"method\": \"diff\"}\n{\"method\": \"status\"}\n";

    let output =
        gitsift().args(["protocol", "--repo"]).arg(dir.path()).write_stdin(stdin).output().unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let non_empty_lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(non_empty_lines.len(), 2);

    // Each line must be valid JSON on its own
    for line in &non_empty_lines {
        let val: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(val.is_object());
    }
}
