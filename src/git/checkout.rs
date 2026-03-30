use anyhow::{Context, Result};
use git2::{ApplyLocation, Diff, Repository};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::diff::hunk_id;
use super::{delta_path, hunk_header, is_binary_delta};
use crate::models::{CheckoutRequest, CheckoutResult, HunkLine, LineTag};

/// Collected hunk data from a diff, used for reverse-patch reconstruction.
struct RawHunk {
    file_path: String,
    old_start: u32,
    new_start: u32,
    lines: Vec<HunkLine>,
}

/// Reconstruct a reverse unified diff patch for a single hunk.
///
/// Swaps the direction: `-` lines become `+` lines (restore old content),
/// `+` lines become `-` lines (remove new content), context stays as-is.
/// The @@ header swaps old/new positions accordingly.
fn reconstruct_reverse_patch(hunk: &RawHunk) -> String {
    use std::fmt::Write;

    let mut patch_lines: Vec<String> = Vec::new();
    let mut old_count: u32 = 0;
    let mut new_count: u32 = 0;

    for line in &hunk.lines {
        match line.tag {
            LineTag::Equal => {
                patch_lines.push(format!(" {}", line.content));
                old_count += 1;
                new_count += 1;
            }
            LineTag::Delete => {
                // Original `-` becomes `+` in the reverse (restore deleted content)
                patch_lines.push(format!("+{}", line.content));
                new_count += 1;
            }
            LineTag::Insert => {
                // Original `+` becomes `-` in the reverse (remove inserted content)
                patch_lines.push(format!("-{}", line.content));
                old_count += 1;
            }
        }
    }

    // In the reverse patch: old side = new_start from original, new side = old_start from original
    let header =
        format!("@@ -{},{} +{},{} @@", hunk.new_start, old_count, hunk.old_start, new_count);

    let mut patch = String::new();
    let _ = writeln!(patch, "diff --git a/{0} b/{0}", hunk.file_path);
    let _ = writeln!(patch, "--- a/{}", hunk.file_path);
    let _ = writeln!(patch, "+++ b/{}", hunk.file_path);
    patch.push_str(&header);
    patch.push('\n');
    for line in &patch_lines {
        patch.push_str(line);
        if !line.ends_with('\n') {
            patch.push('\n');
        }
    }

    patch
}

/// Metadata about a hunk: file path and whether the file is untracked/newly-added.
struct HunkMeta {
    file_path: String,
    is_untracked: bool,
    is_added: bool,
}

/// Scan the unstaged diff and return hunk metadata.
fn scan_unstaged_hunk_metadata(repo: &Repository) -> Result<HashMap<String, HunkMeta>> {
    let mut opts = super::diff_opts_with_untracked();

    let diff =
        repo.diff_index_to_workdir(None, Some(&mut opts)).context("failed to generate diff")?;

    let metadata: RefCell<HashMap<String, HunkMeta>> = RefCell::new(HashMap::new());
    let current: RefCell<(String, bool, bool)> = RefCell::new((String::new(), false, false));

    diff.foreach(
        &mut |delta, _| {
            if is_binary_delta(&delta) {
                *current.borrow_mut() = (String::new(), false, false);
                return true;
            }
            let path = delta_path(&delta);
            let untracked = delta.status() == git2::Delta::Untracked;
            let added = delta.status() == git2::Delta::Added;
            *current.borrow_mut() = (path, untracked, added);
            true
        },
        None,
        Some(&mut |_delta, hunk| {
            let cur = current.borrow();
            let header = hunk_header(&hunk);
            let id = hunk_id(&cur.0, hunk.old_start(), &header);
            metadata.borrow_mut().insert(
                id,
                HunkMeta { file_path: cur.0.clone(), is_untracked: cur.1, is_added: cur.2 },
            );
            true
        }),
        None,
    )
    .context("failed to scan unstaged hunk metadata")?;

    Ok(metadata.into_inner())
}

