use crate::cli::OutputFormat;
use crate::models::{DiffOutput, LineTag, Response, StageResult, StatusSummary};
use std::io::IsTerminal;

/// Resolve Auto format to Json or Human based on terminal detection.
pub fn resolve_format(format: &OutputFormat) -> OutputFormat {
    match format {
        OutputFormat::Auto => {
            if std::io::stdout().is_terminal() {
                OutputFormat::Human
            } else {
                OutputFormat::Json
            }
        }
        other => other.clone(),
    }
}

/// Print diff output in the requested format.
pub fn print_diff(output: &DiffOutput, format: &OutputFormat) {
    match resolve_format(format) {
        OutputFormat::Json => {
            let resp = Response::success(output);
            println!("{}", serde_json::to_string(&resp).unwrap());
        }
        OutputFormat::Human => {
            if output.files.is_empty() {
                println!("No unstaged changes.");
                return;
            }
            for file in &output.files {
                println!("--- a/{}", file.path);
                println!("+++ b/{}", file.path);
                println!("Status: {:?}", file.status);
                for hunk in &file.hunks {
                    println!();
                    println!("[{}] {}", hunk.id, hunk.header);
                    for line in &hunk.lines {
                        let prefix = match line.tag {
                            LineTag::Insert => "+",
                            LineTag::Delete => "-",
                            LineTag::Equal => " ",
                        };
                        print!("{}{}", prefix, line.content);
                    }
                }
                println!();
            }
            println!(
                "Total: {} file(s), {} hunk(s)",
                output.files.len(),
                output.total_hunks
            );
        }
        OutputFormat::Auto => unreachable!("resolve_format always resolves Auto"),
    }
}

/// Print stage result.
pub fn print_stage_result(result: &StageResult, format: &OutputFormat) {
    match resolve_format(format) {
        OutputFormat::Json => {
            let resp = Response::success(result);
            println!("{}", serde_json::to_string(&resp).unwrap());
        }
        OutputFormat::Human => {
            println!("Staged: {}, Failed: {}", result.staged, result.failed);
            for err in &result.errors {
                eprintln!("  error: {err}");
            }
        }
        OutputFormat::Auto => unreachable!(),
    }
}

/// Print status summary.
pub fn print_status(status: &StatusSummary, format: &OutputFormat) {
    match resolve_format(format) {
        OutputFormat::Json => {
            let resp = Response::success(status);
            println!("{}", serde_json::to_string(&resp).unwrap());
        }
        OutputFormat::Human => {
            println!(
                "Staged:   {} file(s), {} hunk(s)",
                status.staged_files, status.staged_hunks
            );
            println!(
                "Unstaged: {} file(s), {} hunk(s)",
                status.unstaged_files, status.unstaged_hunks
            );
        }
        OutputFormat::Auto => unreachable!(),
    }
}
