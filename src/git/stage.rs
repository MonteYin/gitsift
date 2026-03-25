use anyhow::Result;
use std::path::Path;

use crate::models::{StageRequest, StageResult};

/// Stage selected hunks/lines to the git index.
pub fn stage_selection(_repo_path: &Path, _request: &StageRequest) -> Result<StageResult> {
    todo!("implement hunk/line staging via git2 apply")
}
