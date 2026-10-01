//! Lexical relevance for memory recall: BM25 over words and CJK bigrams.
//!
//! Chinese and Japanese have no spaces, and single characters are mostly noise
//! as evidence: "今天" and "天气" share 天 but nothing else. So a run of CJK
//! characters is read as overlapping bigrams, which carry full weight. Its
//! single characters are still indexed so a one-character word (猫) can be
//! found, but inside a longer run they only count as weak evidence.
//!
//! Repeating a word is not extra evidence: query terms are counted once.
//!
//! An English word is read as a reader reads it, whatever its ending: "bikes"
//! is "bike", "cooking" and "cooked" are "cook". Only regular endings are
//! taken off, and alike in memory and query, so what two forms fold to need
//! not be a word, only the same.
//!
//! A memory's concepts are terms too. When the query names a concept by any
//! of its names (猫, 喵, cat), every memory about that concept shares one
//! term with it, even with no word in common.

use std::collections::{HashMap, HashSet};

use super::unified::Concept;

const K1: f64 = 1.2;
const B: f64 = 0.75;
/// Weight of a single CJK character that sits inside a longer run.
const WEAK: f64 = 0.25;

#[derive(Debug, Clone, PartialEq)]
struct Term {
    text: String,
    weight: f64,
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3040..=0x30FF   // Hiragana, Katakana
        | 0x3400..=0x4DBF // CJK Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xAC00..=0xD7AF // Hangul syllables
        | 0xF900..=0xFAFF // CJK Compatibility Ideographs
        | 0x20000..=0x2FA1F)
}

fn terms(text: &str) -> Vec<Term> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut run: Vec<char> = Vec::new();
    for ch in text.chars() {
        if is_cjk(ch) {
            flush_word(&mut word, &mut out);
            run.push(ch);
        } else if ch.is_alphanumeric() {
            flush_run(&mut run, &mut out);
            word.extend(ch.to_lowercase());
        } else {
            flush_word(&mut word, &mut out);
            flush_run(&mut run, &mut out);
        }
    }
    flush_word(&mut word, &mut out);
    flush_run(&mut run, &mut out);
    out
}

fn flush_word(word: &mut String, out: &mut Vec<Term>) {
    // One letter or digit alone says nothing.
    if word.chars().count() >= 2 {
        out.push(Term {
            text: folded(std::mem::take(word)),
            weight: 1.0,
        });
    }
    word.clear();
}

/// `word` (lowercase) without its regular English ending; any other word as
/// it is.
fn folded(word: String) -> String {
    if !word.bytes().all(|byte| byte.is_ascii_lowercase()) || word.len() < 4 {
        return word;
    }
    let has_vowel = |stem: &str| stem.bytes().any(|byte| b"aeiouy".contains(&byte));
    let mut stem = word.clone();
    if let Some(base) = stem.strip_suffix("ies").filter(|base| base.len() >= 2) {
        stem = format!("{base}y");
    } else if let Some(base) = stem.strip_suffix("ied").filter(|base| base.len() >= 2) {
        stem = format!("{base}y");
    } else if ["sses", "shes", "ches", "xes", "zes"]
        .iter()
        .any(|ending| stem.ends_with(ending))
    {
        stem.truncate(stem.len() - 2);
    } else if stem.ends_with('s')
        && !["ss", "us", "is"]
            .iter()
            .any(|ending| stem.ends_with(ending))
    {
        stem.pop();
    } else if let Some(base) = stem
        .strip_suffix("ing")
        .or_else(|| stem.strip_suffix("ed"))
        .filter(|base| base.len() >= 3 && has_vowel(base))
    {
        let mut base = base.to_string();
        // running, stopped: the doubled consonant is the ending's.
        let bytes = base.as_bytes();
        let last = bytes[bytes.len() - 1];
        if bytes[bytes.len() - 2] == last && !b"aeioulsz".contains(&last) {
            base.pop();
        }
        stem = base;
    }
    // bake, baked, baking: the silent e goes with the ending.
    if stem.len() >= 4 && stem.ends_with('e') {
        stem.pop();
    }
    stem
}

fn flush_run(run: &mut Vec<char>, out: &mut Vec<Term>) {
    let single = if run.len() == 1 { 1.0 } else { WEAK };
    for ch in run.iter() {
        out.push(Term {
            text: ch.to_string(),
            weight: single,
        });
    }
    for pair in run.windows(2) {
        out.push(Term {
            text: pair.iter().collect(),
            weight: 1.0,
        });
    }
    run.clear();
}

/// How much of the shorter of two texts the other says too: the share of
/// its full-weight terms (words, CJK bigrams, a one-character word) that both
/// have. 0 when either has none. "他的猫叫豆豆" in "他养了一只叫豆豆的猫" is
/// 0.6; "周五考试" and "周六考试" share only 考试, 0.33.
pub fn overlap(a: &str, b: &str) -> f64 {
    let strong = |text: &str| -> HashSet<String> {
        terms(text)
            .into_iter()
            .filter(|term| term.weight >= 1.0)
            .map(|term| term.text)
            .collect()
    };
    let (a, b) = (strong(a), strong(b));
    let shorter = a.len().min(b.len());
    if shorter == 0 {
        return 0.0;
    }
    a.intersection(&b).count() as f64 / shorter as f64
}

