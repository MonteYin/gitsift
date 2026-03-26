use git2::DiffOptions;

pub mod diff;
pub mod stage;
pub mod status;

/// Default context lines for all diffs.
const CONTEXT_LINES: u32 = 3;

/// Create `DiffOptions` including untracked file content.
///
/// Used for: diff, status, and hunk metadata scanning.
pub fn diff_opts_with_untracked() -> DiffOptions {
    let mut opts = DiffOptions::new();
    opts.context_lines(CONTEXT_LINES);
    opts.include_untracked(true);
    opts.show_untracked_content(true);
    opts
}

/// Create `DiffOptions` for tracked files only.
///
/// Used for: applying hunks to the index (untracked files handled separately).
pub fn diff_opts_tracked_only() -> DiffOptions {
    let mut opts = DiffOptions::new();
    opts.context_lines(CONTEXT_LINES);
    opts
}

/// Extract the file path from a `DiffDelta`, preferring `new_file`.
pub fn delta_path(delta: &git2::DiffDelta) -> String {
    delta
        .new_file()
        .path()
        .or_else(|| delta.old_file().path())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Extract and clean the header string from a git2 `DiffHunk`.
pub fn hunk_header(hunk: &git2::DiffHunk<'_>) -> String {
    String::from_utf8_lossy(hunk.header()).trim().to_string()
}

/// Check if a `DiffDelta` represents a binary file.
pub fn is_binary_delta(delta: &git2::DiffDelta) -> bool {
    delta.new_file().is_binary() || delta.old_file().is_binary()
}
