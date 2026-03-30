use crate::cli::OutputFormat;
use crate::models::{CheckoutResult, DiffOutput, Response, StageResult, StatusSummary};

/// Print a value as a JSON `Response::success` envelope.
fn print_json(data: &impl serde::Serialize) {
    let resp = Response::success(data);
    println!("{}", serde_json::to_string(&resp).unwrap());
}

/// Print diff output in the requested format.
pub fn print_diff(output: &DiffOutput, format: &OutputFormat) {
    match format {
        OutputFormat::Json => print_json(output),
        OutputFormat::Toon => print!("{}", crate::toon::format_diff(output)),
    }
}

/// Print stage result.
pub fn print_stage_result(result: &StageResult, format: &OutputFormat) {
    match format {
        OutputFormat::Json => print_json(result),
        OutputFormat::Toon => print!("{}", crate::toon::format_stage_result(result)),
    }
}

/// Print checkout result.
pub fn print_checkout_result(result: &CheckoutResult, format: &OutputFormat) {
    match format {
        OutputFormat::Json => print_json(result),
        OutputFormat::Toon => print!("{}", crate::toon::format_checkout_result(result)),
    }
}

/// Print status summary.
pub fn print_status(status: &StatusSummary, format: &OutputFormat) {
    match format {
        OutputFormat::Json => print_json(status),
        OutputFormat::Toon => print!("{}", crate::toon::format_status(status)),
    }
}