/// One memory as recall sees it.
pub struct Document<'a> {
    pub text: &'a str,
    pub concepts: &'a [Concept],
}

/// Concept terms cannot collide with words: text never yields a control char.
fn concept_term(name: &str) -> String {
    format!("\u{1}{}", name.to_lowercase())
}

/// Whether `form` is mentioned in `text` (both lowercase). CJK has no word
/// boundaries, so any occurrence counts; elsewhere the form must stand as its
/// own word ("cat" is not in "category").
fn mentions(text: &str, form: &str) -> bool {
    if form.is_empty() {
        return false;
    }
    if form.chars().any(is_cjk) {
        return text.contains(form);
    }
    text.match_indices(form).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + form.len()..].chars().next();
        let boundary = |ch: Option<char>| ch.is_none_or(|ch| !ch.is_alphanumeric() || is_cjk(ch));
        boundary(before) && boundary(after)
    })
}

fn document_terms(document: &Document) -> Vec<Term> {
    let mut out = terms(document.text);
    out.extend(document.concepts.iter().map(|concept| Term {
        text: concept_term(&concept.name),
        weight: 1.0,
    }));
    out
}

/// Distinct query terms, keeping the strongest weight a term was seen with.
/// A concept of any document counts when the query names it.
fn query_terms(text: &str, documents: &[Document]) -> Vec<Term> {
    let mut best: HashMap<String, f64> = HashMap::new();
    let lower = text.to_lowercase();
    for concept in documents.iter().flat_map(|document| document.concepts) {
        if concept
            .surface_forms()
            .any(|form| mentions(&lower, &form.to_lowercase()))
        {
            best.insert(concept_term(&concept.name), 1.0);
        }
    }
    for term in terms(text) {
        let weight = best.entry(term.text).or_insert(0.0);
        *weight = weight.max(term.weight);
    }
    let mut out: Vec<Term> = best
        .into_iter()
        .map(|(text, weight)| Term { text, weight })
        .collect();
    out.sort_by(|left, right| left.text.cmp(&right.text));
    out
}

/// How well one document matched a query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// BM25 over all shared terms.
    pub value: f64,
    /// Whether a full-weight term (a word, a bigram, a one-character word)
    /// was shared. Weak single characters alone never make a match.
    pub strong: bool,
}

