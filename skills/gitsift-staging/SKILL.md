---
name: gitsift-staging
description: "Selective git staging with gitsift — stage specific hunks or individual lines instead of entire files. Use this skill whenever the user needs to split changes into multiple commits, stage only part of their work, create atomic commits from a large diff, cherry-pick specific changes to stage, or do anything that resembles `git add -p` but with structured control. Trigger phrases include: 'stage only the bug fix', 'commit these separately', 'only stage lines X-Y', 'split this into two commits', 'don't stage everything', 'only commit the tests', 'separate the formatting from the logic', 'partial staging', 'I want to pick which changes to commit', 'selective commit'."
user_invocable: true
---

# Selective Staging with gitsift

gitsift is a CLI tool that replaces `git add -p` with structured JSON output. It lets you stage individual hunks or even specific lines from unstaged changes — perfect for creating clean, atomic commits.

## Before You Start

Run `gitsift --version` to confirm it's installed. If not, see https://github.com/MonteYin/gitsift#install for setup.

## Core Workflow

The workflow is always: **diff → decide → stage → verify → commit**.

### 1. See what changed

```bash
gitsift diff --format json
```

This returns all unstaged changes as structured JSON. Each change is organized as files → hunks → lines, and every hunk has a unique ID you'll use for staging.

To focus on a specific file:
```bash
gitsift diff --format json --file src/main.rs
```

For a quick human-readable overview (useful to show the user):
```bash
gitsift diff --format human
```

### 2. Decide what to stage

Look at the diff output and identify which hunks or lines belong together logically. Think about what makes a clean, atomic commit — group related changes together.

The JSON structure looks like this:
```json
{
  "version": 1, "ok": true,
  "data": {
    "files": [{
      "path": "src/lib.rs",
      "status": "modified",
      "hunks": [{
        "id": "59a9050fd4195c94",
        "file_path": "src/lib.rs",
        "old_start": 1, "old_lines": 5,
        "new_start": 1, "new_lines": 7,
        "header": "@@ -1,5 +1,7 @@",
        "lines": [
          {"tag": "equal",  "content": "unchanged line\n", "old_lineno": 1, "new_lineno": 1},
          {"tag": "delete", "content": "old line\n",       "old_lineno": 2},
          {"tag": "insert", "content": "new line\n",       "new_lineno": 2}
        ]
      }]
    }],
    "total_hunks": 1
  }
}
```

Note: `content` contains the raw line text without any diff prefix characters (no `+`, `-`, or space prefix).

### 3. Stage by hunk

When you want to stage entire hunks:
```bash
gitsift stage --hunk-ids 59a9050fd4195c94
```

Multiple hunks at once:
```bash
gitsift stage --hunk-ids abc123,def456,ghi789
```

### 4. Stage by line (fine-grained)

When you need to stage only specific lines within a hunk, pipe a JSON request to `--from-stdin`:
```bash
echo '{"line_selections": [{"hunk_id": "59a9050fd4195c94", "line_indices": [1, 2]}]}' | gitsift stage --from-stdin
```

`line_indices` are 0-based positions in the hunk's `lines` array. Select the `delete` and `insert` lines you want — context (`equal`) lines are handled automatically.

**Important**: you cannot mix `hunk_ids` and `line_selections` in one request. If you need both, make two separate calls.

### 5. Verify and commit

Check what's staged vs unstaged:
```bash
gitsift status --format json
```

Then commit as usual:
```bash
git commit -m "your message"
```

If there are more changes to stage for a second commit, go back to step 1 — you need to re-diff because hunk IDs change after staging.

## Protocol Mode (persistent sessions)

For long-running agent sessions, use `gitsift protocol` to avoid process startup overhead. Send JSON requests on stdin, receive JSON responses on stdout:

```bash
gitsift protocol --repo .
```

```json
{"method": "diff", "params": {"file": "src/main.rs"}}
{"method": "stage", "params": {"hunk_ids": ["abc123"]}}
{"method": "status"}
```

Each response is a single JSON line with the same `Response` envelope. Errors (invalid JSON, unknown method) return `{"ok": false, "error": "..."}` without crashing the process.

## Gotchas

**New/untracked files break staging — even for other files.** If the diff contains any untracked files (status: `added`), `gitsift stage` will fail for ALL hunks — not just the untracked ones. The error is "index does not contain". The fix: always `git add` new/untracked files first before running `gitsift stage`. Recommended pattern:
```bash
# 1. Check for untracked files in diff
gitsift diff --format json | python3 -c "import sys,json; [print(f['path']) for f in json.load(sys.stdin)['data']['files'] if f['status']=='added']"
# 2. git add any untracked files first
git add <new-files>
# 3. Now gitsift stage works for the remaining tracked-file hunks
gitsift stage --hunk-ids <id>
```

**Changes close together merge into one hunk.** If your modifications are within 3 lines of each other, git combines them into a single hunk. You can't split them with hunk-level staging — use line-level staging (`--from-stdin` with `line_selections`) instead. Check the hunk's `lines` array to pick exactly which delete/insert pairs to include.

**Re-diff after every stage.** Once you stage something, the remaining hunks shift and their IDs change. Always run `gitsift diff` again before the next `gitsift stage`. Using stale IDs will fail with "hunk ID not found".

**One mode per request.** Either `hunk_ids` or `line_selections`, never both at once. The API rejects mixed requests — use separate calls if you need both.

**Line indices are 0-based** into the hunk's `lines` array. Look at the `tag` field to identify which lines are changes (`delete`/`insert`) vs context (`equal`). Only selecting context lines will be rejected.

## Example: Splitting a Feature and a Bugfix

Say you modified `app.rs` and it has 3 hunks: a bugfix (hunk 1), a new feature (hunks 2 and 3).

```bash
# 1. See all changes
gitsift diff --format json --file app.rs
# → 3 hunks with IDs: aaa111, bbb222, ccc333

# 2. Stage just the bugfix
gitsift stage --hunk-ids aaa111
git commit -m "fix: resolve null pointer in error handler"

# 3. Re-diff for the feature (IDs may have changed!)
gitsift diff --format json --file app.rs
# → 2 hunks with IDs: ddd444, eee555

# 4. Stage the feature
gitsift stage --hunk-ids ddd444,eee555
git commit -m "feat: add retry logic for API calls"
```

## Response Format

All gitsift JSON responses share this envelope:
```json
{"version": 1, "ok": true, "data": { ... }}
```

On error:
```json
{"version": 1, "ok": false, "error": "description of what went wrong"}
```

Stage results include counts:
```json
{"version": 1, "ok": true, "data": {"staged": 2, "failed": 0}}
```

If some IDs were invalid:
```json
{"version": 1, "ok": true, "data": {"staged": 1, "failed": 1, "errors": ["hunk ID not found: badid"]}}
```
