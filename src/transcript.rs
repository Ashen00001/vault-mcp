use crate::text::{char_index, clip_around};
use regex::{Regex, RegexBuilder};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

const MAX_LINE_CHARS: usize = 300;
const MAX_CONTEXT_CHARS: usize = 200;
const MAX_READABLE_BYTES: u64 = 64 * 1024 * 1024;

pub struct GrepOpts {
    pub pattern: String,
    pub literal: bool,
    pub ignore_case: bool,
    pub context: usize,
    pub max_matches: usize,
    pub max_chars: usize,
}

pub struct Match {
    /// File line (readable) or JSONL line (raw) the match was found on.
    pub line: usize,
    pub role: Option<String>,
    pub tool: Option<String>,
    pub kind: Option<&'static str>,
    pub ts: Option<String>,
    pub text: String,
}

pub struct GrepResult {
    pub matches: Vec<Match>,
    pub truncated: bool,
}

pub fn build_regex(o: &GrepOpts) -> Result<Regex, String> {
    let pat = if o.literal {
        regex::escape(&o.pattern)
    } else {
        o.pattern.clone()
    };
    if pat.is_empty() {
        return Err("pattern must not be empty".into());
    }
    RegexBuilder::new(&pat)
        .case_insensitive(o.ignore_case)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| format!("bad pattern: {e}"))
}

#[derive(Default, Clone)]
struct Meta {
    line: usize,
    role: Option<String>,
    tool: Option<String>,
    kind: Option<&'static str>,
    ts: Option<String>,
}

struct Collector<'a> {
    re: &'a Regex,
    ctx: usize,
    max_matches: usize,
    max_chars: usize,
    used: usize,
    out: Vec<Match>,
    truncated: bool,
}

impl Collector<'_> {
    /// Add match windows from `text`; returns false once the limits are hit.
    fn feed(&mut self, meta: &Meta, text: &str, number_from: usize) -> bool {
        let lines: Vec<&str> = text.lines().collect();
        let hits: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| self.re.is_match(l))
            .map(|(i, _)| i)
            .collect();
        let mut i = 0;
        while i < hits.len() {
            let first = hits[i];
            let start = first.saturating_sub(self.ctx);
            let mut end = (first + self.ctx).min(lines.len() - 1);
            i += 1;
            while i < hits.len() && hits[i].saturating_sub(self.ctx) <= end + 1 {
                end = (hits[i] + self.ctx).min(lines.len() - 1);
                i += 1;
            }
            let mut block = String::new();
            for (n, line) in lines.iter().enumerate().take(end + 1).skip(start) {
                let num = number_from + n;
                match self.re.find(line) {
                    Some(m) => {
                        let c = char_index(line, m.start());
                        block
                            .push_str(&format!("{num}:{}\n", clip_around(line, c, MAX_LINE_CHARS)));
                    }
                    None => block.push_str(&format!(
                        "{num}-{}\n",
                        clip_around(line, 0, MAX_CONTEXT_CHARS)
                    )),
                }
            }
            let block = block.trim_end().to_string();
            if self.out.len() >= self.max_matches {
                self.truncated = true;
                return false;
            }
            let remaining = self.max_chars.saturating_sub(self.used);
            if block.chars().count() > remaining {
                self.truncated = true;
                if self.out.is_empty() {
                    let cut: String = block.chars().take(self.max_chars).collect();
                    self.push(meta, number_from + first, cut);
                }
                return false;
            }
            self.used += block.chars().count();
            self.push(meta, number_from + first, block);
        }
        true
    }

    fn push(&mut self, meta: &Meta, line: usize, text: String) {
        let mut m = Match {
            line,
            role: meta.role.clone(),
            tool: meta.tool.clone(),
            kind: meta.kind,
            ts: meta.ts.clone(),
            text,
        };
        if meta.line != 0 {
            m.line = meta.line;
        }
        self.out.push(m);
    }
}

fn collector<'a>(re: &'a Regex, o: &GrepOpts) -> Collector<'a> {
    Collector {
        re,
        ctx: o.context,
        max_matches: o.max_matches,
        max_chars: o.max_chars,
        used: 0,
        out: Vec::new(),
        truncated: false,
    }
}

pub fn grep_readable(path: &Path, o: &GrepOpts) -> Result<GrepResult, String> {
    use std::io::Read;
    let re = build_regex(o)?;
    let f = std::fs::File::open(path).map_err(|e| format!("cannot open transcript: {e}"))?;
    let mut buf = Vec::new();
    f.take(MAX_READABLE_BYTES)
        .read_to_end(&mut buf)
        .map_err(|e| format!("cannot read transcript: {e}"))?;
    let text = String::from_utf8_lossy(&buf);
    let mut c = collector(&re, o);
    c.feed(&Meta::default(), &text, 1);
    Ok(GrepResult {
        matches: c.out,
        truncated: c.truncated,
    })
}

