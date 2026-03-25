use anyhow::{Context, Result};
use git2::{ApplyLocation, ApplyOptions, Diff, DiffOptions, Repository};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::diff::hunk_id;
use crate::models::{HunkLine, LineSelection, LineTag, StageRequest, StageResult};

/// Collected hunk data from a diff, used for patch reconstruction.
struct RawHunk {
    file_path: String,
    old_start: u32,
    lines: Vec<HunkLine>,
}

/// Reconstruct a valid unified diff patch containing only selected lines from a hunk.
///
/// Rules:
/// - Context lines: keep as-is
/// - Selected `-` lines: keep as `-`
/// - Unselected `-` lines: convert to context (space prefix)
/// - Selected `+` lines: keep as `+`
/// - Unselected `+` lines: drop entirely
/// - Recalculate @@ header counts
fn reconstruct_patch(hunk: &RawHunk, selected_indices: &HashSet<usize>) -> String {
    let mut patch_lines: Vec<String> = Vec::new();
    let mut old_count: u32 = 0;
    let mut new_count: u32 = 0;

    for (i, line) in hunk.lines.iter().enumerate() {
        let selected = selected_indices.contains(&i);
        match line.tag {
            LineTag::Equal => {
                patch_lines.push(format!(" {}", line.content));
                old_count += 1;
                new_count += 1;
            }
            LineTag::Delete => {
                if selected {
                    patch_lines.push(format!("-{}", line.content));
                    old_count += 1;
                } else {
                    // Convert to context line
                    patch_lines.push(format!(" {}", line.content));
                    old_count += 1;
                    new_count += 1;
                }
            }
            LineTag::Insert => {
                if selected {
                    patch_lines.push(format!("+{}", line.content));
                    new_count += 1;
                }
                // Unselected inserts are simply dropped
            }
        }
    }

    // Build the full patch string
    let header = format!("@@ -{},{} +{},{} @@", hunk.old_start, old_count, hunk.old_start, new_count);

    let mut patch = String::new();
    patch.push_str(&format!("diff --git a/{} b/{}\n", hunk.file_path, hunk.file_path));
    patch.push_str(&format!("--- a/{}\n", hunk.file_path));
    patch.push_str(&format!("+++ b/{}\n", hunk.file_path));
    patch.push_str(&header);
    patch.push('\n');
    for line in &patch_lines {
        patch.push_str(line);
        // Ensure each line ends with newline
        if !line.ends_with('\n') {
            patch.push('\n');
        }
    }

    patch
}

/// Collect all hunk data from the unstaged diff, keyed by hunk ID.
fn collect_hunks(repo: &Repository) -> Result<HashMap<String, RawHunk>> {
    let mut opts = DiffOptions::new();
    opts.context_lines(3);
    opts.include_untracked(true);
    opts.show_untracked_content(true);

    let diff = repo
        .diff_index_to_workdir(None, Some(&mut opts))
        .context("failed to generate diff")?;

    let state = RefCell::new((
        HashMap::<String, RawHunk>::new(),
        String::new(),           // current file path
        Option::<String>::None,  // current hunk ID
    ));

    diff.foreach(
        &mut |delta, _| {
            let mut s = state.borrow_mut();
            s.1 = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            s.2 = None;
            true
        },
        None,
        Some(&mut |_delta, hunk| {
            let mut s = state.borrow_mut();
            let header = String::from_utf8_lossy(hunk.header()).trim().to_string();
            let id = hunk_id(&s.1, hunk.old_start(), &header);
            let raw = RawHunk {
                file_path: s.1.clone(),
                old_start: hunk.old_start(),
                lines: Vec::new(),
            };
            s.0.insert(id.clone(), raw);
            s.2 = Some(id);
            true
        }),
        Some(&mut |_delta, _hunk, line| {
            let mut s = state.borrow_mut();
            let tag = match line.origin() {
                '+' | '>' => LineTag::Insert,
                '-' | '<' => LineTag::Delete,
                _ => LineTag::Equal,
            };
            let content = String::from_utf8_lossy(line.content()).into_owned();
            let hunk_line = HunkLine {
                tag,
                content,
                old_lineno: line.old_lineno(),
                new_lineno: line.new_lineno(),
            };
            if let Some(id) = s.2.clone() {
                if let Some(raw) = s.0.get_mut(&id) {
                    raw.lines.push(hunk_line);
                }
            }
            true
        }),
    )
    .context("failed to iterate diff")?;

    Ok(state.into_inner().0)
}

