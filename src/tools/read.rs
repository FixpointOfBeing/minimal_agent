use std::io::{self, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tokio::fs::File;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader, Lines,
};

use crate::tools::{Call, Context, ToolOutput, ToolSpec};

const DEFAULT_MAX_LINES: usize = 2000;
const DEFAULT_MAX_BYTES: usize = 50 * 1024;
/// Files larger than this skip the up-front line count; `total_lines` is
/// reported as `null` instead of paying a second full-file scan.
const MAX_COUNT_BYTES: u64 = 10 * 1024 * 1024;

pub struct ReadTool {}

#[derive(Clone, Copy)]
enum Truncation {
    Lines,
    Bytes,
    /// The first selected line alone exceeded `DEFAULT_MAX_BYTES` and had to
    /// be cut. The remainder of that line is not reachable via `offset`.
    LineTooLong { total_bytes: usize, shown_bytes: usize },
    None,
}

struct Args {
    path: String,
    offset: usize,        // 1-indexed
    limit: Option<usize>, // None = DEFAULT_MAX_LINES
}

impl ReadTool {
    pub fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read",
            snippet: "Read file contents",
            description: "Read the contents of a text file. Output is truncated to 2000 lines or 50KB (whichever is hit first). Use offset/limit for large files. When you need the full file, continue with offset until complete.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read (relative or absolute)"
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "1-indexed line number to start reading from. Defaults to 1."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": DEFAULT_MAX_LINES,
                        "description": "Maximum number of lines to return. Capped at 2000."
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            guidelines: &["Use read to examine files instead of cat or sed"],
        }
    }

    pub async fn execute(&self, ctx: &Context, call: Call) -> ToolOutput {
        let args = match Args::parse(&call) {
            Ok(args) => args,
            Err(msg) => return ToolOutput::err(&call, msg),
        };

        let absolute = resolve_path(&ctx.cwd, &args.path);
        let mut file = match File::open(&absolute).await {
            Ok(file) => file,
            Err(error) => {
                return ToolOutput::err(
                    &call,
                    format!("read: cannot read {}: {error}", absolute.display()),
                );
            }
        };

        // fstat on the already-open handle: cheap, no path re-resolution.
        let size_bytes = match file.metadata().await {
            Ok(metadata) => metadata.len(),
            Err(error) => return io_error(&call, &absolute, error),
        };

        let total_lines = match count_lines(&mut file, size_bytes).await {
            Ok(n) => n,
            Err(error) => return io_error(&call, &absolute, error),
        };

        // count_lines leaves the cursor at EOF; rewind before streaming.
        if let Err(error) = file.seek(SeekFrom::Start(0)).await {
            return io_error(&call, &absolute, error);
        }
        let mut reader = BufReader::new(file).lines();

        let start = args.offset.saturating_sub(1);
        let max_lines = args.limit.unwrap_or(DEFAULT_MAX_LINES);

        let skipped = match skip_lines(&mut reader, start).await {
            Ok(n) => n,
            Err(error) => return io_error(&call, &absolute, error),
        };
        if skipped < start {
            // offset beyond EOF: not an error, just report it.
            return ToolOutput::ok(
                &call,
                json!({
                    "output": "",
                    "total_lines": total_lines,
                    "size_bytes": size_bytes,
                    "caveat": format!(
                        "[offset {} is beyond end of file (file has {skipped} lines).]",
                        args.offset
                    )
                }),
            );
        }

        let (kept, truncation) = match collect_lines(&mut reader, max_lines).await {
            Ok(result) => result,
            Err(error) => return io_error(&call, &absolute, error),
        };

        let start_display = start + 1;
        let end_display = start + kept.len();
        let output = kept.join("\n");

        let mut body = json!({
            "output": output,
            "total_lines": total_lines,
            "size_bytes": size_bytes,
        });
        if let Some(caveat) = format_caveat(
            start_display,
            end_display,
            total_lines,
            max_lines,
            truncation,
        ) {
            body["caveat"] = json!(caveat);
        }
        ToolOutput::ok(&call, body)
    }
}

impl Args {
    fn parse(call: &Call) -> Result<Args, String> {
        let path = call
            .arguments
            .get("path")
            .and_then(Value::as_str)
            .ok_or("read requires a string `path` argument")?
            .to_string();
        if path.is_empty() {
            return Err("read: `path` must not be empty".into());
        }

        let offset = call
            .arguments
            .get("offset")
            .and_then(Value::as_u64)
            .map(|v| v.max(1) as usize)
            .unwrap_or(1);

        let limit = match call.arguments.get("limit").and_then(Value::as_u64) {
            Some(0) => return Err("read: `limit` must be a positive integer".into()),
            Some(v) => Some((v as usize).min(DEFAULT_MAX_LINES)),
            None => None,
        };

        Ok(Args { path, offset, limit })
    }
}

