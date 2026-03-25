use anyhow::Result;
use std::path::Path;

use crate::models::DiffOutput;

/// Generate a structured diff of unstaged changes.
pub fn diff_unstaged(_repo_path: &Path, _file_filter: Option<&str>) -> Result<DiffOutput> {
    todo!("implement diff_index_to_workdir via git2")
}
