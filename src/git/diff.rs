use anyhow::{Context, Result};
use git2::{Delta, DiffOptions, Repository};
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::models::{DiffOutput, FileChange, FileStatus, Hunk, HunkLine, LineTag};

/// Generate a stable hunk ID from file path, old start line, and header.
///
/// Note: Uses `DefaultHasher` which is not guaranteed stable across Rust versions.
/// IDs are ephemeral — valid only within a single diff→stage cycle using the same binary.
/// Do not persist or compare across different gitsift versions.
pub fn hunk_id(file_path: &str, old_start: u32, header: &str) -> String {
    let mut hasher = DefaultHasher::new();
    file_path.hash(&mut hasher);
    old_start.hash(&mut hasher);
    header.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Map git2 Delta to our FileStatus.
fn delta_to_status(delta: Delta) -> FileStatus {
    match delta {
        Delta::Added | Delta::Untracked => FileStatus::Added,
        Delta::Deleted => FileStatus::Deleted,
        Delta::Renamed => FileStatus::Renamed,
        _ => FileStatus::Modified,
    }
}

/// Mutable state shared across git2 foreach callbacks via RefCell.
struct DiffState {
    files: Vec<FileChange>,
    current_file: Option<FileChange>,
    current_hunk: Option<Hunk>,
}

impl DiffState {
    fn new() -> Self {
        Self {
            files: Vec::new(),
            current_file: None,
            current_hunk: None,
        }
    }

    /// Flush current hunk into current file, then flush current file into files list.
    fn flush_file(&mut self) {
        self.flush_hunk();
        if let Some(file) = self.current_file.take() {
            self.files.push(file);
        }
    }

    /// Flush current hunk into current file.
    fn flush_hunk(&mut self) {
        if let Some(hunk) = self.current_hunk.take()
            && let Some(file) = self.current_file.as_mut()
        {
            file.hunks.push(hunk);
        }
    }

    fn finalize(mut self) -> Vec<FileChange> {
        self.flush_file();
        self.files
    }
}

/// Generate a structured diff of unstaged changes.
///
/// Compares the index (staging area) against the working directory.
/// For new/untracked files, compares against an empty tree.
pub fn diff_unstaged(repo_path: &Path, file_filter: Option<&str>) -> Result<DiffOutput> {
    let repo = Repository::open(repo_path).context("failed to open git repository")?;

    let mut opts = DiffOptions::new();
    opts.context_lines(3);
    opts.include_untracked(true);
    opts.show_untracked_content(true);

    if let Some(filter) = file_filter {
        opts.pathspec(filter);
    }

    let diff = repo
        .diff_index_to_workdir(None, Some(&mut opts))
        .context("failed to generate diff")?;

    let state = RefCell::new(DiffState::new());

    diff.foreach(
        &mut |delta, _progress| {
            let mut s = state.borrow_mut();
            s.flush_file();

            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();

            // Skip binary files
            if delta.new_file().is_binary() || delta.old_file().is_binary() {
                return true;
            }

            s.current_file = Some(FileChange {
                path,
                status: delta_to_status(delta.status()),
                hunks: Vec::new(),
            });
            true
        },
        None,
        Some(&mut |_delta, hunk| {
            let mut s = state.borrow_mut();
            s.flush_hunk();

            let file_path = s
                .current_file
                .as_ref()
                .map(|f| f.path.as_str())
                .unwrap_or("");

            let header = String::from_utf8_lossy(hunk.header()).trim().to_string();

            s.current_hunk = Some(Hunk {
                id: hunk_id(file_path, hunk.old_start(), &header),
                file_path: file_path.to_string(),
                old_start: hunk.old_start(),
                old_lines: hunk.old_lines(),
                new_start: hunk.new_start(),
                new_lines: hunk.new_lines(),
                header,
                lines: Vec::new(),
            });
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

            if let Some(hunk) = s.current_hunk.as_mut() {
                hunk.lines.push(hunk_line);
            }
            true
        }),
    )
    .context("failed to iterate diff")?;

    let files = state.into_inner().finalize();
    let total_hunks = files.iter().map(|f| f.hunks.len()).sum();

    Ok(DiffOutput { files, total_hunks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Signature;
    use std::fs;
    use tempfile::TempDir;

    /// Create a temp git repo with an initial commit containing a file.
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
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
                .unwrap();
        }

        (dir, repo)
    }

    /// Helper: commit all current index state.
    fn commit(repo: &Repository, msg: &str) {
        let sig = Signature::now("test", "test@test.com").unwrap();
        let mut index = repo.index().unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &[&head])
            .unwrap();
    }

    #[test]
    fn diff_modified_file() {
        let (dir, _repo) = setup_repo();

        fs::write(
            dir.path().join("hello.txt"),
            "line 1\nline 2 modified\nline 3\nline 4\n",
        )
        .unwrap();

        let output = diff_unstaged(dir.path(), None).unwrap();
        assert_eq!(output.files.len(), 1);
        assert_eq!(output.files[0].path, "hello.txt");
        assert_eq!(output.files[0].status, FileStatus::Modified);
        assert_eq!(output.total_hunks, 1);

        let hunk = &output.files[0].hunks[0];
        assert!(!hunk.id.is_empty());
        assert_eq!(hunk.file_path, "hello.txt");

        let has_delete = hunk.lines.iter().any(|l| l.tag == LineTag::Delete);
        let has_insert = hunk.lines.iter().any(|l| l.tag == LineTag::Insert);
        assert!(has_delete);
        assert!(has_insert);
    }

    #[test]
    fn diff_added_file() {
        let (dir, _repo) = setup_repo();

        fs::write(dir.path().join("new.txt"), "new content\n").unwrap();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let new_file = output.files.iter().find(|f| f.path == "new.txt");
        assert!(new_file.is_some());
        let new_file = new_file.unwrap();
        assert_eq!(new_file.status, FileStatus::Added);
        // Must have hunks with actual content (not empty)
        assert!(!new_file.hunks.is_empty(), "added file must have hunks");
        assert!(
            new_file.hunks[0].lines.iter().any(|l| l.tag == LineTag::Insert),
            "added file hunk must have insert lines"
        );
    }

    #[test]
    fn diff_deleted_file() {
        let (dir, _repo) = setup_repo();

        fs::remove_file(dir.path().join("hello.txt")).unwrap();

        let output = diff_unstaged(dir.path(), None).unwrap();
        assert_eq!(output.files.len(), 1);
        assert_eq!(output.files[0].status, FileStatus::Deleted);

        for line in &output.files[0].hunks[0].lines {
            assert_eq!(line.tag, LineTag::Delete);
        }
    }

    #[test]
    fn diff_no_changes() {
        let (dir, _repo) = setup_repo();

        let output = diff_unstaged(dir.path(), None).unwrap();
        assert_eq!(output.files.len(), 0);
        assert_eq!(output.total_hunks, 0);
    }

    #[test]
    fn diff_file_filter() {
        let (dir, _repo) = setup_repo();

        fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();
        fs::write(dir.path().join("other.txt"), "other\n").unwrap();

        let output = diff_unstaged(dir.path(), Some("hello.txt")).unwrap();
        assert_eq!(output.files.len(), 1);
        assert_eq!(output.files[0].path, "hello.txt");

        let output = diff_unstaged(dir.path(), Some("other.txt")).unwrap();
        assert_eq!(output.files.len(), 1);
        assert_eq!(output.files[0].path, "other.txt");
    }

    #[test]
    fn hunk_ids_are_stable() {
        let (dir, _repo) = setup_repo();

        fs::write(dir.path().join("hello.txt"), "changed\n").unwrap();

        let output1 = diff_unstaged(dir.path(), None).unwrap();
        let output2 = diff_unstaged(dir.path(), None).unwrap();

        assert_eq!(output1.files[0].hunks[0].id, output2.files[0].hunks[0].id);
    }

    #[test]
    fn diff_multiple_hunks() {
        let (dir, repo) = setup_repo();

        // Create a file with enough lines to produce separate hunks
        let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        fs::write(dir.path().join("big.txt"), lines.join("\n") + "\n").unwrap();

        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("big.txt")).unwrap();
            index.write().unwrap();
        }
        commit(&repo, "add big file");

        // Modify lines far apart to create two separate hunks
        let mut modified = lines.clone();
        modified[1] = "line 2 CHANGED".to_string();
        modified[18] = "line 19 CHANGED".to_string();
        fs::write(dir.path().join("big.txt"), modified.join("\n") + "\n").unwrap();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let big_file = output.files.iter().find(|f| f.path == "big.txt").unwrap();
        assert!(
            big_file.hunks.len() >= 2,
            "expected 2+ hunks, got {}",
            big_file.hunks.len()
        );

        // Each hunk should have a unique ID
        let ids: Vec<&str> = big_file.hunks.iter().map(|h| h.id.as_str()).collect();
        let unique: std::collections::HashSet<&&str> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "hunk IDs must be unique");
    }

    #[test]
    fn diff_empty_repo() {
        let dir = TempDir::new().unwrap();
        let _repo = Repository::init(dir.path()).unwrap();

        let output = diff_unstaged(dir.path(), None).unwrap();
        assert_eq!(output.files.len(), 0);
        assert_eq!(output.total_hunks, 0);
    }

    #[test]
    fn diff_line_numbers_correct() {
        let (dir, _repo) = setup_repo();

        fs::write(
            dir.path().join("hello.txt"),
            "line 1\nINSERTED\nline 2\nline 3\n",
        )
        .unwrap();

        let output = diff_unstaged(dir.path(), None).unwrap();
        let hunk = &output.files[0].hunks[0];

        let inserted = hunk
            .lines
            .iter()
            .find(|l| l.tag == LineTag::Insert && l.content.contains("INSERTED"))
            .expect("should find inserted line");

        assert!(inserted.old_lineno.is_none(), "insert has no old line number");
        assert!(inserted.new_lineno.is_some(), "insert has new line number");
    }
}
