use anyhow::Result;
use std::path::Path;

/// Enter the JSON-lines protocol loop: read requests from stdin, write responses to stdout.
pub fn run_protocol(_repo_path: &Path) -> Result<()> {
    todo!("implement stdin/stdout JSON-lines protocol loop")
}
