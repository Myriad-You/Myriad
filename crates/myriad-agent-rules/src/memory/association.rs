//! Spreading activation over one person's memories.
//!
//! What the query names directly lights up first; activation then flows along
//! two kinds of association and brings related memories to mind:
//! - a shared concept (both are about the cat, even if one only says 年糕);
//! - learned close together in time (the same conversation).
//!
//! The graph is never stored. It is rebuilt from the rows admitted for this
//! audience every time, so a memory the present audience may not hear can
//! neither surface nor pass activation on to one it may.
//!
//! Update per step, after Synapse (arXiv 2601.02744):
//! `a' = (1 - DECAY) * a + SPREAD * inflow`, where inflow is divided by the
//! sender's fan-out, so a concept half of all memories share (a hub) passes on
//! little. Activation is capped at 1.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, FixedOffset};

use crate::Concept;

const DECAY: f64 = 0.5;
const SPREAD: f64 = 0.8;
const STEPS: usize = 2;
/// Memories learned within this many seconds of each other are linked.
const WINDOW_SECS: f64 = 3600.0;
/// Link strength falls by e every fifteen minutes apart.
const TIME_SCALE_SECS: f64 = 900.0;
/// Each memory links to at most this many later ones. Rows imported in bulk
/// share one timestamp; without a cap they would make a complete graph.
const TIME_LINKS: usize = 8;

pub struct Node<'a> {
    pub concepts: &'a [Concept],
    pub at: DateTime<FixedOffset>,
}

/// Activation of every node after spreading from `seeds` (each in `0..=1`).
pub fn spread(seeds: &[f64], nodes: &[Node]) -> Vec<f64> {
    debug_assert_eq!(seeds.len(), nodes.len());
    let mut activation: Vec<f64> = seeds.iter().map(|seed| seed.clamp(0.0, 1.0)).collect();
    if activation.iter().all(|value| *value == 0.0) {
        return activation;
    }
    let members = concept_members(nodes);
    let neighbours = time_neighbours(nodes);
    for _ in 0..STEPS {
        let mut inflow = vec![0.0; nodes.len()];
        for members in members.values() {
            // Everything each member sends into the concept, spread over all
            // of its members: a hub carries little to any one of them.
            let pooled: f64 = members
                .iter()
                .map(|&index| activation[index] / nodes[index].concepts.len() as f64)
                .sum();
            for &index in members {
                let own = activation[index] / nodes[index].concepts.len() as f64;
                inflow[index] += (pooled - own) / members.len() as f64;
            }
        }
        for (index, links) in neighbours.iter().enumerate() {
            if activation[index] == 0.0 {
                continue;
            }
            let total: f64 = links.iter().map(|(_, weight)| weight).sum();
            for &(other, weight) in links {
                inflow[other] += activation[index] * weight / total.max(1.0);
            }
        }
        activation = activation
            .iter()
            .zip(&inflow)
            .map(|(value, inflow)| ((1.0 - DECAY) * value + SPREAD * inflow).min(1.0))
            .collect();
    }
    // A memory the query itself named never ends below where it started.
    activation
        .iter()
        .zip(seeds)
        .map(|(value, seed)| value.max(*seed))
        .collect()
}

/// Mind-wandering: from `start`, take up to `steps` steps along association,
/// each to a neighbour chosen in proportion to link strength (a shared rare
/// concept pulls harder than a hub; learned together pulls hardest when close
/// in time). `avoid` nodes are never landed on, so the same thought does not
/// keep coming back. `roll` yields uniform numbers in `0..1`. Returns where
/// the walk stopped if it moved at all.
///
/// A random walk on an association network reproduces how people drift
/// through memory (Abbott, Austerweil & Griffiths 2015); no model is needed
/// until there is something worth saying.
pub fn wander(
    nodes: &[Node],
    start: usize,
    steps: usize,
    avoid: &[usize],
    roll: &mut impl FnMut() -> f64,
) -> Option<usize> {
    if start >= nodes.len() {
        return None;
    }
    let members = concept_members(nodes);
    let neighbours = time_neighbours(nodes);
    let mut at = start;
    let mut visited = vec![start];
    for _ in 0..steps {
        let mut options: HashMap<usize, f64> = HashMap::new();
        for concept in nodes[at].concepts {
            if let Some(members) = members.get(&concept.name.to_lowercase()) {
                for &other in members {
                    *options.entry(other).or_default() += 1.0 / members.len() as f64;
                }
            }
        }
        for &(other, weight) in &neighbours[at] {
            *options.entry(other).or_default() += weight;
        }
        let mut options: Vec<(usize, f64)> = options
            .into_iter()
            .filter(|(other, _)| !visited.contains(other) && !avoid.contains(other))
            .collect();
        if options.is_empty() {
            break;
        }
        options.sort_by_key(|(other, _)| *other);
        let total: f64 = options.iter().map(|(_, weight)| weight).sum();
        let mut pick = roll().clamp(0.0, 1.0) * total;
        let mut next = options[options.len() - 1].0;
        for (other, weight) in options {
            if pick < weight {
                next = other;
                break;
            }
            pick -= weight;
        }
        visited.push(next);
        at = next;
    }
    (at != start).then_some(at)
}

