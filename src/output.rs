use crate::cli::OutputFormat;
use crate::models::DiffOutput;
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
            println!("{}", serde_json::to_string(output).unwrap());
        }
        OutputFormat::Human => {
            // TODO: colored unified diff via similar
            println!("{}", serde_json::to_string_pretty(output).unwrap());
        }
        OutputFormat::Auto => unreachable!("resolve_format always resolves Auto"),
    }
}