fn resolve_path(cwd: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// Count lines without materialising the file. Uses raw bytes so invalid
/// UTF-8 doesn't fail here (the streaming reader will report that later).
/// `size_bytes` is passed in to avoid a redundant `metadata()` call.
/// Returns `None` for files above `MAX_COUNT_BYTES` to keep latency bounded.
async fn count_lines(file: &mut File, size_bytes: u64) -> io::Result<Option<usize>> {
    if size_bytes > MAX_COUNT_BYTES {
        return Ok(None);
    }
    if size_bytes == 0 {
        return Ok(Some(0));
    }

    let mut buf = vec![0u8; 64 * 1024];
    let mut newlines = 0usize;
    let mut last = 0u8;
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        newlines += buf[..n].iter().filter(|&&b| b == b'\n').count();
        last = buf[n - 1];
    }

    // "a\nb" => 1 newline + trailing line; "a\nb\n" => 2 newlines, no trailing.
    let trailing = usize::from(last != b'\n');
    Ok(Some(newlines + trailing))
}

/// Advance the reader by up to `count` lines. Returns the number actually
/// skipped — if it's less than `count`, the caller has hit EOF.
async fn skip_lines<R>(reader: &mut Lines<R>, count: usize) -> io::Result<usize>
where
    R: AsyncBufRead + Unpin,
{
    let mut skipped = 0;
    while skipped < count {
        match reader.next_line().await? {
            Some(_) => skipped += 1,
            None => break,
        }
    }
    Ok(skipped)
}

/// Pull up to `max_lines` lines, stopping early at `DEFAULT_MAX_BYTES`.
/// If the very first selected line alone exceeds the byte budget, truncate it
/// on a UTF-8 char boundary so the result can never blow past the cap.
async fn collect_lines<R>(
    reader: &mut Lines<R>,
    max_lines: usize,
) -> io::Result<(Vec<String>, Truncation)>
where
    R: AsyncBufRead + Unpin,
{
    let mut kept: Vec<String> = Vec::new();
    let mut bytes = 0usize;

    loop {
        if kept.len() >= max_lines {
            // Only advertise truncation if there really is more to read —
            // otherwise a file that ends exactly at the limit would send the
            // caller chasing an offset past EOF.
            return if reader.next_line().await?.is_some() {
                Ok((kept, Truncation::Lines))
            } else {
                Ok((kept, Truncation::None))
            };
        }

        let Some(line) = reader.next_line().await? else {
            return Ok((kept, Truncation::None));
        };

        let cost = line.len() + 1; // +1 for the newline separator

        // First selected line is bigger than the entire byte budget:
        // truncate it instead of keeping it whole. The rest of this line is
        // NOT reachable via `offset` (which is line-granular); the caveat
        // tells the caller to use a different tool.
        if kept.is_empty() && cost > DEFAULT_MAX_BYTES {
            let budget = DEFAULT_MAX_BYTES.saturating_sub(1); // leave room for '\n'
            let shown = truncate_to_char_boundary(&line, budget).to_string();
            let shown_bytes = shown.len();
            kept.push(shown);
            return Ok((
                kept,
                Truncation::LineTooLong {
                    total_bytes: line.len(),
                    shown_bytes,
                },
            ));
        }

        if !kept.is_empty() && bytes + cost > DEFAULT_MAX_BYTES {
            return Ok((kept, Truncation::Bytes));
        }

        bytes += cost;
        kept.push(line);
    }
}

/// Truncate `s` to at most `max_bytes` bytes, backing off to the nearest
/// UTF-8 char boundary so we never emit an invalid sequence.
fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn format_caveat(
    start_display: usize,
    end_display: usize,
    total_lines: Option<usize>,
    max_lines: usize,
    truncation: Truncation,
) -> Option<String> {
    let next_offset = end_display + 1;
    let total = total_lines.map(|n| format!(" of {n}")).unwrap_or_default();
    match truncation {
        Truncation::None => None,
        Truncation::Lines => Some(format!(
            "[Showing lines {start_display}-{end_display}{total} ({max_lines} line limit). Use offset={next_offset} to continue.]"
        )),
        Truncation::Bytes => Some(format!(
            "[Showing lines {start_display}-{end_display}{total} ({}KB limit). Use offset={next_offset} to continue.]",
            DEFAULT_MAX_BYTES / 1024
        )),
        Truncation::LineTooLong {
            total_bytes,
            shown_bytes,
        } => Some(format!(
            "[Line {start_display} is too long to display in full: showing first {shown_bytes} of {total_bytes} bytes. The rest of this line is not available via offset; use another tool (grep/head/a script) for it. Use offset={next_offset} to continue from the next line.]"
        )),
    }
}

fn io_error(call: &Call, path: &Path, error: io::Error) -> ToolOutput {
    if error.kind() == io::ErrorKind::InvalidData {
        ToolOutput::err(
            call,
            format!("read: {} is not valid UTF-8 text (binary file?)", path.display()),
        )
    } else {
        ToolOutput::err(call, format!("read: {}: {error}", path.display()))
    }
}
