use anyhow::Result;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;

use crate::git;
use crate::models::{ProtocolRequest, Response};

/// Maximum line length (10 MiB) to prevent OOM from unbounded input.
const MAX_LINE_LENGTH: usize = 10 * 1024 * 1024;

/// Write a JSON response line to stdout and flush.
/// Returns Err on write failure (broken pipe, etc.) to signal loop exit.
fn write_response<T: serde::Serialize>(
    out: &mut impl Write,
    resp: &Response<T>,
) -> io::Result<()> {
    serde_json::to_writer(&mut *out, resp).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    out.write_all(b"\n")?;
    out.flush()
}

/// Read a single line from the reader, respecting MAX_LINE_LENGTH.
/// Returns None on EOF, Some(Err) on read error, Some(Ok(bytes)) on success.
fn read_line_bytes(reader: &mut impl BufRead) -> Option<io::Result<Vec<u8>>> {
    let mut buf = Vec::new();
    match reader.take(MAX_LINE_LENGTH as u64).read_until(b'\n', &mut buf) {
        Ok(0) => None, // EOF
        Ok(_) => {
            // Strip trailing newline
            if buf.last() == Some(&b'\n') {
                buf.pop();
            }
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            Some(Ok(buf))
        }
        Err(e) => Some(Err(e)),
    }
}

/// Enter the JSON-lines protocol loop: read requests from stdin, write responses to stdout.
///
/// Each line on stdin is a JSON request with a `"method"` field.
/// Each response is a single JSON line on stdout.
/// All logs/warnings go to stderr only.
pub fn run_protocol(repo_path: &Path) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut out = BufWriter::new(stdout.lock());

    loop {
        let bytes = match read_line_bytes(&mut reader) {
            None => break, // EOF
            Some(Err(e)) => {
                // Read error — send error response and continue (don't kill session)
                eprintln!("[gitsift] stdin read error: {e}");
                let resp = Response::<()>::error(format!("stdin read error: {e}"));
                if write_response(&mut out, &resp).is_err() {
                    break;
                }
                continue;
            }
            Some(Ok(b)) => b,
        };

        // Skip empty lines
        if bytes.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }

        // Parse as UTF-8, return error response for invalid encoding
        let line = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                let resp = Response::<()>::error("invalid UTF-8 input");
                if write_response(&mut out, &resp).is_err() {
                    break;
                }
                continue;
            }
        };

        let request = match serde_json::from_str::<ProtocolRequest>(&line) {
            Ok(req) => req,
            Err(e) => {
                let resp = Response::<()>::error(format!("invalid request: {e}"));
                if write_response(&mut out, &resp).is_err() {
                    break;
                }
                continue;
            }
        };

        let write_ok = match request {
            ProtocolRequest::Diff { params } => {
                match git::diff::diff_unstaged(repo_path, params.file.as_deref()) {
                    Ok(diff) => write_response(&mut out, &Response::success(diff)),
                    Err(e) => write_response(&mut out, &Response::<()>::error(format!("{e}"))),
                }
            }
            ProtocolRequest::Stage { params } => {
                match git::stage::stage_selection(repo_path, &params) {
                    Ok(result) => write_response(&mut out, &Response::success(result)),
                    Err(e) => write_response(&mut out, &Response::<()>::error(format!("{e}"))),
                }
            }
            ProtocolRequest::Status => match git::status::get_status(repo_path) {
                Ok(status) => write_response(&mut out, &Response::success(status)),
                Err(e) => write_response(&mut out, &Response::<()>::error(format!("{e}"))),
            },
        };

        if write_ok.is_err() {
            break; // stdout broken, exit cleanly
        }
    }

    Ok(())
}
