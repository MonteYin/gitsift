//! Compact output format inspired by TOON (Token-Oriented Object Notation).
//!
//! Key ideas borrowed from TOON to reduce token consumption:
//! - Indentation-based nesting instead of braces/brackets
//! - Tabular rows for uniform arrays: declare keys once, list values as CSV rows
//! - Strip context lines (tag=equal) — agents don't need them for staging
//! - Remove redundant `file_path` from hunks (already in parent `FileChange`)

use std::fmt::Write;

use crate::models::{DiffOutput, LineTag, Response, StageResult, StatusSummary};

/// Format a diff response in compact TOON-like format.
pub fn format_diff(output: &DiffOutput) -> String {
    let mut buf = String::new();
    writeln!(buf, "version: {}", crate::models::PROTOCOL_VERSION).unwrap();
    writeln!(buf, "ok: true").unwrap();
    writeln!(buf, "total_hunks: {}", output.total_hunks).unwrap();

    if output.files.is_empty() {
        writeln!(buf, "files[0]:").unwrap();
        return buf;
    }

    writeln!(buf, "files[{}]:", output.files.len()).unwrap();
    for file in &output.files {
        writeln!(buf, "  - path: {}", file.path).unwrap();
        writeln!(buf, "    status: {}", file.status).unwrap();

        if file.hunks.is_empty() {
            writeln!(buf, "    hunks[0]:").unwrap();
            continue;
        }

        writeln!(buf, "    hunks[{}]:", file.hunks.len()).unwrap();
        for hunk in &file.hunks {
            writeln!(buf, "      - id: {}", hunk.id).unwrap();
            writeln!(buf, "        header: {}", hunk.header).unwrap();
            writeln!(
                buf,
                "        old_start: {} old_lines: {} new_start: {} new_lines: {}",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            )
            .unwrap();

            // Filter out context lines, then output as tabular rows
            let change_lines: Vec<_> =
                hunk.lines.iter().filter(|l| l.tag != LineTag::Equal).collect();

            if change_lines.is_empty() {
                writeln!(buf, "        lines[0]:").unwrap();
            } else {
                writeln!(buf, "        lines[{}]{{tag,content,old,new}}:", change_lines.len())
                    .unwrap();
                for line in &change_lines {
                    let tag = match line.tag {
                        LineTag::Insert => "+",
                        LineTag::Delete => "-",
                        LineTag::Equal => " ",
                    };
                    let old = line.old_lineno.map_or(String::new(), |n| n.to_string());
                    let new = line.new_lineno.map_or(String::new(), |n| n.to_string());
                    // Escape content: replace newlines for single-line row
                    let content = escape_content(&line.content);
                    writeln!(buf, "          {tag},{content},{old},{new}").unwrap();
                }
            }
        }
    }

    buf
}

/// Format a stage result response in compact format.
pub fn format_stage_result(result: &StageResult) -> String {
    let resp = Response::success(result);
    // StageResult is small enough — just use compact key: value
    let mut buf = String::new();
    writeln!(buf, "version: {}", resp.version).unwrap();
    writeln!(buf, "ok: true").unwrap();
    writeln!(buf, "staged: {}", result.staged).unwrap();
    writeln!(buf, "failed: {}", result.failed).unwrap();
    if !result.errors.is_empty() {
        writeln!(buf, "errors[{}]:", result.errors.len()).unwrap();
        for err in &result.errors {
            writeln!(buf, "  - {err}").unwrap();
        }
    }
    buf
}

/// Format a status summary response in compact format.
pub fn format_status(status: &StatusSummary) -> String {
    let mut buf = String::new();
    writeln!(buf, "version: {}", crate::models::PROTOCOL_VERSION).unwrap();
    writeln!(buf, "ok: true").unwrap();
    writeln!(buf, "staged_files: {}", status.staged_files).unwrap();
    writeln!(buf, "staged_hunks: {}", status.staged_hunks).unwrap();
    writeln!(buf, "unstaged_files: {}", status.unstaged_files).unwrap();
    writeln!(buf, "unstaged_hunks: {}", status.unstaged_hunks).unwrap();
    buf
}

/// Escape content string for tabular row output.
/// Replaces literal newlines with `\n` and commas with `\,` to keep rows single-line.
fn escape_content(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace(',', "\\,")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn sample_diff() -> DiffOutput {
        DiffOutput {
            files: vec![FileChange {
                path: "src/main.rs".into(),
                status: FileStatus::Modified,
                hunks: vec![Hunk {
                    id: "abc123".into(),
                    file_path: "src/main.rs".into(),
                    old_start: 1,
                    old_lines: 5,
                    new_start: 1,
                    new_lines: 6,
                    header: "@@ -1,5 +1,6 @@".into(),
                    lines: vec![
                        HunkLine {
                            tag: LineTag::Equal,
                            content: "unchanged\n".into(),
                            old_lineno: Some(1),
                            new_lineno: Some(1),
                        },
                        HunkLine {
                            tag: LineTag::Delete,
                            content: "old line\n".into(),
                            old_lineno: Some(2),
                            new_lineno: None,
                        },
                        HunkLine {
                            tag: LineTag::Insert,
                            content: "new line\n".into(),
                            old_lineno: None,
                            new_lineno: Some(2),
                        },
                        HunkLine {
                            tag: LineTag::Equal,
                            content: "also unchanged\n".into(),
                            old_lineno: Some(3),
                            new_lineno: Some(3),
                        },
                    ],
                }],
            }],
            total_hunks: 1,
        }
    }

    #[test]
    fn strips_context_lines() {
        let out = format_diff(&sample_diff());
        assert!(!out.contains("unchanged"), "context lines should be removed");
    }

    #[test]
    fn no_file_path_in_hunks() {
        let out = format_diff(&sample_diff());
        // file_path should not appear as a hunk field
        // "path: src/main.rs" appears once at file level, not at hunk level
        let hunk_section = out.split("- id: abc123").nth(1).unwrap();
        assert!(!hunk_section.contains("file_path:"));
    }

    #[test]
    fn preserves_hunk_id() {
        let out = format_diff(&sample_diff());
        assert!(out.contains("id: abc123"));
    }

    #[test]
    fn tabular_header_present() {
        let out = format_diff(&sample_diff());
        assert!(out.contains("lines[2]{tag,content,old,new}:"));
    }

    #[test]
    fn change_lines_as_csv_rows() {
        let out = format_diff(&sample_diff());
        assert!(out.contains("-,old line\\n,2,"));
        assert!(out.contains("+,new line\\n,,2"));
    }

    #[test]
    fn empty_diff() {
        let empty = DiffOutput { files: vec![], total_hunks: 0 };
        let out = format_diff(&empty);
        assert!(out.contains("total_hunks: 0"));
        assert!(out.contains("files[0]:"));
    }

    #[test]
    fn stage_result_format() {
        let result = StageResult { staged: 2, failed: 1, errors: vec!["hunk not found".into()] };
        let out = format_stage_result(&result);
        assert!(out.contains("staged: 2"));
        assert!(out.contains("failed: 1"));
        assert!(out.contains("- hunk not found"));
    }

    #[test]
    fn status_format() {
        let status = StatusSummary {
            staged_files: 1,
            staged_hunks: 2,
            unstaged_files: 3,
            unstaged_hunks: 4,
        };
        let out = format_status(&status);
        assert!(out.contains("staged_files: 1"));
        assert!(out.contains("unstaged_hunks: 4"));
    }

    #[test]
    fn escapes_special_chars() {
        assert_eq!(escape_content("a,b\nc\\d"), "a\\,b\\nc\\\\d");
    }
}