/// Each shared concept's members, in the concepts' own order: activation is
/// summed concept by concept, and a fixed order keeps the sum the same on
/// every run and every platform.
fn concept_members(nodes: &[Node]) -> BTreeMap<String, Vec<usize>> {
    let mut members: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        for concept in node.concepts {
            let entry = members.entry(concept.name.to_lowercase()).or_default();
            if entry.last() != Some(&index) {
                entry.push(index);
            }
        }
    }
    members.retain(|_, members| members.len() > 1);
    members
}

fn time_neighbours(nodes: &[Node]) -> Vec<Vec<(usize, f64)>> {
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&index| nodes[index].at);
    let mut neighbours = vec![Vec::new(); nodes.len()];
    for (position, &index) in order.iter().enumerate() {
        for &other in order[position + 1..].iter().take(TIME_LINKS) {
            let apart = (nodes[other].at - nodes[index].at).num_seconds() as f64;
            if apart > WINDOW_SECS {
                break;
            }
            let weight = (-apart / TIME_SCALE_SECS).exp();
            neighbours[index].push((other, weight));
            neighbours[other].push((index, weight));
        }
    }
    neighbours
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concept(name: &str) -> Concept {
        Concept {
            name: name.into(),
            aliases: Vec::new(),
        }
    }

    fn at(minutes: i64) -> DateTime<FixedOffset> {
        (chrono::DateTime::parse_from_rfc3339("2026-09-01T00:00:00+08:00").unwrap())
            + chrono::Duration::minutes(minutes)
    }

    #[test]
    fn a_shared_concept_brings_a_memory_to_mind() {
        let cat = [concept("猫"), concept("年糕")];
        let mochi = [concept("年糕")];
        let tea = [concept("茶")];
        let nodes = [
            Node {
                concepts: &cat,
                at: at(0),
            },
            Node {
                concepts: &mochi,
                at: at(60 * 24 * 30),
            },
            Node {
                concepts: &tea,
                at: at(60 * 24 * 60),
            },
        ];
        let activation = spread(&[1.0, 0.0, 0.0], &nodes);
        assert!(activation[1] > 0.1, "年糕 links the two: {activation:?}");
        assert_eq!(activation[2], 0.0, "nothing links tea");
        assert_eq!(activation[0], 1.0);
    }

    #[test]
    fn memories_learned_together_come_back_together() {
        let none: [Concept; 0] = [];
        let nodes = [
            Node {
                concepts: &none,
                at: at(0),
            },
            Node {
                concepts: &none,
                at: at(5),
            },
            Node {
                concepts: &none,
                at: at(60 * 24),
            },
        ];
        let activation = spread(&[1.0, 0.0, 0.0], &nodes);
        assert!(activation[1] > 0.1, "{activation:?}");
        assert_eq!(activation[2], 0.0, "a day apart is not the same moment");
    }

    #[test]
    fn a_hub_concept_passes_on_less_than_a_rare_one() {
        let hub = [concept("日常")];
        let rare = [concept("年糕")];
        let mut nodes = vec![
            Node {
                concepts: &rare,
                at: at(0),
            },
            Node {
                concepts: &rare,
                at: at(60 * 24 * 10),
            },
        ];
        for day in 0..8 {
            nodes.push(Node {
                concepts: &hub,
                at: at(60 * 24 * (20 + day * 5)),
            });
        }
        let mut seeds = vec![0.0; nodes.len()];
        seeds[0] = 1.0;
        seeds[2] = 1.0;
        let activation = spread(&seeds, &nodes);
        assert!(
            activation[1] > activation[3],
            "rare link {} vs hub link {}",
            activation[1],
            activation[3]
        );
    }

    #[test]
    fn wandering_follows_associations_and_avoids_recent_thoughts() {
        let cat = [concept("猫"), concept("年糕")];
        let mochi = [concept("年糕")];
        let tea = [concept("茶")];
        let nodes = [
            Node {
                concepts: &cat,
                at: at(0),
            },
            Node {
                concepts: &mochi,
                at: at(60 * 24 * 30),
            },
            Node {
                concepts: &tea,
                at: at(60 * 24 * 60),
            },
        ];
        let mut always = || 0.5;
        assert_eq!(wander(&nodes, 0, 3, &[], &mut always), Some(1));
        assert_eq!(
            wander(&nodes, 0, 3, &[1], &mut always),
            None,
            "the only association was thought of lately"
        );
        assert_eq!(
            wander(&nodes, 2, 3, &[], &mut always),
            None,
            "nothing links tea"
        );
        assert_eq!(wander(&nodes, 9, 3, &[], &mut always), None);
    }

    #[test]
    fn no_seed_means_nothing_comes_to_mind() {
        let cat = [concept("猫")];
        let nodes = [
            Node {
                concepts: &cat,
                at: at(0),
            },
            Node {
                concepts: &cat,
                at: at(1),
            },
        ];
        assert_eq!(spread(&[0.0, 0.0], &nodes), vec![0.0, 0.0]);
    }
}
