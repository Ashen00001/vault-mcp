pub fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "of", "to", "and", "in", "is", "for", "on", "it", "my", "i", "with", "how",
];

/// Query terms with stopwords removed (kept if that would leave nothing).
pub fn query_terms(q: &str) -> Vec<String> {
    let all = tokenize(q);
    let kept: Vec<String> = all
        .iter()
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .cloned()
        .collect();
    let mut out = if kept.is_empty() { all } else { kept };
    out.dedup();
    out
}

/// Truncate to at most `max` chars, adding an ellipsis when cut.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Return a window of at most `max` chars of `s` that contains char index `center`.
pub fn clip_around(s: &str, center: usize, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let mut start = center.saturating_sub(max / 4).min(chars.len());
    let end = (start + max).min(chars.len());
    start = end.saturating_sub(max);
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// Char index of a byte offset.
pub fn char_index(s: &str, byte: usize) -> usize {
    s[..byte.min(s.len())].chars().count()
}

fn clean_line(line: &str) -> &str {
    let t = line.trim();
    let t = t.strip_prefix("> ").unwrap_or(t);
    t.strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .unwrap_or(t)
        .trim()
}

/// One-line snippet: the first line containing a query term, clipped around the hit.
pub fn snippet(text: &str, terms: &[String], max: usize) -> String {
    let mut fallback: Option<&str> = None;
    for line in text.lines() {
        let t = clean_line(line);
        if t.is_empty() {
            continue;
        }
        fallback.get_or_insert(t);
        let low = t.to_lowercase();
        if let Some(pos) = terms.iter().filter_map(|q| low.find(q.as_str())).min() {
            return clip_around(t, char_index(&low, pos), max);
        }
    }
    fallback.map(|t| clip(t, max)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_paths_and_hyphens() {
        assert_eq!(
            tokenize("Wake-Proxy v2/main.rs"),
            ["wake", "proxy", "v2", "main", "rs"]
        );
    }

    #[test]
    fn stopwords_dropped_unless_only_words() {
        assert_eq!(query_terms("the wake proxy"), ["wake", "proxy"]);
        assert_eq!(query_terms("the"), ["the"]);
    }

    #[test]
    fn clip_is_char_safe() {
        assert_eq!(clip("héllo wörld", 5), "héll…");
        assert_eq!(clip("short", 10), "short");
    }

    #[test]
    fn clip_around_keeps_center() {
        let s = format!("{}NEEDLE{}", "a".repeat(200), "b".repeat(200));
        let out = clip_around(&s, 200, 40);
        assert!(out.contains("NEEDLE"));
        assert!(out.chars().count() <= 42);
        assert!(out.starts_with('…') && out.ends_with('…'));
    }

    #[test]
    fn snippet_picks_matching_line() {
        let t = "> first line\n- second has Cargo here\nthird";
        let s = snippet(t, &["cargo".to_string()], 80);
        assert_eq!(s, "second has Cargo here");
        assert_eq!(snippet(t, &["zzz".to_string()], 80), "first line");
        assert_eq!(snippet("", &[], 80), "");
    }
}
