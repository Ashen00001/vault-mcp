use crate::frontmatter;
use crate::search::Doc;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Folders never indexed by `search_vault` (case-insensitive, any depth).
pub const VAULT_EXCLUDED: &[&str] = &[
    "claude chats",
    "chatgpt chats",
    "claude code chats",
    "agent chats",
    "sessions",
    "session summaries",
    ".obsidian",
    ".git",
    ".trash",
];

const MAX_NOTE_BYTES: u64 = 256 * 1024;

pub struct Summary {
    pub path: PathBuf,
    pub session_id: String,
    pub title: String,
    pub created: String,
    pub date: String,
    pub cwd: String,
    pub transcript_file: String,
    pub raw_jsonl: String,
    pub sections: Vec<(String, String)>,
    pub first_ask: String,
    pub ended: String,
    pub files: String,
    pub commits: String,
    pub repos: String,
    pub doc: Doc,
}

impl Summary {
    pub fn field_text(&self, name: &str) -> &str {
        match name {
            "title" => &self.title,
            "first_ask" => &self.first_ask,
            "ended" => &self.ended,
            "files" => &self.files,
            "commits" => &self.commits,
            "repos" => &self.repos,
            _ => "",
        }
    }

    pub fn section(&self, wanted: &str) -> Option<&str> {
        let w = wanted.trim().to_lowercase();
        self.sections
            .iter()
            .find(|(n, _)| n.to_lowercase() == w)
            .map(|(_, t)| t.as_str())
    }
}

