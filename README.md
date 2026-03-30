# gitsift

Git hunk sifter for code agents. A selective staging and checkout tool (`git add -p` / `git checkout -p` replacement) designed for CLI agents like Claude Code and Codex.

## Features

- **Hunk-level staging** — stage entire hunks by ID
- **Line-level staging** — stage individual lines within a hunk via patch reconstruction
- **Hunk-level checkout** — discard unstaged or staged changes per hunk
- **Staged diff** — view staged changes (HEAD vs index) with `diff --staged`
- **Compact output** — token-efficient default format (~40% smaller than JSON), inspired by [TOON](https://toonformat.dev/)
- **JSON output** — full structured diff output with file/hunk/line metadata
- **JSON-lines protocol** — persistent stdin/stdout mode for agent sessions

## Install

### Shell script (Linux & macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/MonteYin/gitsift/main/install.sh | bash
```

Custom install directory:

```bash
INSTALL_DIR=~/.local/bin curl -fsSL https://raw.githubusercontent.com/MonteYin/gitsift/main/install.sh | bash
```

### Homebrew (macOS)

```bash
brew tap MonteYin/tap
brew install gitsift
```

### From source

```bash
cargo install --path .
```

## Usage

### CLI

```bash
# List unstaged changes (compact format, default)
gitsift diff

# List unstaged changes as JSON
gitsift diff --format json

# Filter by file
gitsift diff --file src/main.rs

# List staged changes (HEAD vs index)
gitsift diff --staged

# Stage hunks by ID
gitsift stage --hunk-ids abc123,def456

# Stage via JSON on stdin (supports line-level selections)
echo '{"hunk_ids": ["abc123"]}' | gitsift stage --from-stdin

# Discard unstaged hunks (revert workdir → index)
gitsift checkout --hunk-ids abc123,def456

# Discard staged hunks (revert index → HEAD)
gitsift checkout --staged --hunk-ids abc123

# Discard via JSON on stdin
echo '{"hunk_ids": ["abc123"]}' | gitsift checkout --from-stdin

# Show staging status
gitsift status
```

### JSON-lines protocol

For persistent agent sessions, use protocol mode:

```bash
gitsift protocol
```

Send JSON requests on stdin, receive JSON responses on stdout:

```json
{"method": "diff", "params": {"file": "src/main.rs"}}
{"method": "diff", "params": {"staged": true}}
{"method": "stage", "params": {"hunk_ids": ["abc123"]}}
{"method": "checkout", "params": {"hunk_ids": ["abc123"]}}
{"method": "checkout", "params": {"hunk_ids": ["abc123"], "staged": true}}
{"method": "status"}
```

Each response is a single JSON line:

```json
{"version": 1, "ok": true, "data": {"files": [...], "total_hunks": 2}}
```

### Output formats

Default is `toon` (compact). Use `--format json` for full structured JSON.

The compact format is inspired by [TOON (Token-Oriented Object Notation)](https://toonformat.dev/) and uses ~40% fewer tokens than JSON by:
- Stripping context lines (unchanged lines) — agents only need change lines for staging decisions
- Removing redundant `file_path` from hunks (already in parent file entry)
- Using tabular rows for line arrays: schema header `{tag,content,old,new}:` declared once, values as CSV rows

Example compact output:
```
version: 1
ok: true
total_hunks: 1
files[1]:
  - path: src/main.rs
    status: modified
    hunks[1]:
      - id: 59a9050fd4195c94
        header: @@ -1,5 +1,7 @@
        old_start: 1 old_lines: 5 new_start: 1 new_lines: 7
        lines[2]{tag,content,old,new}:
          -,old line\n,2,
          +,new line\n,,2
```

## Agent integration

gitsift is designed for the following workflows:

**Selective staging:**
1. Agent calls `gitsift diff` to inspect unstaged changes
2. Agent selects hunks/lines to stage based on the structured output
3. Agent calls `gitsift stage --hunk-ids <ids>` or pipes a `StageRequest` via `--from-stdin`
4. Agent calls `gitsift status` to verify staging result

**Selective discard:**
1. Agent calls `gitsift diff` (or `gitsift diff --staged`) to inspect changes
2. Agent selects hunks to discard
3. Agent calls `gitsift checkout --hunk-ids <ids>` (or `--staged` for staged changes)
4. Agent calls `gitsift diff` to verify the changes were discarded

For persistent sessions, use `gitsift protocol` to avoid process startup overhead.

## Architecture

```
src/
├── main.rs          # CLI entry point (clap)
├── cli.rs           # Clap structs: Cli, Commands, OutputFormat
├── models.rs        # Serde types: Hunk, HunkLine, DiffOutput, StageRequest, Response<T>
├── git/
│   ├── mod.rs       # shared git2 helpers (diff_opts, delta_path, hunk_header, etc.)
│   ├── diff.rs      # diff engine: diff_unstaged (index→workdir), diff_staged (HEAD→index)
│   ├── stage.rs     # staging: hunk-level via ApplyOptions, line-level via patch reconstruction
│   ├── checkout.rs  # checkout (discard): hunk-level reverse-patch for unstaged and staged changes
│   └── status.rs    # staging status summary
├── protocol.rs      # stdin/stdout JSON-lines request/response loop
├── toon.rs          # compact output format (TOON-inspired, token-efficient)
└── output.rs        # format dispatch: compact vs JSON
```

## License

MIT
