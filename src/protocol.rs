use anyhow::Result;
use std::io::{self, BufRead, BufWriter, Write};
use std::path::Path;

use crate::git;
use crate::models::{ProtocolRequest, Response};

/// Write a JSON response line to stdout and flush.
fn write_response<T: serde::Serialize>(out: &mut impl Write, resp: &Response<T>) {
    // Errors writing to stdout are fatal — nothing we can do.
    let _ = serde_json::to_writer(&mut *out, resp);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

/// Enter the JSON-lines protocol loop: read requests from stdin, write responses to stdout.
///
/// Each line on stdin is a JSON request with a `"method"` field.
/// Each response is a single JSON line on stdout.
/// All logs/warnings go to stderr only.
pub fn run_protocol(repo_path: &Path) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[gitsift] stdin read error: {e}");
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let request = match serde_json::from_str::<ProtocolRequest>(&line) {
            Ok(req) => req,
            Err(e) => {
                let resp = Response::<()>::error(format!("invalid request: {e}"));
                write_response(&mut out, &resp);
                continue;
            }
        };

        match request {
            ProtocolRequest::Diff { params } => {
                match git::diff::diff_unstaged(repo_path, params.file.as_deref()) {
                    Ok(diff) => {
                        let resp = Response::success(diff);
                        write_response(&mut out, &resp);
                    }
                    Err(e) => {
                        let resp = Response::<()>::error(format!("{e}"));
                        write_response(&mut out, &resp);
                    }
                }
            }
            ProtocolRequest::Stage { params } => {
                match git::stage::stage_selection(repo_path, &params) {
                    Ok(result) => {
                        let resp = Response::success(result);
                        write_response(&mut out, &resp);
                    }
                    Err(e) => {
                        let resp = Response::<()>::error(format!("{e}"));
                        write_response(&mut out, &resp);
                    }
                }
            }
            ProtocolRequest::Status => match git::status::get_status(repo_path) {
                Ok(status) => {
                    let resp = Response::success(status);
                    write_response(&mut out, &resp);
                }
                Err(e) => {
                    let resp = Response::<()>::error(format!("{e}"));
                    write_response(&mut out, &resp);
                }
            },
        }
    }

    Ok(())
}