/// Scan the staged diff (HEAD vs index) and return hunk metadata.
fn scan_staged_hunk_metadata(repo: &Repository) -> Result<HashMap<String, HunkMeta>> {
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());

    let mut opts = super::diff_opts_tracked_only();

    let diff = repo
        .diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))
        .context("failed to generate staged diff")?;

    let metadata: RefCell<HashMap<String, HunkMeta>> = RefCell::new(HashMap::new());
    let current: RefCell<(String, bool)> = RefCell::new((String::new(), false));

    diff.foreach(
        &mut |delta, _| {
            if is_binary_delta(&delta) {
                *current.borrow_mut() = (String::new(), false);
                return true;
            }
            let path = delta_path(&delta);
            let is_added = delta.status() == git2::Delta::Added;
            *current.borrow_mut() = (path, is_added);
            true
        },
        None,
        Some(&mut |_delta, hunk| {
            let cur = current.borrow();
            let header = hunk_header(&hunk);
            let id = hunk_id(&cur.0, hunk.old_start(), &header);
            metadata.borrow_mut().insert(
                id,
                HunkMeta { file_path: cur.0.clone(), is_untracked: false, is_added: cur.1 },
            );
            true
        }),
        None,
    )
    .context("failed to scan staged hunk metadata")?;

    Ok(metadata.into_inner())
}

/// Collect full hunk data from a diff, keyed by hunk ID.
fn collect_hunks_from_diff(diff: &git2::Diff<'_>) -> Result<HashMap<String, RawHunk>> {
    let state = RefCell::new(CollectState {
        hunks: HashMap::new(),
        current_path: String::new(),
        current_hunk_id: None,
    });

    diff.foreach(
        &mut |delta, _| {
            let mut s = state.borrow_mut();
            if is_binary_delta(&delta) {
                s.current_path.clear();
                s.current_hunk_id = None;
            } else {
                s.current_path = delta_path(&delta);
                s.current_hunk_id = None;
            }
            true
        },
        None,
        Some(&mut |_delta, hunk| {
            let mut s = state.borrow_mut();
            let header = hunk_header(&hunk);
            let id = hunk_id(&s.current_path, hunk.old_start(), &header);
            let raw = RawHunk {
                file_path: s.current_path.clone(),
                old_start: hunk.old_start(),
                new_start: hunk.new_start(),
                lines: Vec::new(),
            };
            s.hunks.insert(id.clone(), raw);
            s.current_hunk_id = Some(id);
            true
        }),
        Some(&mut |_delta, _hunk, line| {
            let mut s = state.borrow_mut();
            let tag = LineTag::from_origin(line.origin());
            let content = String::from_utf8_lossy(line.content()).into_owned();
            let hunk_line = HunkLine {
                tag,
                content,
                old_lineno: line.old_lineno(),
                new_lineno: line.new_lineno(),
            };
            if let Some(id) = s.current_hunk_id.clone()
                && let Some(raw) = s.hunks.get_mut(&id)
            {
                raw.lines.push(hunk_line);
            }
            true
        }),
    )
    .context("failed to iterate diff")?;

    Ok(state.into_inner().hunks)
}

struct CollectState {
    hunks: HashMap<String, RawHunk>,
    current_path: String,
    current_hunk_id: Option<String>,
}

