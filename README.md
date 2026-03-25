# gitsift

Git hunk sifter for code agents. A selective staging tool (`git add -p` replacement) designed for CLI agents like Claude Code and Codex.

## Features

- **Hunk-level staging** — stage entire hunks by ID
- **Line-level staging** — stage individual lines within a hunk via patch reconstruction
- **JSON output** — structured diff output with file/hunk/line metadata for machine consumption
- **JSON-lines protocol** — persistent stdin/stdout mode for agent sessions
- **Human output** — readable unified diff with hunk IDs for quick reference

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
# List unstaged changes as JSON
gitsift diff --format json

# List unstaged changes (human-readable)
gitsift diff --format human

# Filter by file
gitsift diff --file src/main.rs

# Stage hunks by ID
gitsift stage --hunk-ids abc123,def456

# Stage via JSON on stdin (supports line-level selections)
echo '{"hunk_ids": ["abc123"]}' | gitsift stage --from-stdin

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
{"method": "stage", "params": {"hunk_ids": ["abc123"]}}
{"method": "status"}
```

Each response is a single JSON line:

```json
{"version": 1, "ok": true, "data": {"files": [...], "total_hunks": 2}}
```

### Output format

Output format is auto-detected: JSON when piped, human-readable in a terminal. Override with `--format json` or `--format human`.

## Agent integration

gitsift is designed for the following workflow:

1. Agent calls `gitsift diff --format json` to inspect available changes
2. Agent selects hunks/lines to stage based on the structured output
3. Agent calls `gitsift stage --hunk-ids <ids>` or pipes a `StageRequest` via `--from-stdin`
4. Agent calls `gitsift status` to verify staging result

For persistent sessions, use `gitsift protocol` to avoid process startup overhead.

## Architecture

```
src/
├── main.rs          # CLI entry point (clap)
├── cli.rs           # Clap structs: Cli, Commands, GlobalArgs
├── models.rs        # Serde types: Hunk, HunkLine, DiffOutput, StageRequest, Response<T>
├── git/
│   ├── diff.rs      # diff engine: git2 diff_index_to_workdir → Vec<Hunk>
│   ├── stage.rs     # staging: hunk-level via ApplyOptions, line-level via patch reconstruction
│   └── status.rs    # staging status summary
├── protocol.rs      # stdin/stdout JSON-lines request/response loop
└── output.rs        # format dispatch: JSON vs human-readable
```

## License

MIT
