use anyhow::Result;
use std::path::Path;

use crate::models::StatusSummary;

/// Get a summary of staged vs unstaged changes.
pub fn get_status(_repo_path: &Path) -> Result<StatusSummary> {
    todo!("implement status summary via git2")
}