/// Discard selected unstaged hunks (revert working tree → index state).
pub fn checkout_unstaged(repo_path: &Path, request: &CheckoutRequest) -> Result<CheckoutResult> {
    if request.hunk_ids.is_empty() {
        return Ok(CheckoutResult {
            discarded: 0,
            failed: 0,
            errors: vec!["no hunk IDs provided".into()],
        });
    }

    let repo = Repository::open(repo_path).context("failed to open git repository")?;
    let hunk_metadata = scan_unstaged_hunk_metadata(&repo)?;

    let unique_requested: HashSet<&str> = request.hunk_ids.iter().map(String::as_str).collect();

    let mut discarded = 0usize;
    let mut failed = 0usize;
    let mut errors = Vec::new();

    // Separate untracked files from tracked changes
    let mut untracked_paths: Vec<String> = Vec::new();
    let mut untracked_hunk_count = 0usize;
    let mut tracked_ids: HashSet<String> = HashSet::new();

    for req_id in &unique_requested {
        match hunk_metadata.get(*req_id) {
            None => {
                errors.push(format!("hunk ID not found: {req_id}"));
                failed += 1;
            }
            Some(meta) if meta.is_untracked => {
                // Discarding an untracked file = deleting it
                if !untracked_paths.contains(&meta.file_path) {
                    untracked_paths.push(meta.file_path.clone());
                }
                untracked_hunk_count += 1;
            }
            Some(_) => {
                tracked_ids.insert(req_id.to_string());
            }
        }
    }

    // Delete untracked files (with path traversal guard)
    let repo_root = repo_path.canonicalize().context("failed to canonicalize repo path")?;
    for path in &untracked_paths {
        let full_path = repo_path.join(path);
        let canonical =
            full_path.canonicalize().with_context(|| format!("failed to resolve path: {path}"))?;
        if !canonical.starts_with(&repo_root) {
            errors.push(format!("path escapes repository boundary: {path}"));
            failed += 1;
            untracked_hunk_count -= 1;
            continue;
        }
        std::fs::remove_file(&canonical)
            .with_context(|| format!("failed to delete untracked file: {path}"))?;
    }
    discarded += untracked_hunk_count;

    // For tracked hunks, collect full hunk data and apply all reverse patches atomically
    if !tracked_ids.is_empty() {
        let mut opts = super::diff_opts_with_untracked();
        let diff =
            repo.diff_index_to_workdir(None, Some(&mut opts)).context("failed to generate diff")?;

        let hunks = collect_hunks_from_diff(&diff)?;

        // Build a single combined patch from all valid hunks
        let mut combined_patch = String::new();
        let mut valid_count = 0usize;

        for hunk_id in &tracked_ids {
            match hunks.get(hunk_id) {
                None => {
                    errors.push(format!("hunk ID not found in current diff: {hunk_id}"));
                    failed += 1;
                }
                Some(raw) => {
                    combined_patch.push_str(&reconstruct_reverse_patch(raw));
                    valid_count += 1;
                }
            }
        }

        if valid_count > 0 {
            let patch_diff = Diff::from_buffer(combined_patch.as_bytes())
                .context("failed to parse combined reverse patch")?;
            repo.apply(&patch_diff, ApplyLocation::WorkDir, None)
                .context("failed to apply reverse patches to working tree")?;
            discarded += valid_count;
        }
    }

    Ok(CheckoutResult { discarded, failed, errors })
}