/// BM25 of every document against `query`, in document order. The corpus is
/// the documents themselves, so a term most memories share counts for little.
pub fn score_all(query: &str, documents: &[Document]) -> Vec<Score> {
    let query = query_terms(query, documents);
    if query.is_empty() || documents.is_empty() {
        return vec![
            Score {
                value: 0.0,
                strong: false,
            };
            documents.len()
        ];
    }
    let indexed: Vec<(HashMap<String, u32>, usize)> = documents
        .iter()
        .map(|document| {
            let terms = document_terms(document);
            let mut counts = HashMap::new();
            for term in &terms {
                *counts.entry(term.text.clone()).or_insert(0) += 1;
            }
            (counts, terms.len())
        })
        .collect();
    let total = indexed.len() as f64;
    let average_len = indexed.iter().map(|(_, len)| *len as f64).sum::<f64>() / total;
    let wanted: HashSet<&str> = query.iter().map(|term| term.text.as_str()).collect();
    let mut frequency: HashMap<&str, f64> = HashMap::new();
    for (counts, _) in &indexed {
        for text in counts.keys() {
            if let Some(text) = wanted.get(text.as_str()) {
                *frequency.entry(text).or_insert(0.0) += 1.0;
            }
        }
    }
    indexed
        .iter()
        .map(|(counts, len)| {
            let norm = K1 * (1.0 - B + B * (*len as f64) / average_len.max(1.0));
            let mut score = Score {
                value: 0.0,
                strong: false,
            };
            for term in &query {
                let Some(&count) = counts.get(&term.text) else {
                    continue;
                };
                let seen = frequency.get(term.text.as_str()).copied().unwrap_or(0.0);
                let idf = (1.0 + (total - seen + 0.5) / (seen + 0.5)).ln();
                let count = count as f64;
                score.value += term.weight * idf * count * (K1 + 1.0) / (count + norm);
                score.strong |= term.weight >= 1.0;
            }
            score
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score_all(query: &str, texts: &[&str]) -> Vec<Score> {
        let documents: Vec<Document> = texts
            .iter()
            .map(|text| Document {
                text,
                concepts: &[],
            })
            .collect();
        super::score_all(query, &documents)
    }

    fn texts(text: &str) -> Vec<(String, f64)> {
        terms(text)
            .into_iter()
            .map(|term| (term.text, term.weight))
            .collect()
    }

    #[test]
    fn overlap_is_how_much_of_the_shorter_both_say() {
        let same = super::overlap("他的猫叫豆豆", "他养了一只叫豆豆的猫");
        assert!((same - 0.6).abs() < 1e-9, "{same}");
        let changed = super::overlap("周五考试", "周六考试");
        assert!((changed - 1.0 / 3.0).abs() < 1e-9, "{changed}");
        assert_eq!(super::overlap("喜欢咖啡", "在学吉他"), 0.0);
        assert_eq!(super::overlap("", "喜欢咖啡"), 0.0);
        assert_eq!(super::overlap("Likes cycling", "likes to cycle"), 1.0);
    }

    #[test]
    fn an_english_word_is_the_same_word_whatever_its_ending() {
        for (one, other) in [
            ("bikes", "bike"),
            ("cooking", "cook"),
            ("cooked", "cooks"),
            ("baking", "bake"),
            ("baked", "bake"),
            ("stories", "story"),
            ("studied", "study"),
            ("classes", "class"),
            ("watches", "watch"),
            ("running", "run"),
            ("stopped", "stop"),
            ("buses", "bus"),
        ] {
            assert_eq!(
                folded(one.to_string()),
                folded(other.to_string()),
                "{one} / {other}"
            );
        }
        // Short words, words without such an ending, and anything not plain
        // English letters stay as they are.
        for word in [
            "was", "this", "class", "yoga", "thing", "being", "café", "mp3s",
        ] {
            assert_eq!(
                folded(word.to_string()),
                word.trim_end_matches('e'),
                "{word}"
            );
        }
        let scores = score_all("How many bikes do I own?", &["They also have a road bike."]);
        assert!(scores[0].strong);
    }

    #[test]
    fn cjk_runs_become_bigrams_with_weak_characters() {
        assert_eq!(
            texts("今天"),
            vec![
                ("今".into(), WEAK),
                ("天".into(), WEAK),
                ("今天".into(), 1.0)
            ]
        );
        assert_eq!(texts("猫"), vec![("猫".into(), 1.0)]);
        assert_eq!(
            texts("Likes 抹茶 a lot"),
            vec![
                // "likes", read as any form of "like".
                ("lik".into(), 1.0),
                ("抹".into(), WEAK),
                ("茶".into(), WEAK),
                ("抹茶".into(), 1.0),
                ("lot".into(), 1.0)
            ]
        );
        // Kana is read like any other CJK run.
        assert!(
            texts("ゲームが好き")
                .iter()
                .any(|(text, weight)| text == "好き" && *weight == 1.0)
        );
        assert!(texts("。，！ a").is_empty());
    }

    #[test]
    fn a_shared_character_alone_is_not_a_match() {
        let scores = score_all("今天", &["天气不错", "今天加班"]);
        assert!(!scores[0].strong, "今天 and 天气 only share 天");
        assert!(scores[0].value > 0.0);
        assert!(scores[1].strong);
        assert!(scores[1].value > scores[0].value);
    }

    #[test]
    fn a_one_character_query_word_is_a_match() {
        let scores = score_all("猫", &["养了一只猫", "喜欢狗"]);
        assert!(scores[0].strong);
        assert!(!scores[1].strong);
    }

    #[test]
    fn a_rare_term_outweighs_a_common_one() {
        let documents = [
            "likes tea in the morning",
            "likes tea at night",
            "likes tea with oolong",
            "likes coffee",
        ];
        let scores = score_all("oolong tea", &documents);
        assert!(scores[2].value > scores[0].value);
        assert!(scores[2].value > scores[1].value);
    }

    #[test]
    fn repeating_a_query_word_is_not_extra_evidence() {
        let once = score_all("tea", &["tea", "milk"]);
        let many = score_all("tea tea tea", &["tea", "milk"]);
        assert_eq!(once, many);
    }

    #[test]
    fn no_query_scores_nothing() {
        let scores = score_all("  。", &["tea"]);
        assert_eq!(scores[0].value, 0.0);
        assert!(!scores[0].strong);
        assert!(score_all("tea", &[]).is_empty());
    }

    fn concept(name: &str, aliases: &[&str]) -> Concept {
        Concept {
            name: name.into(),
            aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
        }
    }

    #[test]
    fn naming_a_concept_by_any_name_finds_its_memories() {
        let cat = [concept("猫", &["喵", "猫咪", "cat"])];
        let documents = [
            Document {
                text: "年糕最近不爱动",
                concepts: &cat,
            },
            Document {
                text: "今天吐了好几次",
                concepts: &[],
            },
        ];
        let scores = super::score_all("喵喵吃饭了吗", &documents);
        assert!(scores[0].strong, "喵 names the cat");
        assert!(scores[0].value > scores[1].value);
        assert!(super::score_all("My CAT is sick", &documents)[0].strong);
        assert!(
            !super::score_all("category theory", &documents)[0].strong,
            "cat inside a longer word is not the cat"
        );
    }

    #[test]
    fn latin_forms_need_word_boundaries_but_cjk_forms_do_not() {
        assert!(mentions("my cat.", "cat"));
        assert!(mentions("养了cat", "cat"));
        assert!(!mentions("concatenate", "cat"));
        assert!(mentions("喵喵叫", "喵"));
        assert!(!mentions("anything", ""));
    }
}
