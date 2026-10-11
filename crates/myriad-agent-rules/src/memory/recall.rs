//! Which concept a person just brought up is one she knows only a little
//! about: a real gap worth one question.

use std::collections::HashMap;

use super::lexical::{self, Document};
use crate::Concept;

/// A concept she knows this few things about is one she knows only a little
/// about: curiosity peaks between knowing nothing and knowing plenty.
pub const THINLY_KNOWN: usize = 2;

/// How many memories each concept (by lowercased name) appears in.
pub fn concept_counts<'a>(
    concepts: impl IntoIterator<Item = &'a [Concept]>,
) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for concept in concepts.into_iter().flatten() {
        *counts.entry(concept.name.to_lowercase()).or_insert(0) += 1;
    }
    counts
}

/// The concept of a memory she knows least about, if she knows only a little,
/// with how many memories she has of it.
pub fn thinly_known(
    concepts: &[Concept],
    counts: &HashMap<String, usize>,
) -> Option<(String, usize)> {
    concepts
        .iter()
        .filter_map(|concept| {
            let known = counts.get(&concept.name.to_lowercase()).copied()?;
            (known <= THINLY_KNOWN).then_some((known, concept.name.clone()))
        })
        .min_by_key(|(known, _)| *known)
        .map(|(known, name)| (name, known))
}

/// Among `memories` (one person's, as recall sees them), the concept `query`
/// names that she knows only a little about, with how many memories she has
/// of it: of the memories `query` truly names (see `lexical`), best match
/// first, the first with such a concept.
pub fn gap_in(memories: &[Document], query: &str) -> Option<(String, usize)> {
    let scores = lexical::score_all(query, memories);
    let counts = concept_counts(memories.iter().map(|memory| memory.concepts));
    let mut named: Vec<(f64, usize)> = scores
        .iter()
        .enumerate()
        .filter(|(_, score)| score.strong)
        .map(|(index, score)| (score.value, index))
        .collect();
    named.sort_by(|left, right| right.0.total_cmp(&left.0));
    named
        .into_iter()
        .find_map(|(_, index)| thinly_known(memories[index].concepts, &counts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn about(names: &[&str]) -> Vec<Concept> {
        names
            .iter()
            .map(|name| Concept {
                name: name.to_string(),
                aliases: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn she_is_curious_where_she_knows_a_little_not_nothing_or_plenty() {
        let guitar = about(&["吉他"]);
        let cat = about(&["猫", "年糕"]);
        let memories = [
            Document {
                text: "最近在学吉他",
                concepts: &guitar,
            },
            Document {
                text: "养了一只猫叫年糕",
                concepts: &cat,
            },
            Document {
                text: "年糕上周打了疫苗",
                concepts: &cat,
            },
            Document {
                text: "年糕怕吸尘器",
                concepts: &cat,
            },
            Document {
                text: "年糕喜欢晒太阳",
                concepts: &cat,
            },
        ];
        assert_eq!(
            gap_in(&memories, "今天吉他弹了一小时"),
            Some(("吉他".into(), 1))
        );
        assert_eq!(
            gap_in(&memories, "猫今天好乖"),
            None,
            "she knows plenty about the cat"
        );
        assert_eq!(gap_in(&memories, "明日预报"), None, "nothing named");
        assert_eq!(gap_in(&[], "吉他"), None);
    }

    #[test]
    fn the_least_known_concept_of_a_memory_is_the_gap() {
        let concepts = [about(&["猫", "Mochi"]), about(&["mochi"]), about(&["猫"])];
        let counts = concept_counts(concepts.iter().map(Vec::as_slice));
        assert_eq!(counts.get("mochi"), Some(&2));
        assert_eq!(counts.get("猫"), Some(&2));
        let one = about(&["吉他", "猫"]);
        let counts = concept_counts([one.as_slice(), &concepts[0]]);
        assert_eq!(thinly_known(&one, &counts), Some(("吉他".into(), 1)));
        assert_eq!(thinly_known(&about(&["茶"]), &counts), None, "never seen");
    }
}
