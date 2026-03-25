use anyhow::{Context, Result};
use git2::{DiffOptions, Repository};
use std::path::Path;

use crate::models::StatusSummary;

/// Get a summary of staged vs unstaged changes.
pub fn get_status(repo_path: &Path) -> Result<StatusSummary> {
    let repo = Repository::open(repo_path).context("failed to open git repository")?;

    // Staged: diff between HEAD tree and index
    let (staged_files, staged_hunks) = if let Ok(head) = repo.head() {
        let tree = head
            .peel_to_tree()
            .context("failed to peel HEAD to tree")?;
        let diff = repo
            .diff_tree_to_index(Some(&tree), None, None)
            .context("failed to diff tree to index")?;
        count_files_and_hunks(&diff)?
    } else {
        // No HEAD (empty repo) — check if index has any entries
        let index = repo.index().context("failed to read index")?;
        if index.is_empty() {
            (0, 0)
        } else {
            // Everything in the index is "staged" relative to empty tree
            let diff = repo
                .diff_tree_to_index(None, None, None)
                .context("failed to diff empty tree to index")?;
            count_files_and_hunks(&diff)?
        }
    };

    // Unstaged: diff between index and working directory
    let mut opts = DiffOptions::new();
    opts.include_untracked(true);
    opts.show_untracked_content(true);

    let unstaged_diff = repo
        .diff_index_to_workdir(None, Some(&mut opts))
        .context("failed to diff index to workdir")?;
    let (unstaged_files, unstaged_hunks) = count_files_and_hunks(&unstaged_diff)?;

    Ok(StatusSummary {
        staged_files,
        unstaged_files,
        staged_hunks,
        unstaged_hunks,
    })
}

fn count_files_and_hunks(diff: &git2::Diff) -> Result<(usize, usize)> {
    let mut files = 0usize;
    let mut hunks = 0usize;
    diff.foreach(
        &mut |_, _| {
            files += 1;
            true
        },
        None,
        Some(&mut |_, _| {
            hunks += 1;
            true
        }),
        None,
    )
    .context("failed to iterate diff for status counts")?;
    Ok((files, hunks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff::diff_unstaged;
    use crate::git::stage::stage_selection;
    use crate::models::StageRequest;
    use git2::Signature;
    use std::fs;
    use tempfile::TempDir;

    fn setup_repo() -> (TempDir, Repository) {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();

        fs::write(dir.path().join("a.txt"), "aaa\n").unwrap();
        fs::write(dir.path().join("b.txt"), "bbb\n").unwrap();

        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.add_path(Path::new("b.txt")).unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
                .unwrap();
        }

        (dir, repo)
    }

    #[test]
    fn status_clean_repo() {
        let (dir, _repo) = setup_repo();
        let status = get_status(dir.path()).unwrap();
        assert_eq!(status.staged_files, 0);
        assert_eq!(status.staged_hunks, 0);
        assert_eq!(status.unstaged_files, 0);
        assert_eq!(status.unstaged_hunks, 0);
    }

    #[test]
    fn status_unstaged_changes() {
        let (dir, _repo) = setup_repo();
        fs::write(dir.path().join("a.txt"), "modified\n").unwrap();
        fs::write(dir.path().join("b.txt"), "also modified\n").unwrap();

        let status = get_status(dir.path()).unwrap();
        assert_eq!(status.staged_files, 0);
        assert_eq!(status.unstaged_files, 2);
        assert_eq!(status.unstaged_hunks, 2);
    }

    #[test]
    fn status_after_partial_staging() {
        let (dir, _repo) = setup_repo();
        fs::write(dir.path().join("a.txt"), "modified\n").unwrap();
        fs::write(dir.path().join("b.txt"), "also modified\n").unwrap();

        // Stage one file's hunk
        let output = diff_unstaged(dir.path(), Some("a.txt")).unwrap();
        let hunk_id = output.files[0].hunks[0].id.clone();
        stage_selection(
            dir.path(),
            &StageRequest {
                hunk_ids: vec![hunk_id],
                line_selections: vec![],
            },
        )
        .unwrap();

        let status = get_status(dir.path()).unwrap();
        assert_eq!(status.staged_files, 1);
        assert_eq!(status.staged_hunks, 1);
        assert_eq!(status.unstaged_files, 1);
        assert_eq!(status.unstaged_hunks, 1);
    }

    #[test]
    fn status_empty_repo() {
        let dir = TempDir::new().unwrap();
        let _repo = Repository::init(dir.path()).unwrap();

        let status = get_status(dir.path()).unwrap();
        assert_eq!(status.staged_files, 0);
        assert_eq!(status.unstaged_files, 0);
    }
}
