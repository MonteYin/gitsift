use anyhow::{Context, Result};
use git2::{ApplyLocation, ApplyOptions, DiffOptions, Repository};
use std::collections::HashSet;
use std::path::Path;

use super::diff::hunk_id;
use crate::models::{StageRequest, StageResult};

/// Stage selected hunks to the git index.
///
/// For each hunk in the unstaged diff, computes its ID and checks if it's in
/// the requested set. Uses git2's `apply()` with `hunk_callback` to filter.
pub fn stage_selection(repo_path: &Path, request: &StageRequest) -> Result<StageResult> {
    if request.hunk_ids.is_empty() && request.line_selections.is_empty() {
        return Ok(StageResult {
            staged: 0,
            failed: 0,
            errors: vec!["no hunk IDs or line selections provided".into()],
        });
    }

    // Line-level staging is handled in a future task (GTST-5)
    if !request.line_selections.is_empty() {
        return Ok(StageResult {
            staged: 0,
            failed: 0,
            errors: vec!["line-level staging not yet implemented".into()],
        });
    }

    let repo = Repository::open(repo_path).context("failed to open git repository")?;

    // Get the current unstaged diff
    let mut opts = DiffOptions::new();
    opts.context_lines(3);
    opts.include_untracked(true);
    opts.show_untracked_content(true);

    let diff = repo
        .diff_index_to_workdir(None, Some(&mut opts))
        .context("failed to generate diff")?;

    // First pass: scan all hunk IDs in the diff to validate the request.
    // Uses RefCell to share file path state across foreach callbacks.
    let available_ids: std::cell::RefCell<HashSet<String>> =
        std::cell::RefCell::new(HashSet::new());
    let scan_path = std::cell::RefCell::new(String::new());

    diff.foreach(
        &mut |delta, _| {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            *scan_path.borrow_mut() = path;
            true
        },
        None,
        Some(&mut |_delta, hunk| {
            let path = scan_path.borrow();
            let header = String::from_utf8_lossy(hunk.header()).trim().to_string();
            let id = hunk_id(&path, hunk.old_start(), &header);
            available_ids.borrow_mut().insert(id);
            true
        }),
        None,
    )
    .context("failed to scan diff for hunk IDs")?;

    let available_ids = available_ids.into_inner();

    // Deduplicate requested IDs at input boundary to avoid phantom failures.
    let unique_requested: HashSet<&str> = request.hunk_ids.iter().map(|s| s.as_str()).collect();

    let mut errors = Vec::new();
    let mut valid_ids: HashSet<String> = HashSet::new();

    for req_id in &unique_requested {
        if available_ids.contains(*req_id) {
            valid_ids.insert(req_id.to_string());
        } else {
            errors.push(format!("hunk ID not found: {req_id}"));
        }
    }

    if valid_ids.is_empty() {
        return Ok(StageResult {
            staged: 0,
            failed: unique_requested.len(),
            errors,
        });
    }

    // Second pass: apply the diff with hunk filtering.
    // The hunk callback needs to compute the hunk ID to decide whether to include it.
    // We track the current file path via the delta callback.
    let current_path = std::cell::RefCell::new(String::new());
    let staged_count = std::cell::Cell::new(0usize);

    let mut apply_opts = ApplyOptions::new();
    apply_opts.delta_callback(|delta| {
        let path = delta
            .and_then(|d| d.new_file().path().or_else(|| d.old_file().path()))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        *current_path.borrow_mut() = path;
        true
    });
    apply_opts.hunk_callback(|hunk| {
        let hunk = match hunk {
            Some(h) => h,
            None => return false,
        };
        let path = current_path.borrow();
        let header = String::from_utf8_lossy(hunk.header()).trim().to_string();
        let id = hunk_id(&path, hunk.old_start(), &header);
        let selected = valid_ids.contains(&id);
        if selected {
            staged_count.set(staged_count.get() + 1);
        }
        selected
    });

    repo.apply(&diff, ApplyLocation::Index, Some(&mut apply_opts))
        .context("failed to apply selected hunks to index")?;

    let staged = staged_count.get();
    let failed = unique_requested.len() - staged;

    Ok(StageResult {
        staged,
        failed,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff::diff_unstaged;
    use git2::Signature;
    use std::fs;
    use tempfile::TempDir;

    /// Create a temp repo with a big file that produces 2 hunks when modified.
    fn setup_two_hunk_repo() -> (TempDir, Repository) {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();

        // 20 lines — changes at line 2 and line 19 produce separate hunks with 3 context lines
        let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        fs::write(dir.path().join("file.txt"), lines.join("\n") + "\n").unwrap();

        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("file.txt")).unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
                .unwrap();
        }

        // Modify lines far apart
        let mut modified = lines;
        modified[1] = "line 2 CHANGED".to_string();
        modified[18] = "line 19 CHANGED".to_string();
        fs::write(dir.path().join("file.txt"), modified.join("\n") + "\n").unwrap();

        (dir, repo)
    }

    /// Count hunks in staged diff (tree-to-index).
    fn count_staged_hunks(repo: &Repository) -> usize {
        let head = repo.head().unwrap().peel_to_tree().unwrap();
        let diff = repo
            .diff_tree_to_index(Some(&head), None, None)
            .unwrap();
        let mut count = 0;
        diff.foreach(
            &mut |_, _| true,
            None,
            Some(&mut |_, _| {
                count += 1;
                true
            }),
            None,
        )
        .unwrap();
        count
    }

    #[test]
    fn stage_single_hunk() {
        let (dir, repo) = setup_two_hunk_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        assert!(output.total_hunks >= 2, "need at least 2 hunks");

        let first_id = output.files[0].hunks[0].id.clone();

        // Stage only the first hunk
        let request = StageRequest {
            hunk_ids: vec![first_id],
            line_selections: vec![],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 1);
        assert_eq!(result.failed, 0);
        assert!(result.errors.is_empty());

        // Verify: 1 hunk staged
        assert_eq!(count_staged_hunks(&repo), 1);

        // Verify: remaining hunk still unstaged
        let remaining = diff_unstaged(dir.path(), None).unwrap();
        assert!(remaining.total_hunks >= 1);
    }

    #[test]
    fn stage_multiple_hunks() {
        let (dir, repo) = setup_two_hunk_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let all_ids: Vec<String> = output.files[0]
            .hunks
            .iter()
            .map(|h| h.id.clone())
            .collect();

        let request = StageRequest {
            hunk_ids: all_ids.clone(),
            line_selections: vec![],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, all_ids.len());
        assert_eq!(result.failed, 0);

        // All hunks staged
        assert_eq!(count_staged_hunks(&repo), all_ids.len());

        // No remaining unstaged hunks for this file
        let remaining = diff_unstaged(dir.path(), Some("file.txt")).unwrap();
        assert_eq!(remaining.total_hunks, 0);
    }

    #[test]
    fn stage_invalid_hunk_id() {
        let (dir, repo) = setup_two_hunk_repo();

        let request = StageRequest {
            hunk_ids: vec!["nonexistent_id".into()],
            line_selections: vec![],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 0);
        assert_eq!(result.failed, 1);
        assert!(result.errors[0].contains("not found"));

        // Index unchanged
        assert_eq!(count_staged_hunks(&repo), 0);
    }

    #[test]
    fn stage_mix_valid_and_invalid() {
        let (dir, repo) = setup_two_hunk_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let valid_id = output.files[0].hunks[0].id.clone();

        let request = StageRequest {
            hunk_ids: vec![valid_id, "bad_id".into()],
            line_selections: vec![],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 1);
        assert_eq!(result.failed, 1);
        assert!(result.errors.iter().any(|e| e.contains("bad_id")));

        assert_eq!(count_staged_hunks(&repo), 1);
    }

    #[test]
    fn stage_duplicate_hunk_ids() {
        let (dir, repo) = setup_two_hunk_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let first_id = output.files[0].hunks[0].id.clone();

        // Pass the same ID twice — should deduplicate, not produce phantom failure
        let request = StageRequest {
            hunk_ids: vec![first_id.clone(), first_id],
            line_selections: vec![],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 1);
        assert_eq!(result.failed, 0);
        assert!(result.errors.is_empty());

        assert_eq!(count_staged_hunks(&repo), 1);
    }

    #[test]
    fn stage_empty_request() {
        let (dir, _repo) = setup_two_hunk_repo();

        let request = StageRequest {
            hunk_ids: vec![],
            line_selections: vec![],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 0);
        assert!(result.errors[0].contains("no hunk IDs"));
    }

    #[test]
    fn stage_then_diff_shows_remaining() {
        let (dir, _repo) = setup_two_hunk_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let first_id = output.files[0].hunks[0].id.clone();
        let original_count = output.total_hunks;

        // Stage first hunk
        let request = StageRequest {
            hunk_ids: vec![first_id.clone()],
            line_selections: vec![],
        };
        stage_selection(dir.path(), &request).unwrap();

        // Diff again — should have one fewer hunk
        let after = diff_unstaged(dir.path(), None).unwrap();
        assert_eq!(after.total_hunks, original_count - 1);

        // The staged hunk ID should no longer appear
        let remaining_ids: Vec<&str> = after
            .files
            .iter()
            .flat_map(|f| f.hunks.iter().map(|h| h.id.as_str()))
            .collect();
        assert!(!remaining_ids.contains(&first_id.as_str()));
    }
}
