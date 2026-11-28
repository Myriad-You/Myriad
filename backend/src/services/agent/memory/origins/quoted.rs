//! The words a memory quotes, as its evidence gives them, and which lines
//! say them: how [`super::forget`] finds the lines a memory came from, and
//! [`super::quoting`] the memories a line taught. A quote is plain text with
//! runs of space as one; a few words alone are held to more, for they are
//! said in many lines.

/// Fewer characters than this say too little to be found by: a quote this
/// short is not looked for.
pub(super) const SHORTEST_QUOTE: usize = 2;
/// Fewer letters or characters than this ('不是', '好的') are said in many
/// lines: one of several a memory quotes is then a whole line's words, never
/// found inside another's.
const TELLS_ALONE: usize = 6;

/// The words as said, with runs of space as one: a quote is plain text (a
/// record's evidence is JSON and quotes nothing).
pub(crate) fn quote(evidence: Option<&str>) -> Option<String> {
    let evidence = evidence?.trim();
    if evidence.starts_with('{') || evidence.starts_with('[') {
        return None;
    }
    let quote = evidence.split_whitespace().collect::<Vec<_>>().join(" ");
    (quote.chars().count() >= SHORTEST_QUOTE).then_some(quote)
}

/// Runs of space as one.
pub(crate) fn folded(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `text` says too little to tell a line by ([`TELLS_ALONE`]).
pub(super) fn few(text: &str) -> bool {
    text.chars().filter(|ch| ch.is_alphanumeric()).count() < TELLS_ALONE
}

/// Words a memory quotes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Quote {
    pub(crate) text: String,
    /// All a line says, not a part of one: too few to be found inside one.
    pub(crate) whole: bool,
}

impl Quote {
    /// Whether a line saying `said` ([`folded`]) is one it quotes.
    pub(crate) fn is_in(&self, said: &str) -> bool {
        let letters =
            |text: &str| -> Vec<char> { text.chars().filter(|ch| ch.is_alphanumeric()).collect() };
        said.contains(self.text.as_str()) && (!self.whole || letters(said) == letters(&self.text))
    }

    /// Of lines saying `said` ([`folded`], each within its reach), which it
    /// quotes: for a few words alone, those that say only them, else the one
    /// they are in when they are in no other.
    pub(crate) fn among(&self, said: &[String]) -> Vec<usize> {
        let those = |quote: &Self| -> Vec<usize> {
            (0..said.len())
                .filter(|at| quote.is_in(&said[*at]))
                .collect()
        };
        let mut of = those(&self.strictly());
        if of.is_empty() {
            of = those(self);
            of.truncate(if of.len() == 1 { 1 } else { 0 });
        }
        of
    }

    /// Whether it is a few words alone: in many lines, it may be.
    pub(crate) fn few(&self) -> bool {
        !self.whole && few(&self.text)
    }

    /// Held to more: few words are a whole line's, alone or not.
    pub(crate) fn strictly(&self) -> Self {
        Self {
            text: self.text.clone(),
            whole: self.whole || few(&self.text),
        }
    }
}

/// What a memory quotes, each as [`quote`] reads one: one put together from
/// several lines quotes each of them, with ` / ` between
/// (`myriad_merope::chat_remember::settle_evidence_holds`), and may be one
/// line that has that in it.
pub(crate) fn quotes(evidence: Option<&str>) -> Vec<Quote> {
    let Some(all) = quote(evidence) else {
        return Vec::new();
    };
    let parts: Vec<String> = all
        .split(" / ")
        .filter_map(|part| quote(Some(part)))
        .collect();
    let mut quotes = vec![Quote {
        text: all.clone(),
        whole: false,
    }];
    if all.contains(" / ") {
        for text in parts {
            let part = Quote {
                whole: few(&text),
                text,
            };
            if !quotes.contains(&part) {
                quotes.push(part);
            }
        }
    }
    quotes
}