/// Discard selected staged hunks (revert index → HEAD state).
pub fn checkout_staged(repo_path: &Path, request: &CheckoutRequest) -> Result<CheckoutResult> {
    if request.hunk_ids.is_empty() {
        return Ok(CheckoutResult {
            discarded: 0,
            failed: 0,
            errors: vec!["no hunk IDs provided".into()],
        });
    }

    let repo = Repository::open(repo_path).context("failed to open git repository")?;
    let hunk_metadata = scan_staged_hunk_metadata(&repo)?;

    let unique_requested: HashSet<&str> = request.hunk_ids.iter().map(String::as_str).collect();

    let mut discarded = 0usize;
    let mut failed = 0usize;
    let mut errors = Vec::new();

    // Separate newly-added files from modified files
    let mut added_paths: Vec<String> = Vec::new();
    let mut added_hunk_count = 0usize;
    let mut modified_ids: HashSet<String> = HashSet::new();

    for req_id in &unique_requested {
        match hunk_metadata.get(*req_id) {
            None => {
                errors.push(format!("hunk ID not found: {req_id}"));
                failed += 1;
            }
            Some(meta) if meta.is_added => {
                // Discarding a staged new file = removing from index
                if !added_paths.contains(&meta.file_path) {
                    added_paths.push(meta.file_path.clone());
                }
                added_hunk_count += 1;
            }
            Some(_) => {
                modified_ids.insert(req_id.to_string());
            }
        }
    }

    // Remove newly-added files from the index
    if !added_paths.is_empty() {
        let mut index = repo.index().context("failed to open index")?;
        for path in &added_paths {
            index
                .remove_path(Path::new(path))
                .with_context(|| format!("failed to remove from index: {path}"))?;
        }
        index.write().context("failed to write index")?;
        discarded += added_hunk_count;
    }

    // For modified files, collect staged hunk data and apply all reverse patches atomically
    if !modified_ids.is_empty() {
        let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());

        let mut opts = super::diff_opts_tracked_only();
        let diff = repo
            .diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))
            .context("failed to generate staged diff")?;

        let hunks = collect_hunks_from_diff(&diff)?;

        // Build a single combined patch from all valid hunks
        let mut combined_patch = String::new();
        let mut valid_count = 0usize;

        for hunk_id in &modified_ids {
            match hunks.get(hunk_id) {
                None => {
                    errors.push(format!("hunk ID not found in staged diff: {hunk_id}"));
                    failed += 1;
                }
                Some(raw) => {
                    combined_patch.push_str(&reconstruct_reverse_patch(raw));
                    valid_count += 1;
                }
            }
        }

        if valid_count > 0 {
            let patch_diff = Diff::from_buffer(combined_patch.as_bytes())
                .context("failed to parse combined reverse patch")?;
            repo.apply(&patch_diff, ApplyLocation::Index, None)
                .context("failed to apply reverse patches to index")?;
            discarded += valid_count;
        }
    }

    Ok(CheckoutResult { discarded, failed, errors })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff::{diff_staged, diff_unstaged};
    use git2::Signature;
    use std::fs;
    use tempfile::TempDir;

    /// Create a temp repo with an initial commit containing a multi-line file.
    fn setup_repo() -> (TempDir, Repository) {
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

        (dir, repo)
    }

    /// Create a temp repo with a file that produces 2 hunks when modified.
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
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[]).unwrap();
        }

        let mut modified = lines;
        modified[1] = "line 2 CHANGED".to_string();
        modified[18] = "line 19 CHANGED".to_string();
        fs::write(dir.path().join("file.txt"), modified.join("\n") + "\n").unwrap();

        (dir, repo)
    }

    // ===== Checkout unstaged tests =====

    #[test]
    fn checkout_single_unstaged_hunk() {
        let (dir, _repo) = setup_two_hunk_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        assert!(output.total_hunks >= 2);
        let first_id = output.files[0].hunks[0].id.clone();

        let request = CheckoutRequest { hunk_ids: vec![first_id] };
        let result = checkout_unstaged(dir.path(), &request).unwrap();
        assert_eq!(result.discarded, 1);
        assert_eq!(result.failed, 0);

        // Should still have remaining hunks
        let remaining = diff_unstaged(dir.path(), None).unwrap();
        assert!(remaining.total_hunks >= 1);
    }

    #[test]
    fn checkout_all_unstaged_hunks_restores_file() {
        let (dir, _repo) = setup_two_hunk_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let all_ids: Vec<String> = output.files[0].hunks.iter().map(|h| h.id.clone()).collect();

        let request = CheckoutRequest { hunk_ids: all_ids };
        let result = checkout_unstaged(dir.path(), &request).unwrap();
        assert!(result.discarded >= 2);
        assert_eq!(result.failed, 0);

        // No more unstaged changes
        let remaining = diff_unstaged(dir.path(), Some("file.txt")).unwrap();
        assert_eq!(remaining.total_hunks, 0);
    }

    #[test]
    fn checkout_unstaged_invalid_id() {
        let (dir, _repo) = setup_two_hunk_repo();

        let request = CheckoutRequest { hunk_ids: vec!["nonexistent".into()] };
        let result = checkout_unstaged(dir.path(), &request).unwrap();
        assert_eq!(result.discarded, 0);
        assert_eq!(result.failed, 1);
        assert!(result.errors[0].contains("not found"));
    }

    #[test]
    fn checkout_unstaged_empty_request() {
        let (dir, _repo) = setup_two_hunk_repo();

        let request = CheckoutRequest { hunk_ids: vec![] };
        let result = checkout_unstaged(dir.path(), &request).unwrap();
        assert_eq!(result.discarded, 0);
        assert!(result.errors[0].contains("no hunk IDs"));
    }

    #[test]
    fn checkout_unstaged_untracked_file_deletes_it() {
        let (dir, _repo) = setup_repo();

        fs::write(dir.path().join("untracked.txt"), "new stuff\n").unwrap();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let untracked = output.files.iter().find(|f| f.path == "untracked.txt").unwrap();
        let hunk_id = untracked.hunks[0].id.clone();

        let request = CheckoutRequest { hunk_ids: vec![hunk_id] };
        let result = checkout_unstaged(dir.path(), &request).unwrap();
        assert_eq!(result.discarded, 1);
        assert_eq!(result.failed, 0);

        // File should be deleted
        assert!(!dir.path().join("untracked.txt").exists());
    }

    #[test]
    fn checkout_unstaged_modified_restores_content() {
        let (dir, _repo) = setup_repo();

        let original = fs::read_to_string(dir.path().join("hello.txt")).unwrap();
        fs::write(dir.path().join("hello.txt"), "completely changed\n").unwrap();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let hunk_id = output.files[0].hunks[0].id.clone();

        let request = CheckoutRequest { hunk_ids: vec![hunk_id] };
        let result = checkout_unstaged(dir.path(), &request).unwrap();
        assert_eq!(result.discarded, 1);

        let restored = fs::read_to_string(dir.path().join("hello.txt")).unwrap();
        assert_eq!(restored, original);
    }

    // ===== Checkout staged tests =====

    #[test]
    fn checkout_staged_single_hunk() {
        let (dir, repo) = setup_repo();

        // Modify and stage
        fs::write(dir.path().join("hello.txt"), "line 1\nline 2 STAGED\nline 3\n").unwrap();
        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("hello.txt")).unwrap();
            index.write().unwrap();
        }

        let output = diff_staged(dir.path(), None).unwrap();
        assert!(output.total_hunks >= 1);
        let hunk_id = output.files[0].hunks[0].id.clone();

        let request = CheckoutRequest { hunk_ids: vec![hunk_id] };
        let result = checkout_staged(dir.path(), &request).unwrap();
        assert_eq!(result.discarded, 1);
        assert_eq!(result.failed, 0);

        // No more staged changes
        let remaining = diff_staged(dir.path(), None).unwrap();
        assert_eq!(remaining.total_hunks, 0);
    }

    #[test]
    fn checkout_staged_new_file_removes_from_index() {
        let (dir, repo) = setup_repo();

        fs::write(dir.path().join("new_file.txt"), "brand new\n").unwrap();
        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("new_file.txt")).unwrap();
            index.write().unwrap();
        }

        let output = diff_staged(dir.path(), None).unwrap();
        let new_file_hunk =
            output.files.iter().find(|f| f.path == "new_file.txt").unwrap().hunks[0].id.clone();

        let request = CheckoutRequest { hunk_ids: vec![new_file_hunk] };
        let result = checkout_staged(dir.path(), &request).unwrap();
        assert_eq!(result.discarded, 1);

        // File should no longer be in the staged diff
        let remaining = diff_staged(dir.path(), None).unwrap();
        assert!(remaining.files.iter().all(|f| f.path != "new_file.txt"));

        // File should still exist on disk (just unstaged now)
        assert!(dir.path().join("new_file.txt").exists());
    }

    #[test]
    fn checkout_staged_invalid_id() {
        let (dir, _repo) = setup_repo();

        let request = CheckoutRequest { hunk_ids: vec!["nonexistent".into()] };
        let result = checkout_staged(dir.path(), &request).unwrap();
        assert_eq!(result.discarded, 0);
        assert_eq!(result.failed, 1);
        assert!(result.errors[0].contains("not found"));
    }

    #[test]
    fn checkout_staged_empty_request() {
        let (dir, _repo) = setup_repo();

        let request = CheckoutRequest { hunk_ids: vec![] };
        let result = checkout_staged(dir.path(), &request).unwrap();
        assert_eq!(result.discarded, 0);
        assert!(result.errors[0].contains("no hunk IDs"));
    }
}