/// Stage selected hunks and/or lines to the git index.
pub fn stage_selection(repo_path: &Path, request: &StageRequest) -> Result<StageResult> {
    if request.hunk_ids.is_empty() && request.line_selections.is_empty() {
        return Ok(StageResult {
            staged: 0,
            failed: 0,
            errors: vec!["no hunk IDs or line selections provided".into()],
        });
    }

    let repo = Repository::open(repo_path).context("failed to open git repository")?;

    let mut staged = 0usize;
    let mut failed = 0usize;
    let mut errors = Vec::new();

    // --- Hunk-level staging ---
    if !request.hunk_ids.is_empty() {
        let mut opts = DiffOptions::new();
        opts.context_lines(3);
        opts.include_untracked(true);
        opts.show_untracked_content(true);

        let diff = repo
            .diff_index_to_workdir(None, Some(&mut opts))
            .context("failed to generate diff")?;

        // Scan available IDs
        let available_ids: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
        let scan_path = RefCell::new(String::new());

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
        let unique_requested: HashSet<&str> = request.hunk_ids.iter().map(|s| s.as_str()).collect();

        let mut valid_ids: HashSet<String> = HashSet::new();
        for req_id in &unique_requested {
            if available_ids.contains(*req_id) {
                valid_ids.insert(req_id.to_string());
            } else {
                errors.push(format!("hunk ID not found: {req_id}"));
                failed += 1;
            }
        }

        if !valid_ids.is_empty() {
            let current_path = RefCell::new(String::new());

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
                valid_ids.contains(&id)
            });

            repo.apply(&diff, ApplyLocation::Index, Some(&mut apply_opts))
                .context("failed to apply selected hunks to index")?;

            staged += valid_ids.len();
        }
    }

    // --- Line-level staging ---
    if !request.line_selections.is_empty() {
        let hunks = collect_hunks(&repo)?;

        for sel in &request.line_selections {
            match hunks.get(&sel.hunk_id) {
                None => {
                    errors.push(format!("hunk ID not found: {}", sel.hunk_id));
                    failed += 1;
                }
                Some(raw) => {
                    let selected: HashSet<usize> = sel.line_indices.iter().copied().collect();

                    // Validate: at least one selected index is a change line (not context)
                    let has_change = selected.iter().any(|&i| {
                        raw.lines
                            .get(i)
                            .map(|l| l.tag != LineTag::Equal)
                            .unwrap_or(false)
                    });
                    if !has_change {
                        errors.push(format!(
                            "no change lines selected for hunk {}",
                            sel.hunk_id
                        ));
                        failed += 1;
                        continue;
                    }

                    let patch_str = reconstruct_patch(raw, &selected);
                    match Diff::from_buffer(patch_str.as_bytes()) {
                        Ok(patch_diff) => {
                            repo.apply(&patch_diff, ApplyLocation::Index, None)
                                .with_context(|| {
                                    format!("failed to apply line selection for hunk {}", sel.hunk_id)
                                })?;
                            staged += 1;
                        }
                        Err(e) => {
                            errors.push(format!(
                                "failed to parse reconstructed patch for hunk {}: {}",
                                sel.hunk_id, e
                            ));
                            failed += 1;
                        }
                    }
                }
            }
        }
    }

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

        let mut modified = lines;
        modified[1] = "line 2 CHANGED".to_string();
        modified[18] = "line 19 CHANGED".to_string();
        fs::write(dir.path().join("file.txt"), modified.join("\n") + "\n").unwrap();

        (dir, repo)
    }

    /// Create a repo with interleaved add/delete changes for line-level testing.
    fn setup_line_level_repo() -> (TempDir, Repository) {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();

        fs::write(
            dir.path().join("code.txt"),
            "alpha\nbeta\ngamma\ndelta\nepsilon\n",
        )
        .unwrap();

        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("code.txt")).unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
                .unwrap();
        }

        // Replace beta→BETA, add NEW after gamma, delete delta
        fs::write(
            dir.path().join("code.txt"),
            "alpha\nBETA\ngamma\nNEW\nepsilon\n",
        )
        .unwrap();

        (dir, repo)
    }

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

    /// Read file content from the index (staging area).
    /// Reopens the repo to get a fresh index (apply modifies via its own handle).
    fn read_index_content(repo_path: &Path, file_path: &str) -> String {
        let repo = Repository::open(repo_path).unwrap();
        let index = repo.index().unwrap();
        let entry = index.get_path(Path::new(file_path), 0).unwrap();
        let blob = repo.find_blob(entry.id).unwrap();
        String::from_utf8(blob.content().to_vec()).unwrap()
    }

    // ===== Hunk-level tests (preserved from GTST-4) =====

    #[test]
    fn stage_single_hunk() {
        let (dir, repo) = setup_two_hunk_repo();
        let output = diff_unstaged(dir.path(), None).unwrap();
        assert!(output.total_hunks >= 2);

        let first_id = output.files[0].hunks[0].id.clone();
        let request = StageRequest {
            hunk_ids: vec![first_id],
            line_selections: vec![],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 1);
        assert_eq!(result.failed, 0);
        assert_eq!(count_staged_hunks(&repo), 1);

        let remaining = diff_unstaged(dir.path(), None).unwrap();
        assert!(remaining.total_hunks >= 1);
    }

    #[test]
    fn stage_multiple_hunks() {
        let (dir, repo) = setup_two_hunk_repo();
        let output = diff_unstaged(dir.path(), None).unwrap();
        let all_ids: Vec<String> = output.files[0].hunks.iter().map(|h| h.id.clone()).collect();

        let request = StageRequest {
            hunk_ids: all_ids.clone(),
            line_selections: vec![],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, all_ids.len());
        assert_eq!(count_staged_hunks(&repo), all_ids.len());

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
        assert_eq!(count_staged_hunks(&repo), 1);
    }

    #[test]
    fn stage_duplicate_hunk_ids() {
        let (dir, repo) = setup_two_hunk_repo();
        let output = diff_unstaged(dir.path(), None).unwrap();
        let first_id = output.files[0].hunks[0].id.clone();

        let request = StageRequest {
            hunk_ids: vec![first_id.clone(), first_id],
            line_selections: vec![],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 1);
        assert_eq!(result.failed, 0);
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

        let request = StageRequest {
            hunk_ids: vec![first_id.clone()],
            line_selections: vec![],
        };
        stage_selection(dir.path(), &request).unwrap();

        let after = diff_unstaged(dir.path(), None).unwrap();
        assert_eq!(after.total_hunks, original_count - 1);

        let remaining_ids: Vec<&str> = after
            .files
            .iter()
            .flat_map(|f| f.hunks.iter().map(|h| h.id.as_str()))
            .collect();
        assert!(!remaining_ids.contains(&first_id.as_str()));
    }

    // ===== Line-level tests (GTST-5) =====

    #[test]
    fn line_stage_select_one_change() {
        let (dir, repo) = setup_line_level_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let hunk = &output.files[0].hunks[0];

        // Find the first insert line index
        let insert_idx = hunk
            .lines
            .iter()
            .position(|l| l.tag == LineTag::Insert)
            .unwrap();

        let request = StageRequest {
            hunk_ids: vec![],
            line_selections: vec![LineSelection {
                hunk_id: hunk.id.clone(),
                line_indices: vec![insert_idx],
            }],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 1);
        assert_eq!(result.failed, 0);

        // Should have staged something
        assert!(count_staged_hunks(&repo) > 0);

        // Remaining unstaged diff should still have changes
        let remaining = diff_unstaged(dir.path(), None).unwrap();
        assert!(remaining.total_hunks > 0);
    }

    #[test]
    fn line_stage_select_delete_and_insert() {
        let (dir, repo) = setup_line_level_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let hunk = &output.files[0].hunks[0];

        // Select one delete and one insert
        let delete_idx = hunk
            .lines
            .iter()
            .position(|l| l.tag == LineTag::Delete)
            .unwrap();
        let insert_idx = hunk
            .lines
            .iter()
            .position(|l| l.tag == LineTag::Insert)
            .unwrap();

        let request = StageRequest {
            hunk_ids: vec![],
            line_selections: vec![LineSelection {
                hunk_id: hunk.id.clone(),
                line_indices: vec![delete_idx, insert_idx],
            }],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 1);
        assert_eq!(result.failed, 0);

        // Index should have changes
        assert!(count_staged_hunks(&repo) > 0);
    }

    #[test]
    fn line_stage_all_changes_equals_full_hunk() {
        let (dir, _repo) = setup_line_level_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let hunk = &output.files[0].hunks[0];

        // Select ALL change lines (delete + insert)
        let change_indices: Vec<usize> = hunk
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.tag != LineTag::Equal)
            .map(|(i, _)| i)
            .collect();

        let request = StageRequest {
            hunk_ids: vec![],
            line_selections: vec![LineSelection {
                hunk_id: hunk.id.clone(),
                line_indices: change_indices,
            }],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 1);

        // Index content should match working tree content
        let index_content = read_index_content(dir.path(), "code.txt");
        let working_content = fs::read_to_string(dir.path().join("code.txt")).unwrap();
        assert_eq!(index_content, working_content);
    }

    #[test]
    fn line_stage_invalid_hunk_id() {
        let (dir, _repo) = setup_line_level_repo();

        let request = StageRequest {
            hunk_ids: vec![],
            line_selections: vec![LineSelection {
                hunk_id: "nonexistent".into(),
                line_indices: vec![0],
            }],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 0);
        assert_eq!(result.failed, 1);
        assert!(result.errors[0].contains("not found"));
    }

    #[test]
    fn line_stage_only_context_lines_fails() {
        let (dir, _repo) = setup_line_level_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let hunk = &output.files[0].hunks[0];

        // Select only context lines
        let context_indices: Vec<usize> = hunk
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.tag == LineTag::Equal)
            .map(|(i, _)| i)
            .collect();

        let request = StageRequest {
            hunk_ids: vec![],
            line_selections: vec![LineSelection {
                hunk_id: hunk.id.clone(),
                line_indices: context_indices,
            }],
        };
        let result = stage_selection(dir.path(), &request).unwrap();
        assert_eq!(result.staged, 0);
        assert_eq!(result.failed, 1);
        assert!(result.errors[0].contains("no change lines"));
    }

    #[test]
    fn line_stage_partial_then_remaining_visible() {
        let (dir, _repo) = setup_line_level_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let hunk = &output.files[0].hunks[0];
        let original_changes: Vec<usize> = hunk
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.tag != LineTag::Equal)
            .map(|(i, _)| i)
            .collect();

        // Stage only the first change line
        let request = StageRequest {
            hunk_ids: vec![],
            line_selections: vec![LineSelection {
                hunk_id: hunk.id.clone(),
                line_indices: vec![original_changes[0]],
            }],
        };
        stage_selection(dir.path(), &request).unwrap();

        // Remaining diff should still show unstaged changes
        let remaining = diff_unstaged(dir.path(), None).unwrap();
        assert!(remaining.total_hunks > 0, "should still have unstaged changes");
    }
}
