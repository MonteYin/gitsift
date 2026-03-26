use crate::cli::OutputFormat;
use crate::models::{DiffOutput, LineTag, Response, StageResult, StatusSummary};
use std::io::IsTerminal;

/// Resolve Auto format to Json or Human based on terminal detection.
fn resolve_format(format: &OutputFormat) -> OutputFormat {
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

/// Print a value as a JSON `Response::success` envelope.
fn print_json(data: &impl serde::Serialize) {
    let resp = Response::success(data);
    println!("{}", serde_json::to_string(&resp).unwrap());
}

/// Print diff output in the requested format.
pub fn print_diff(output: &DiffOutput, format: &OutputFormat) {
    match resolve_format(format) {
        OutputFormat::Json => print_json(output),
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
            println!("Total: {} file(s), {} hunk(s)", output.files.len(), output.total_hunks);
        }
        OutputFormat::Auto => unreachable!("resolve_format always resolves Auto"),
    }
}

/// Print stage result.
pub fn print_stage_result(result: &StageResult, format: &OutputFormat) {
    match resolve_format(format) {
        OutputFormat::Json => print_json(result),
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
        OutputFormat::Json => print_json(status),
        OutputFormat::Human => {
            println!("Staged:   {} file(s), {} hunk(s)", status.staged_files, status.staged_hunks);
            println!(
                "Unstaged: {} file(s), {} hunk(s)",
                status.unstaged_files, status.unstaged_hunks
            );
        }
        OutputFormat::Auto => unreachable!(),
    }
}
