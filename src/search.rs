use crate::text::{query_terms, tokenize};
use std::collections::HashMap;

pub struct Field {
    pub name: &'static str,
    pub weight: f32,
    counts: HashMap<String, u32>,
    len: usize,
    lower: String,
}

pub struct Doc {
    pub fields: Vec<Field>,
}

impl Doc {
    /// Every doc of one corpus must pass the same field layout in the same order.
    pub fn new(fields: &[(&'static str, f32, &str)]) -> Doc {
        Doc {
            fields: fields
                .iter()
                .map(|(name, weight, text)| {
                    let tokens = tokenize(text);
                    let len = tokens.len();
                    let mut counts = HashMap::new();
                    for t in tokens {
                        *counts.entry(t).or_insert(0u32) += 1;
                    }
                    Field {
                        name,
                        weight: *weight,
                        counts,
                        len,
                        lower: text.to_lowercase(),
                    }
                })
                .collect(),
        }
    }
}

pub struct Hit {
    pub index: usize,
    pub score: f32,
    pub matched: Vec<&'static str>,
}

const K1: f32 = 1.2;
const B: f32 = 0.75;

fn term_freq(f: &Field, term: &str) -> f32 {
    let exact = f.counts.get(term).copied().unwrap_or(0) as f32;
    let prefix = if term.chars().count() >= 4 {
        f.counts
            .iter()
            .filter(|(k, _)| k.as_str() != term && k.starts_with(term))
            .map(|(_, c)| *c as f32)
            .sum::<f32>()
    } else {
        0.0
    };
    exact + 0.5 * prefix
}

/// Weighted per-field BM25 with a prefix match and a whole-phrase bonus.
/// Ties keep the input order, so pass docs newest-first.
pub fn rank(docs: &[&Doc], query: &str) -> Vec<Hit> {
    let terms = query_terms(query);
    if terms.is_empty() || docs.is_empty() {
        return vec![];
    }
    let phrase = query.trim().to_lowercase();
    let nfields = docs[0].fields.len();
    let n = docs.len() as f32;
    let avg: Vec<f32> = (0..nfields)
        .map(|i| {
            let total: usize = docs.iter().map(|d| d.fields[i].len).sum();
            (total as f32 / n).max(1.0)
        })
        .collect();
    let idf: Vec<f32> = terms
        .iter()
        .map(|t| {
            let df = docs
                .iter()
                .filter(|d| d.fields.iter().any(|f| term_freq(f, t) > 0.0))
                .count() as f32;
            (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
        })
        .collect();

    let mut hits = Vec::new();
    for (index, doc) in docs.iter().enumerate() {
        let mut score = 0.0;
        let mut matched = Vec::new();
        for (fi, f) in doc.fields.iter().enumerate() {
            let mut fscore = 0.0;
            for (ti, t) in terms.iter().enumerate() {
                let tf = term_freq(f, t);
                if tf > 0.0 {
                    let norm = tf + K1 * (1.0 - B + B * f.len as f32 / avg[fi]);
                    fscore += idf[ti] * tf * (K1 + 1.0) / norm;
                }
            }
            if phrase.chars().count() >= 3 && f.lower.contains(&phrase) {
                fscore += 0.8;
            }
            if fscore > 0.0 {
                score += f.weight * fscore;
                matched.push(f.name);
            }
        }
        if score > 0.0 {
            hits.push(Hit {
                index,
                score,
                matched,
            });
        }
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(title: &str, body: &str) -> Doc {
        Doc::new(&[("title", 3.0, title), ("body", 1.0, body)])
    }

    #[test]
    fn title_match_beats_body_match() {
        let a = doc("Unrelated chat", "we talked about wake proxy once");
        let b = doc("wake-proxy build", "go server");
        let c = doc("Other", "nothing here");
        let hits = rank(&[&a, &b, &c], "wake-proxy");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].index, 1);
        assert!(hits[0].matched.contains(&"title"));
        assert_eq!(hits[1].index, 0);
    }

    #[test]
    fn rare_terms_outweigh_common() {
        let a = doc("a", "common common rarity");
        let b = doc("b", "common common common");
        let c = doc("c", "common stuff");
        let hits = rank(&[&b, &c, &a], "common rarity");
        assert_eq!(hits[0].index, 2);
    }

    #[test]
    fn prefix_matching_and_ties_keep_order() {
        let a = doc("torrenting setup", "");
        let b = doc("torrenting setup", "");
        let hits = rank(&[&a, &b], "torrent");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].index, 0);
    }

    #[test]
    fn empty_query_and_no_match() {
        let a = doc("x", "y");
        assert!(rank(&[&a], "   ").is_empty());
        assert!(rank(&[&a], "zzzz").is_empty());
    }
}
