use std::collections::BTreeMap;

pub struct Parsed<'a> {
    pub fields: BTreeMap<String, String>,
    pub body: &'a str,
}

/// Parse a minimal `key: value` YAML-ish frontmatter block.
/// Documents without a (terminated) block return no fields and the whole text as body.
pub fn parse(src: &str) -> Parsed<'_> {
    let s = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut fields = BTreeMap::new();
    let Some(rest) = s
        .strip_prefix("---\r\n")
        .or_else(|| s.strip_prefix("---\n"))
    else {
        return Parsed { fields, body: s };
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        let t = line.trim_end_matches(['\r', '\n']);
        if t.trim_end() == "---" {
            return Parsed {
                fields,
                body: &rest[offset..],
            };
        }
        if t.starts_with([' ', '\t', '#']) {
            continue;
        }
        if let Some((k, v)) = t.split_once(':') {
            let k = k.trim();
            if !k.is_empty() && !k.contains(' ') {
                fields.insert(k.to_string(), unquote(v.trim()));
            }
        }
    }
    Parsed {
        fields: BTreeMap::new(),
        body: s,
    }
}

pub fn unquote(v: &str) -> String {
    let b = v.as_bytes();
    if b.len() >= 2 && b[0] == b'"' && b[b.len() - 1] == b'"' {
        v[1..v.len() - 1].replace("\\\"", "\"")
    } else if b.len() >= 2 && b[0] == b'\'' && b[b.len() - 1] == b'\'' {
        v[1..v.len() - 1].replace("''", "'")
    } else {
        v.to_string()
    }
}

/// Parse an inline list like `[a, b, c]`.
pub fn parse_list(v: &str) -> Vec<String> {
    v.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|s| unquote(s.trim()))
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fields_and_body() {
        let p =
            parse("---\ntitle: \"Hello: world\"\ncwd: C:\\work dir\ntags: [a, b]\n---\n# Body\n");
        assert_eq!(p.fields["title"], "Hello: world");
        assert_eq!(p.fields["cwd"], "C:\\work dir");
        assert_eq!(parse_list(&p.fields["tags"]), vec!["a", "b"]);
        assert_eq!(p.body, "# Body\n");
    }

    #[test]
    fn handles_crlf_bom_and_missing_block() {
        let p = parse("\u{feff}---\r\ntitle: x\r\n---\r\nbody");
        assert_eq!(p.fields["title"], "x");
        assert_eq!(p.body, "body");
        let q = parse("no frontmatter\n");
        assert!(q.fields.is_empty());
        assert_eq!(q.body, "no frontmatter\n");
        let r = parse("---\ntitle: x\nnever closed\n");
        assert!(r.fields.is_empty());
    }

    #[test]
    fn unquotes_both_styles() {
        assert_eq!(unquote("\"a \\\"b\\\"\""), "a \"b\"");
        assert_eq!(unquote("'it''s'"), "it's");
        assert_eq!(unquote("plain"), "plain");
    }
}
