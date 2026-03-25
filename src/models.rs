use serde::{Deserialize, Serialize};

/// A single line within a hunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HunkLine {
    pub tag: LineTag,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_lineno: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_lineno: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineTag {
    Equal,
    Insert,
    Delete,
}

/// A diff hunk with stable ID for agent reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hunk {
    pub id: String,
    pub file_path: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
    pub lines: Vec<HunkLine>,
}

/// File-level change metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub status: FileStatus,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
}

/// Output of `gitsift diff`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct DiffOutput {
    pub files: Vec<FileChange>,
    pub total_hunks: usize,
}

/// Input for `gitsift stage --from-stdin`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct StageRequest {
    /// Stage entire hunks by ID.
    #[serde(default)]
    pub hunk_ids: Vec<String>,
    /// Stage individual lines within hunks.
    #[serde(default)]
    pub line_selections: Vec<LineSelection>,
}

/// Select specific lines within a hunk to stage.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct LineSelection {
    pub hunk_id: String,
    /// Line indices (0-based within the hunk's lines array) to include.
    pub line_indices: Vec<usize>,
}

/// Result of a staging operation.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct StageResult {
    pub staged: usize,
    pub failed: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub errors: Vec<String>,
}

/// Generic JSON response envelope.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Response<T> {
    pub version: u8,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> Response<T> {
    pub fn success(data: T) -> Self {
        Self {
            version: 1,
            ok: true,
            data: Some(data),
            error: None,
        }
    }
}

impl Response<()> {
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            version: 1,
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

/// JSON-lines protocol request.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum ProtocolRequest {
    Diff {
        #[serde(default)]
        params: DiffParams,
    },
    Stage {
        params: StageRequest,
    },
    Status,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DiffParams {
    pub file: Option<String>,
}

/// Staging status summary.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct StatusSummary {
    pub staged_files: usize,
    pub unstaged_files: usize,
    pub staged_hunks: usize,
    pub unstaged_hunks: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- HunkLine round-trip ---

    #[test]
    fn hunkline_insert_roundtrip() {
        let line = HunkLine {
            tag: LineTag::Insert,
            content: "+hello world\n".into(),
            old_lineno: None,
            new_lineno: Some(5),
        };
        let json = serde_json::to_string(&line).unwrap();
        let back: HunkLine = serde_json::from_str(&json).unwrap();
        assert_eq!(line, back);
        // old_lineno should be omitted
        assert!(!json.contains("old_lineno"));
    }

    #[test]
    fn hunkline_delete_roundtrip() {
        let line = HunkLine {
            tag: LineTag::Delete,
            content: "-removed\n".into(),
            old_lineno: Some(3),
            new_lineno: None,
        };
        let json = serde_json::to_string(&line).unwrap();
        let back: HunkLine = serde_json::from_str(&json).unwrap();
        assert_eq!(line, back);
        assert!(!json.contains("new_lineno"));
    }

    #[test]
    fn hunkline_equal_roundtrip() {
        let line = HunkLine {
            tag: LineTag::Equal,
            content: " context\n".into(),
            old_lineno: Some(1),
            new_lineno: Some(1),
        };
        let json = serde_json::to_string(&line).unwrap();
        let back: HunkLine = serde_json::from_str(&json).unwrap();
        assert_eq!(line, back);
    }

    // --- LineTag JSON values ---

    #[test]
    fn linetag_json_values() {
        assert_eq!(serde_json::to_string(&LineTag::Equal).unwrap(), "\"equal\"");
        assert_eq!(
            serde_json::to_string(&LineTag::Insert).unwrap(),
            "\"insert\""
        );
        assert_eq!(
            serde_json::to_string(&LineTag::Delete).unwrap(),
            "\"delete\""
        );
    }

    // --- FileStatus JSON values ---

    #[test]
    fn filestatus_json_values() {
        assert_eq!(
            serde_json::to_string(&FileStatus::Modified).unwrap(),
            "\"modified\""
        );
        assert_eq!(
            serde_json::to_string(&FileStatus::Added).unwrap(),
            "\"added\""
        );
        assert_eq!(
            serde_json::to_string(&FileStatus::Deleted).unwrap(),
            "\"deleted\""
        );
        assert_eq!(
            serde_json::to_string(&FileStatus::Renamed).unwrap(),
            "\"renamed\""
        );
    }

    // --- Hunk round-trip ---

    #[test]
    fn hunk_roundtrip() {
        let hunk = Hunk {
            id: "abc123".into(),
            file_path: "src/main.rs".into(),
            old_start: 10,
            old_lines: 5,
            new_start: 10,
            new_lines: 7,
            header: "@@ -10,5 +10,7 @@".into(),
            lines: vec![
                HunkLine {
                    tag: LineTag::Equal,
                    content: " context\n".into(),
                    old_lineno: Some(10),
                    new_lineno: Some(10),
                },
                HunkLine {
                    tag: LineTag::Delete,
                    content: "-old line\n".into(),
                    old_lineno: Some(11),
                    new_lineno: None,
                },
                HunkLine {
                    tag: LineTag::Insert,
                    content: "+new line\n".into(),
                    old_lineno: None,
                    new_lineno: Some(11),
                },
            ],
        };
        let json = serde_json::to_string(&hunk).unwrap();
        let back: Hunk = serde_json::from_str(&json).unwrap();
        assert_eq!(hunk, back);
    }