/// One searchable piece of a raw JSONL record.
#[derive(Debug, PartialEq)]
pub struct Unit {
    pub role: String,
    pub kind: &'static str,
    pub tool: Option<String>,
    pub text: String,
}

fn render_tool_input(input: &Value) -> String {
    match input {
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| match v {
                Value::String(s) => format!("{k}: {s}"),
                other => format!("{k}: {other}"),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|b| match b.get("type").and_then(Value::as_str) {
                Some("text") => b.get("text").and_then(Value::as_str).map(str::to_string),
                Some("image") => Some("[image omitted]".to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Turn one JSONL record into searchable units. Images and thinking blocks are
/// never included; `tools` maps tool_use ids to tool names across records.
pub fn extract_units(rec: &Value, tools: &mut HashMap<String, String>) -> Vec<Unit> {
    let t = rec.get("type").and_then(Value::as_str).unwrap_or("");
    if t != "user" && t != "assistant" {
        return vec![];
    }
    let msg = &rec["message"];
    let role = msg
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or(t)
        .to_string();
    let mut units = Vec::new();
    let mut add = |kind: &'static str, tool: Option<String>, text: String| {
        if !text.trim().is_empty() {
            units.push(Unit {
                role: role.clone(),
                kind,
                tool,
                text,
            });
        }
    };
    match &msg["content"] {
        Value::String(s) => add("text", None, s.clone()),
        Value::Array(blocks) => {
            for b in blocks {
                match b.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text" => add(
                        "text",
                        None,
                        b.get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    ),
                    "tool_use" => {
                        let name = b.get("name").and_then(Value::as_str).unwrap_or("?");
                        if let Some(id) = b.get("id").and_then(Value::as_str) {
                            tools.insert(id.to_string(), name.to_string());
                        }
                        add(
                            "tool_use",
                            Some(name.to_string()),
                            render_tool_input(&b["input"]),
                        );
                    }
                    "tool_result" => {
                        let name = b
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .and_then(|id| tools.get(id))
                            .cloned();
                        add("tool_result", name, tool_result_text(&b["content"]));
                    }
                    "image" => add("image", None, "[image omitted]".to_string()),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    units
}

pub fn grep_raw(path: &Path, o: &GrepOpts) -> Result<GrepResult, String> {
    let re = build_regex(o)?;
    let f = std::fs::File::open(path).map_err(|e| format!("cannot open transcript: {e}"))?;
    let mut rd = BufReader::new(f);
    let mut c = collector(&re, o);
    let mut tools = HashMap::new();
    let mut buf = Vec::new();
    let mut lineno = 0usize;
    'outer: loop {
        buf.clear();
        match rd.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("cannot read transcript: {e}")),
        }
        lineno += 1;
        let Ok(rec) = serde_json::from_slice::<Value>(&buf) else {
            continue;
        };
        let ts = rec
            .get("timestamp")
            .and_then(Value::as_str)
            .map(String::from);
        for u in extract_units(&rec, &mut tools) {
            let meta = Meta {
                line: lineno,
                role: Some(u.role),
                tool: u.tool,
                kind: Some(u.kind),
                ts: ts.clone(),
            };
            if !c.feed(&meta, &u.text, 1) {
                break 'outer;
            }
        }
    }
    Ok(GrepResult {
        matches: c.out,
        truncated: c.truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn opts(p: &str) -> GrepOpts {
        GrepOpts {
            pattern: p.into(),
            literal: false,
            ignore_case: true,
            context: 1,
            max_matches: 10,
            max_chars: 4000,
        }
    }

    #[test]
    fn extracts_text_tool_use_result_and_skips_images() {
        let mut tools = HashMap::new();
        let a = json!({"type":"assistant","message":{"role":"assistant","content":[
            {"type":"thinking","thinking":"secret thoughts"},
            {"type":"text","text":"running it"},
            {"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}});
        let u = extract_units(&a, &mut tools);
        assert_eq!(u.len(), 2);
        assert_eq!(u[1].tool.as_deref(), Some("Bash"));
        assert_eq!(u[1].text, "command: cargo test");

        let r = json!({"type":"user","message":{"role":"user","content":[
            {"type":"tool_result","tool_use_id":"t1","content":[
                {"type":"text","text":"ok 5 passed"},
                {"type":"image","source":{"type":"base64","data":"AAAABBBB"}}]}]}});
        let u = extract_units(&r, &mut tools);
        assert_eq!(u[0].tool.as_deref(), Some("Bash"));
        assert_eq!(u[0].kind, "tool_result");
        assert!(u[0].text.contains("ok 5 passed"));
        assert!(!u[0].text.contains("AAAA"));
        assert!(u[0].text.contains("[image omitted]"));

        let other = json!({"type":"queue-operation","content":"x"});
        assert!(extract_units(&other, &mut tools).is_empty());
        let plain = json!({"type":"user","message":{"role":"user","content":"hello"}});
        assert_eq!(extract_units(&plain, &mut tools)[0].text, "hello");
    }

    #[test]
    fn readable_windows_merge_and_number() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.md");
        std::fs::write(&p, "l1\nl2\nNEEDLE a\nl4\nl5\nl6\nl7\nl8\nNEEDLE b\nl10\n").unwrap();
        let mut o = opts("needle");
        o.context = 2;
        let r = grep_readable(&p, &o).unwrap();
        assert_eq!(r.matches.len(), 2);
        assert_eq!(r.matches[0].line, 3);
        assert_eq!(r.matches[0].text, "1-l1\n2-l2\n3:NEEDLE a\n4-l4\n5-l5");
        assert!(r.matches[1].text.starts_with("7-l7"));
        assert!(!r.truncated);

        o.context = 6; // windows overlap and merge into one
        assert_eq!(grep_readable(&p, &o).unwrap().matches.len(), 1);
    }

    #[test]
    fn limits_set_truncated() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.md");
        let body: String = (0..20)
            .map(|i| format!("hit {i}\nfiller\nfiller\nfiller\nfiller\n"))
            .collect();
        std::fs::write(&p, body).unwrap();
        let mut o = opts("hit");
        o.context = 0;
        o.max_matches = 3;
        let r = grep_readable(&p, &o).unwrap();
        assert_eq!(r.matches.len(), 3);
        assert!(r.truncated);

        o.max_matches = 50;
        o.max_chars = 30;
        let r = grep_readable(&p, &o).unwrap();
        assert!(r.truncated);
        assert!(r.matches.iter().map(|m| m.text.len()).sum::<usize>() <= 30);

        o.max_chars = 4000;
        let r = grep_readable(&p, &o).unwrap();
        assert_eq!(r.matches.len(), 20);
        assert!(!r.truncated);
    }

    #[test]
    fn literal_and_bad_regex() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.md");
        std::fs::write(&p, "a.b\nfiller\naxb\n").unwrap();
        let mut o = opts("a.b");
        o.context = 0;
        assert_eq!(grep_readable(&p, &o).unwrap().matches.len(), 2);
        o.literal = true;
        assert_eq!(grep_readable(&p, &o).unwrap().matches.len(), 1);
        assert!(grep_readable(&p, &opts("(")).is_err());
        assert!(grep_readable(&p, &opts("")).is_err());
    }

    #[test]
    fn long_lines_are_clipped_around_match() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.md");
        std::fs::write(
            &p,
            format!("{}NEEDLE{}\n", "x".repeat(5000), "y".repeat(5000)),
        )
        .unwrap();
        let r = grep_readable(&p, &opts("needle")).unwrap();
        assert!(r.matches[0].text.contains("NEEDLE"));
        assert!(r.matches[0].text.chars().count() < 400);
    }

    #[test]
    fn raw_reports_role_tool_and_line() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.jsonl");
        let lines = [
            json!({"type":"ai-title","aiTitle":"x"}).to_string(),
            "not json at all".to_string(),
            json!({"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t9","name":"Grep","input":{"pattern":"foo"}}]}}).to_string(),
            json!({"type":"user","message":{"role":"user","content":[
                {"type":"tool_result","tool_use_id":"t9","content":"src/a.rs:1: needle here"},
                {"type":"image","source":{"data":"needle-in-base64"}}]}}).to_string(),
        ];
        std::fs::write(&p, lines.join("\n")).unwrap();
        let r = grep_raw(&p, &opts("needle")).unwrap();
        assert_eq!(r.matches.len(), 1);
        let m = &r.matches[0];
        assert_eq!(m.line, 4);
        assert_eq!(m.role.as_deref(), Some("user"));
        assert_eq!(m.tool.as_deref(), Some("Grep"));
        assert_eq!(m.kind, Some("tool_result"));
        assert!(m.text.contains("needle here"));
        let r2 = grep_raw(&p, &opts("pattern: foo")).unwrap();
        assert_eq!(r2.matches[0].ts.as_deref(), Some("2026-01-01T00:00:00Z"));
    }
}