fn strip_quotes(text: &str) -> String {
    text.lines()
        .map(|l| {
            l.strip_prefix("> ")
                .unwrap_or(l.strip_prefix('>').unwrap_or(l))
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

pub fn parse_summary(path: &Path, src: &str) -> Option<Summary> {
    let fm = frontmatter::parse(src);
    let get = |k: &str| fm.fields.get(k).cloned().unwrap_or_default();
    let session_id = get("session_id");
    if session_id.is_empty() {
        return None;
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = Some(get("title")).filter(|t| !t.is_empty()).unwrap_or(stem);
    let created = get("created");
    let date = created.chars().take(10).collect::<String>();

    let mut sections: Vec<(String, String)> = Vec::new();
    for line in fm.body.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            sections.push((name.trim().to_string(), String::new()));
        } else if let Some((_, text)) = sections.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    for (_, t) in sections.iter_mut() {
        *t = t.trim().to_string();
    }
    let sec = |n: &str| {
        sections
            .iter()
            .find(|(s, _)| s.eq_ignore_ascii_case(n))
            .map(|(_, t)| t.as_str())
            .unwrap_or("")
    };
    let first_ask = strip_quotes(sec("First ask"));
    let ended = strip_quotes(sec("Where it ended"));

    let (mut files, mut commits, mut repos) = (String::new(), String::new(), String::new());
    let mut group = 0u8;
    for line in sec("What changed").lines() {
        let t = line.trim();
        if t.starts_with("**") {
            let low = t.to_lowercase();
            group = if low.contains("file") {
                1
            } else if low.contains("commit") {
                2
            } else if low.contains("repo") {
                3
            } else {
                0
            };
            continue;
        }
        let dst = match group {
            1 => &mut files,
            2 => &mut commits,
            3 => &mut repos,
            _ => continue,
        };
        dst.push_str(t);
        dst.push('\n');
    }

    let doc = Doc::new(&[
        ("title", 3.0, &title),
        ("first_ask", 2.0, &first_ask),
        ("ended", 1.5, &ended),
        ("files", 1.0, &files),
        ("commits", 2.0, &commits),
        ("repos", 2.0, &repos),
    ]);
    Some(Summary {
        path: path.to_path_buf(),
        session_id,
        title,
        created,
        date,
        cwd: get("cwd"),
        transcript_file: get("transcript_file"),
        raw_jsonl: get("raw_jsonl"),
        sections,
        first_ask,
        ended,
        files,
        commits,
        repos,
        doc,
    })
}

pub struct VaultNote {
    pub rel: String,
    pub title: String,
    pub text: String,
    pub doc: Doc,
}

pub fn parse_note(root: &Path, path: &Path, src: &str) -> VaultNote {
    let fm = frontmatter::parse(src);
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let heading = fm
        .body
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|h| h.trim().to_string());
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = fm
        .fields
        .get("title")
        .cloned()
        .filter(|t| !t.is_empty())
        .or(heading)
        .unwrap_or(stem);
    let text = fm.body.to_string();
    let doc = Doc::new(&[
        ("title", 3.0, &title),
        ("path", 2.0, &rel),
        ("body", 1.0, &text),
    ]);
    VaultNote {
        rel,
        title,
        text,
        doc,
    }
}

/// `.md` files under `dir`. Symlinks are never followed. Directories whose
/// lowercase name is in `excluded` are skipped.
pub fn list_md(dir: &Path, recursive: bool, excluded: &[&str]) -> Vec<(PathBuf, SystemTime, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                let name = e.file_name().to_string_lossy().to_lowercase();
                if recursive && !excluded.contains(&name.as_str()) {
                    stack.push(p);
                }
            } else if ft.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md")) {
                if let Ok(m) = e.metadata() {
                    out.push((p, m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Parse cache keyed by path; a file is re-parsed only when its mtime or size changed.
pub struct Scanner<T> {
    cache: HashMap<PathBuf, (SystemTime, u64, Option<T>)>,
}

impl<T> Default for Scanner<T> {
    fn default() -> Self {
        Scanner {
            cache: HashMap::new(),
        }
    }
}

impl<T> Scanner<T> {
    /// Sync the cache with the given file list; returns how many files were (re)parsed.
    pub fn refresh(
        &mut self,
        files: Vec<(PathBuf, SystemTime, u64)>,
        parse: impl Fn(&Path) -> Option<T>,
    ) -> usize {
        let mut reparsed = 0;
        let live: std::collections::HashSet<&PathBuf> = files.iter().map(|f| &f.0).collect();
        self.cache.retain(|k, _| live.contains(k));
        for (p, m, len) in &files {
            let fresh = matches!(self.cache.get(p), Some((cm, cl, _)) if cm == m && cl == len);
            if !fresh {
                let parsed = if *len <= MAX_NOTE_BYTES * 8 {
                    parse(p)
                } else {
                    None
                };
                self.cache.insert(p.clone(), (*m, *len, parsed));
                reparsed += 1;
            }
        }
        reparsed
    }

    pub fn items(&self) -> Vec<&T> {
        self.cache
            .values()
            .filter_map(|(_, _, t)| t.as_ref())
            .collect()
    }
}

pub fn read_capped(path: &Path, cap: u64) -> Option<String> {
    use std::io::Read;
    let f = fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.take(cap).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

pub fn read_note_text(path: &Path) -> Option<String> {
    read_capped(path, MAX_NOTE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const SAMPLE: &str = "---\ntitle: \"Build the widget\"\ncreated: 2026-03-02 10:00 UTC\nsession_id: aaaa1111-0000-4000-8000-000000000001\ncwd: /tmp/demo\ntranscript_file: \"/tmp/demo/chat.md\"\nraw_jsonl: \"\"\n---\n\n# Build the widget\n\n> link line\n\n## First ask\n\n> please build a widget\n> with two gears\n\n## Where it ended\n\n> Done, shipped v1.\n\n## What changed\n\n**Files written or edited (2):**\n- src/gear.rs\n- README.md\n\n**Commits (1):**\n- add gears\n\n**Repos created (1):**\n- demo-org/widget\n\n## Tools used\n\n- Bash x3\n";

    #[test]
    fn parses_summary_fields_and_sections() {
        let s = parse_summary(Path::new("2026-03-02 widget.md"), SAMPLE).unwrap();
        assert_eq!(s.title, "Build the widget");
        assert_eq!(s.date, "2026-03-02");
        assert_eq!(s.first_ask, "please build a widget\nwith two gears");
        assert_eq!(s.ended, "Done, shipped v1.");
        assert!(s.files.contains("src/gear.rs"));
        assert_eq!(s.commits.trim(), "- add gears");
        assert_eq!(s.repos.trim(), "- demo-org/widget");
        assert_eq!(s.sections.len(), 4);
        assert_eq!(s.section("tools used"), Some("- Bash x3"));
        assert_eq!(s.raw_jsonl, "");
    }

    #[test]
    fn rejects_notes_without_session_id() {
        assert!(parse_summary(Path::new("x.md"), "---\ntitle: t\n---\nbody").is_none());
    }

    #[test]
    fn note_title_falls_back_to_heading_then_stem() {
        let root = Path::new("/v");
        let n = parse_note(root, Path::new("/v/Projects/a.md"), "# Real Title\ntext");
        assert_eq!(n.title, "Real Title");
        assert_eq!(n.rel, "Projects/a.md");
        let m = parse_note(root, Path::new("/v/b.md"), "no heading");
        assert_eq!(m.title, "b");
    }

    #[test]
    fn list_md_skips_excluded_and_non_md() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("Claude Code Chats")).unwrap();
        fs::create_dir_all(d.path().join("Projects/sessions")).unwrap();
        fs::create_dir_all(d.path().join("Projects/keep")).unwrap();
        for f in [
            "Claude Code Chats/x.md",
            "Projects/sessions/y.md",
            "Projects/keep/z.md",
            "top.md",
            "image.png",
        ] {
            fs::write(d.path().join(f), "x").unwrap();
        }
        let got: Vec<String> = list_md(d.path(), true, VAULT_EXCLUDED)
            .iter()
            .map(|f| f.0.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(got, vec!["z.md".to_string(), "top.md".to_string()]);
    }

    #[test]
    fn scanner_reparses_only_changed() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.md");
        fs::write(&p, "one").unwrap();
        let mut sc: Scanner<String> = Scanner::default();
        let parse = |p: &Path| fs::read_to_string(p).ok();
        assert_eq!(sc.refresh(list_md(d.path(), false, &[]), parse), 1);
        assert_eq!(sc.refresh(list_md(d.path(), false, &[]), parse), 0);
        fs::write(&p, "two!").unwrap();
        assert_eq!(sc.refresh(list_md(d.path(), false, &[]), parse), 1);
        assert_eq!(sc.items(), vec![&"two!".to_string()]);
        fs::remove_file(&p).unwrap();
        sc.refresh(list_md(d.path(), false, &[]), parse);
        assert!(sc.items().is_empty());
    }
}