    // --- DiffOutput round-trip ---

    #[test]
    fn diff_output_roundtrip() {
        let output = DiffOutput {
            files: vec![FileChange {
                path: "src/lib.rs".into(),
                status: FileStatus::Modified,
                hunks: vec![],
            }],
            total_hunks: 0,
        };
        let json = serde_json::to_string(&output).unwrap();
        let back: DiffOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, back);
    }

    #[test]
    fn diff_output_empty() {
        let output = DiffOutput {
            files: vec![],
            total_hunks: 0,
        };
        let json = serde_json::to_string(&output).unwrap();
        assert_eq!(json, r#"{"files":[],"total_hunks":0}"#);
    }

    // --- StageRequest round-trip ---

    #[test]
    fn stage_request_hunk_ids_roundtrip() {
        let req = StageRequest {
            hunk_ids: vec!["abc".into(), "def".into()],
            line_selections: vec![],
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: StageRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn stage_request_line_selections_roundtrip() {
        let req = StageRequest {
            hunk_ids: vec![],
            line_selections: vec![LineSelection {
                hunk_id: "abc".into(),
                line_indices: vec![0, 2, 4],
            }],
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: StageRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn stage_request_defaults() {
        // Minimal JSON with just empty object should work via #[serde(default)]
        let req: StageRequest = serde_json::from_str("{}").unwrap();
        assert!(req.hunk_ids.is_empty());
        assert!(req.line_selections.is_empty());
    }

    // --- StageResult round-trip ---

    #[test]
    fn stage_result_success_roundtrip() {
        let result = StageResult {
            staged: 3,
            failed: 0,
            errors: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        // errors should be omitted when empty
        assert!(!json.contains("errors"));
        let back: StageResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, back);
    }

    #[test]
    fn stage_result_with_errors_roundtrip() {
        let result = StageResult {
            staged: 1,
            failed: 2,
            errors: vec!["hunk xyz not found".into(), "apply failed".into()],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("errors"));
        let back: StageResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, back);
    }

    // --- Response envelope ---

    #[test]
    fn response_success_roundtrip() {
        let resp = Response::success(DiffOutput {
            files: vec![],
            total_hunks: 0,
        });
        let json = serde_json::to_string(&resp).unwrap();
        // Should not contain "error" key
        assert!(!json.contains("\"error\""));
        let back: Response<DiffOutput> = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn response_error_roundtrip() {
        let resp = Response::<()>::error("repo not found");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"ok\":false"));
        assert!(!json.contains("\"data\""));
        let back: Response<()> = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn response_schema_shape() {
        let resp = Response::success(StageResult {
            staged: 1,
            failed: 0,
            errors: vec![],
        });
        let val: serde_json::Value = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["version"], 1);
        assert_eq!(val["ok"], true);
        assert_eq!(val["data"]["staged"], 1);
        assert!(val.get("error").is_none());
    }

    // --- ProtocolRequest ---

    #[test]
    fn protocol_request_diff_parse() {
        let json = r#"{"method": "diff", "params": {"file": "src/main.rs"}}"#;
        let req: ProtocolRequest = serde_json::from_str(json).unwrap();
        match req {
            ProtocolRequest::Diff { params } => {
                assert_eq!(params.file, Some("src/main.rs".into()));
            }
            _ => panic!("expected Diff variant"),
        }
    }

    #[test]
    fn protocol_request_diff_no_params() {
        let json = r#"{"method": "diff"}"#;
        let req: ProtocolRequest = serde_json::from_str(json).unwrap();
        match req {
            ProtocolRequest::Diff { params } => {
                assert_eq!(params.file, None);
            }
            _ => panic!("expected Diff variant"),
        }
    }

    #[test]
    fn protocol_request_stage_parse() {
        let json = r#"{"method": "stage", "params": {"hunk_ids": ["a1", "b2"]}}"#;
        let req: ProtocolRequest = serde_json::from_str(json).unwrap();
        match req {
            ProtocolRequest::Stage { params } => {
                assert_eq!(params.hunk_ids, vec!["a1", "b2"]);
            }
            _ => panic!("expected Stage variant"),
        }
    }

    #[test]
    fn protocol_request_status_parse() {
        let json = r#"{"method": "status"}"#;
        let req: ProtocolRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req, ProtocolRequest::Status);
    }

    #[test]
    fn protocol_request_unknown_method() {
        let json = r#"{"method": "unknown"}"#;
        let result = serde_json::from_str::<ProtocolRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn protocol_request_roundtrip() {
        let requests = vec![
            ProtocolRequest::Diff {
                params: DiffParams {
                    file: Some("test.rs".into()),
                },
            },
            ProtocolRequest::Stage {
                params: StageRequest {
                    hunk_ids: vec!["id1".into()],
                    line_selections: vec![],
                },
            },
            ProtocolRequest::Status,
        ];
        for req in &requests {
            let json = serde_json::to_string(req).unwrap();
            let back: ProtocolRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(*req, back);
        }
    }
}
