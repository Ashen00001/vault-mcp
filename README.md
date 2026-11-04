# vault-mcp

A small, **read-only** [MCP](https://modelcontextprotocol.io) server (stdio) written in Rust that lets Claude find and read exactly the part of an old Claude Code conversation it needs, in one cheap tool call.

## Why

If you keep an archive of past sessions (a short summary note per session, a readable transcript, and a raw JSONL copy), the usual way to dig something out is to grep big folders and read whole files. That burns tokens and context. `vault-mcp` does the same job in three steps with hard size limits:

1. `search_sessions`: rank the short summary notes (about 1 KB of output for 5 hits).
2. `get_session`: read one summary, or only the sections you want.
3. `grep_transcript`: regex search inside **one** transcript and get numbered snippets with context, including tool calls and tool output from the raw JSONL.

In a measurement on a real archive of about 117 sessions, a search plus a transcript grep returned about 4.4 KB, while reading the whole readable transcript of that one session was about 19 KB (4x). The gap grows with session length, and the raw JSONL is orders of magnitude larger (the biggest raw transcript there is over 100 MB, and a grep over it still returns under 3 KB).

## Tools

| Tool | Inputs (defaults) | Returns |
|---|---|---|
| `search_sessions` | `query`, `limit=5` (max 25), `since?` (`YYYY-MM-DD`), `cwd_contains?` | Per hit: `session_id`, `date`, `title`, 1 line `snippet`, which fields `matched`, `score`; plus `total_matches` and `truncated` |
| `get_session` | `session_id` (full or unique prefix), `sections?`, `max_chars=6000` | Summary content (all or chosen sections), `readable_transcript` and `raw_jsonl` paths (or `null`), `truncated` |
| `grep_transcript` | `session_id`, `pattern`, `source="readable"\|"raw"`, `literal=false`, `ignore_case=true`, `context_lines=2`, `max_matches=10`, `max_chars=4000` | Numbered snippets (`N:` match, `N-` context); for `raw` also `role`, `kind`, `tool`, `ts`; `count`, `truncated` |
| `recent_sessions` | `limit=10` (max 50) | Newest first: id, date, title, cwd |
| `search_vault` | `query`, `limit=5` | Ranked vault notes (not the chat/session archives): title, relative path, snippet |
| `list_projects` | `max_chars=4000` | The vault's `Projects/Code Index.md`, capped |

Example calls (fake data):

```json
{"name":"search_sessions","arguments":{"query":"gizmo compiler","limit":3,"since":"2026-01-01"}}
```
```json
{"results":[{"session_id":"aaaa1111-0000-4000-8000-000000000001","date":"2026-03-02","title":"Gizmo compiler build","snippet":"build a gizmo compiler in rust","matched":["title","first_ask"],"score":9.31}],"total_matches":1,"truncated":false,"notice":"..."}
```
```json
{"name":"grep_transcript","arguments":{"session_id":"aaaa1111","pattern":"E0425","source":"raw","context_lines":1}}
```
```json
{"matches":[{"line":3,"role":"user","kind":"tool_result","tool":"Bash","text":"1:error[E0425]: cannot find value `frobnicate`"}],"count":1,"truncated":false,"source":"raw"}
```

Every limit has a clamp, every list result has a `truncated` flag, and the whole response is capped (default 12,000 characters; `--max-response-chars`). Trailing results are dropped first.

## Expected layout

```
<vault>/Session Summaries/*.md     summary notes with frontmatter
<vault>/Claude Code Chats/*.md     readable transcripts
<archive>/<date>_<session_id>.jsonl  raw transcripts (optional per session)
```

A summary note has frontmatter keys `title`, `created`, `session_id`, `cwd`, `transcript_file`, `raw_jsonl`, and sections `## First ask`, `## Where it ended`, `## What changed` (with bold `Files…`, `Commits…`, `Repos…` groups), `## Tools used`. See `tests/e2e.rs` for a complete fake example.

## Install

```
cargo build --release
```

The binary is `target/release/vault-mcp` (`vault-mcp.exe` on Windows). Needs Rust 1.82 or newer. Only two crates are used at runtime: `serde_json` and `regex`; the MCP/JSON-RPC layer is hand-written.

Configuration (flag, then environment variable, then default):

| Flag | Env | Default |
|---|---|---|
| `--vault DIR` | `VAULT_MCP_VAULT` | `~/Downloads/obsidian-vault` |
| `--archive DIR` | `VAULT_MCP_ARCHIVE` | `D:/Archive/claude-transcripts` on Windows, `~/Archive/claude-transcripts` elsewhere |
| `--max-response-chars N` | | `12000` |

## Register in Claude Code

```
claude mcp add vault-mcp -- /path/to/vault-mcp
```

Add `-e VAULT_MCP_VAULT=... -e VAULT_MCP_ARCHIVE=...` before the `--` if your folders are not the defaults. The tools then show up as `mcp__vault-mcp__search_sessions` and so on.

## Security model

- **Read-only.** The server never writes, deletes or executes anything and makes no network calls. Its only I/O is reading files and talking JSON-RPC on stdin/stdout.
- **Allowlisted roots.** Reads are limited to the vault folder and the archive folder. Every path (including ones taken from a note's frontmatter) is canonicalized, so symlinks are resolved, then must be an existing regular file inside a root. Any `..` component is rejected before that. Directory walks never follow symlinks. A note that points outside the roots simply gets no transcript path.
- **Session ids are validated** (letters, digits, `-`), and tool inputs are typed and clamped.
- **Untrusted content.** Transcripts and notes can contain anything, including text written to look like instructions. Every response carries a `notice` saying so, and archive text is only ever returned inside JSON string fields (`snippet`, `content`, `text`). Clients should still treat it as data.
- Regex search uses the `regex` crate (no backtracking, bounded program size), so a hostile pattern can't hang the server.

## Limitations

- Search covers the summary notes only. If a term never made it into a summary (title, first ask, final message, files, commits, repos), use `grep_transcript` on a candidate session instead.
- Titles are whatever the summary hook wrote, usually the first message, so they can be long or noisy.
- Ranking is a weighted per-field BM25 with prefix matching. There are no embeddings and no stemming beyond prefixes.
- The index is rebuilt in memory per process and refreshed by mtime and size on each call; very large vaults make the first `search_vault` slower (a few hundred ms for a few thousand notes).
- `grep_transcript` reads a whole file per call (readable transcripts are capped at 64 MB). Raw JSONL is streamed, so large files are fine.
- Windows paths in frontmatter are used as written; forward slashes are recommended.

## Development

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Tests use only made-up fixtures generated in temp directories, and include an in-process end-to-end test that drives the stdio loop with real JSON-RPC messages.

## License

MIT
