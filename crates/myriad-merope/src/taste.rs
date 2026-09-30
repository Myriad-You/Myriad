//! Her taste, as it grows out of how things landed with her: not written
//! for her, and not a note she keeps, but what her reactions add up to.
//! It changes what comes her way. What moved her she wants again soon and
//! what was not for her she skips for a long while, the way the reactions
//! themselves are defined (see `doing::digest_system`); and among what is
//! at hand, what is by someone who keeps getting to her is likelier to be
//! there. Nothing is ever certain or shut out for good, so she still runs
//! into what she has never tried, and a taste she stops feeding fades.

use std::collections::HashMap;

use crate::doing::Reaction;
use crate::sources::Thing;

/// Something she took in, as her taste reads it.
pub struct Taken<'a> {
    pub thing: &'a Thing,
    /// None when nothing of it reached her.
    pub reaction: Option<Reaction>,
    pub days_ago: f64,
}

/// A reaction's weight halves over this many days.
const HALF_LIFE_DAYS: f64 = 21.0;

fn worth(reaction: Reaction) -> f64 {
    match reaction {
        Reaction::Moved => 2.0,
        Reaction::Liked => 1.0,
        Reaction::Fine => 0.0,
        Reaction::NotForMe => -2.0,
    }
}

/// How many days before she would take the same thing again, by how it
/// last landed: "stayed with you", "gladly put it on again soon", "would
/// not look for it", "would skip it".
fn again_after_days(reaction: Option<Reaction>) -> f64 {
    match reaction {
        Some(Reaction::Moved) => 1.0,
        Some(Reaction::Liked) | None => 3.0,
        Some(Reaction::Fine) => 7.0,
        Some(Reaction::NotForMe) => 30.0,
    }
}

struct Last {
    reaction: Option<Reaction>,
    days_ago: f64,
}

#[derive(Default)]
struct Sum {
    worth: f64,
    weight: f64,
}

impl Sum {
    fn add(&mut self, worth: f64, weight: f64) {
        self.worth += worth * weight;
        self.weight += weight;
    }

    /// The mean, drawn toward nothing while there is little of it.
    fn leaning(&self) -> f64 {
        if self.weight <= 0.0 {
            return 0.0;
        }
        self.worth / self.weight * (self.weight / (self.weight + 2.0))
    }
}

/// What her reactions add up to.
#[derive(Default)]
pub struct Taste {
    last: HashMap<String, Last>,
    things: HashMap<String, Sum>,
    /// By `kind:name` lowercased: the name as written, and the sum.
    by: HashMap<String, (String, Sum)>,
}

fn by_key(thing: &Thing) -> Option<String> {
    thing
        .by()
        .map(|by| format!("{}:{}", thing.kind(), by.trim().to_lowercase()))
}

impl Taste {
    pub fn of(taken: &[Taken<'_>]) -> Self {
        let mut taste = Self::default();
        for one in taken {
            let key = one.thing.key();
            let newer = taste
                .last
                .get(&key)
                .is_none_or(|last| one.days_ago < last.days_ago);
            if newer {
                taste.last.insert(
                    key.clone(),
                    Last {
                        reaction: one.reaction,
                        days_ago: one.days_ago,
                    },
                );
            }
            let Some(reaction) = one.reaction else {
                continue;
            };
            let weight = 0.5_f64.powf(one.days_ago.max(0.0) / HALF_LIFE_DAYS);
            taste
                .things
                .entry(key)
                .or_default()
                .add(worth(reaction), weight);
            if let (Some(by), Some(name)) = (by_key(one.thing), one.thing.by()) {
                taste
                    .by
                    .entry(by)
                    .or_insert_with(|| (name.trim().to_string(), Sum::default()))
                    .1
                    .add(worth(reaction), weight);
            }
        }
        taste
    }

    /// Whether she would take `thing` again by now; anything new is.
    pub fn would_again(&self, thing: &Thing) -> bool {
        self.last
            .get(&thing.key())
            .is_none_or(|last| last.days_ago >= again_after_days(last.reaction))
    }

    /// How much more (above 1) or less (below 1) likely `thing` is to be
    /// at hand than anything else, from how it and whoever it is by have
    /// landed with her.
    pub fn pull(&self, thing: &Thing) -> f64 {
        let itself = self
            .things
            .get(&thing.key())
            .map_or(0.0, |sum| sum.worth.clamp(-4.0, 4.0));
        let by = by_key(thing)
            .and_then(|by| self.by.get(&by))
            .map_or(0.0, |(_, sum)| sum.leaning());
        (0.35 * itself + 0.6 * by).exp().clamp(0.25, 4.0)
    }

    /// Who keeps getting to her (a leaning of at least `LIKES_BY`), best
    /// first, as a line each ("songs by ヨルシカ").
    pub fn liked_by(&self, most: usize) -> Vec<String> {
        self.liked(most).iter().map(By::line).collect()
    }

    /// Who keeps getting to her, best first.
    pub fn liked(&self, most: usize) -> Vec<By> {
        self.leaning_by(most, |leaning| leaning >= LIKES_BY)
    }

    /// Whose things keep not being for her, the least liked first.
    pub fn not_for_her(&self, most: usize) -> Vec<By> {
        self.leaning_by(most, |leaning| leaning <= -LIKES_BY)
    }

    fn leaning_by(&self, most: usize, keep: impl Fn(f64) -> bool) -> Vec<By> {
        let mut found: Vec<(&str, &str, f64)> = self
            .by
            .iter()
            .filter(|(_, (_, sum))| sum.weight >= KEPT_AT)
            .map(|(by, (name, sum))| (by.as_str(), name.as_str(), sum.leaning()))
            .filter(|(_, _, leaning)| keep(*leaning))
            .collect();
        found.sort_by(|a, b| b.2.abs().total_cmp(&a.2.abs()).then(a.0.cmp(b.0)));
        found
            .into_iter()
            .take(most)
            .map(|(by, name, _)| By {
                kind: if by.starts_with("song:") {
                    "song"
                } else {
                    "book"
                },
                name: name.to_string(),
            })
            .collect()
    }

    /// How `thing` landed the last time she had it and how many days ago,
    /// if she has had it.
    pub fn last_time(&self, thing: &Thing) -> Option<(Option<Reaction>, f64)> {
        self.last
            .get(&thing.key())
            .map(|last| (last.reaction, last.days_ago))
    }
}

/// Whose things they are: `kind` is "song" or "book".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct By {
    pub kind: &'static str,
    pub name: String,
}

impl By {
    /// "songs by ヨルシカ".
    pub fn line(&self) -> String {
        format!("{}s by {}", self.kind, self.name)
    }
}

/// A leaning past which someone is one she keeps liking.
pub const LIKES_BY: f64 = 0.6;
/// How much of someone (in fresh times) it takes to say she keeps liking
/// them: once is not yet a taste.
const KEPT_AT: f64 = 1.5;

/// `most` of `things`, drawn so a heavier one is likelier to be among them
/// (weighted sampling without replacement: each is keyed by roll^(1/weight)
/// and the highest keys are kept). `rolls` are uniform in (0, 1), one each.
pub fn draw<T>(things: Vec<T>, weights: &[f64], rolls: &[f64], most: usize) -> Vec<T> {
    let mut keyed: Vec<(f64, T)> = things
        .into_iter()
        .enumerate()
        .map(|(index, thing)| {
            let weight = weights.get(index).copied().unwrap_or(1.0).max(1e-6);
            let roll = rolls
                .get(index)
                .copied()
                .unwrap_or(0.5)
                .clamp(1e-12, 1.0 - 1e-12);
            (roll.powf(1.0 / weight), thing)
        })
        .collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
    keyed.truncate(most);
    keyed.into_iter().map(|(_, thing)| thing).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str, artist: &str) -> Thing {
        Thing::Song {
            id: id.into(),
            source: "netease".into(),
            name: format!("song {id}"),
            artist: artist.into(),
            album: String::new(),
            cover: String::new(),
            duration_ms: 200_000,
        }
    }

    #[test]
    fn what_moved_her_comes_back_soon_and_what_did_not_stays_away() {
        let (loved, skipped, plain, new) = (
            song("1", "ヨルシカ"),
            song("2", "Someone"),
            song("3", "Other"),
            song("4", "Nobody"),
        );
        let taken = [
            Taken {
                thing: &loved,
                reaction: Some(Reaction::Moved),
                days_ago: 1.5,
            },
            Taken {
                thing: &skipped,
                reaction: Some(Reaction::NotForMe),
                days_ago: 10.0,
            },
            Taken {
                thing: &plain,
                reaction: Some(Reaction::Fine),
                days_ago: 4.0,
            },
        ];
        let taste = Taste::of(&taken);
        assert!(taste.would_again(&loved));
        assert!(!taste.would_again(&skipped));
        assert!(!taste.would_again(&plain));
        assert!(taste.would_again(&new));
        // Only the latest time counts for when.
        let again = [
            Taken {
                thing: &skipped,
                reaction: Some(Reaction::Liked),
                days_ago: 4.0,
            },
            Taken {
                thing: &skipped,
                reaction: Some(Reaction::NotForMe),
                days_ago: 40.0,
            },
        ];
        assert!(Taste::of(&again).would_again(&skipped));
    }

    #[test]
    fn who_keeps_getting_to_her_pulls_and_a_new_name_is_neither() {
        let songs: Vec<Thing> = (0..4).map(|i| song(&i.to_string(), "ヨルシカ")).collect();
        let dull = song("9", "Dull");
        let mut taken: Vec<Taken> = songs
            .iter()
            .map(|thing| Taken {
                thing,
                reaction: Some(Reaction::Moved),
                days_ago: 2.0,
            })
            .collect();
        taken.push(Taken {
            thing: &dull,
            reaction: Some(Reaction::NotForMe),
            days_ago: 2.0,
        });
        let taste = Taste::of(&taken);
        let unheard_by_her = song("5", "ヨルシカ");
        assert!(taste.pull(&unheard_by_her) > 2.0);
        assert!(taste.pull(&song("10", "Dull")) < 1.0);
        assert_eq!(taste.pull(&song("11", "Stranger")), 1.0);
        assert_eq!(taste.liked_by(3), ["songs by ヨルシカ"]);
        // One skip is not yet a dislike either.
        assert!(taste.not_for_her(3).is_empty());
        let skipped_twice = [
            Taken {
                thing: &dull,
                reaction: Some(Reaction::NotForMe),
                days_ago: 1.0,
            },
            Taken {
                thing: &songs[0],
                reaction: Some(Reaction::Moved),
                days_ago: 1.0,
            },
        ];
        let other_dull = song("12", "Dull");
        let mut twice: Vec<Taken> = skipped_twice.into_iter().collect();
        twice.push(Taken {
            thing: &other_dull,
            reaction: Some(Reaction::NotForMe),
            days_ago: 1.0,
        });
        assert_eq!(
            Taste::of(&twice).not_for_her(3),
            [By {
                kind: "song",
                name: "Dull".into()
            }]
        );
        assert_eq!(
            taste.last_time(&songs[0]),
            Some((Some(Reaction::Moved), 2.0))
        );
        assert_eq!(taste.last_time(&unheard_by_her), None);
        // One good time is not yet a taste, however good.
        let once = [Taken {
            thing: &dull,
            reaction: Some(Reaction::Moved),
            days_ago: 0.0,
        }];
        assert!(Taste::of(&once).liked_by(3).is_empty());
        // Long ago weighs little.
        let long_ago: Vec<Taken> = songs
            .iter()
            .map(|thing| Taken {
                thing,
                reaction: Some(Reaction::Moved),
                days_ago: 120.0,
            })
            .collect();
        assert!(Taste::of(&long_ago).pull(&unheard_by_her) < 1.2);
    }

    #[test]
    fn a_heavier_one_is_likelier_but_never_certain() {
        let draws = 10_000;
        let mut first = 0;
        for index in 0..draws {
            let roll = |salt: u64| {
                let mut x = (index as u64 * 4 + salt).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                x ^= x >> 33;
                x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
                x ^= x >> 33;
                (x >> 11) as f64 / (1u64 << 53) as f64
            };
            let rolls = [roll(1), roll(2), roll(3), roll(4)];
            let drawn = draw(
                vec!["heavy", "a", "b", "c"],
                &[4.0, 1.0, 1.0, 1.0],
                &rolls,
                1,
            );
            if drawn == ["heavy"] {
                first += 1;
            }
        }
        // 4 / (4 + 3) of the time, give or take.
        let share = first as f64 / draws as f64;
        assert!((share - 4.0 / 7.0).abs() < 0.03, "{share}");
        assert_eq!(draw(vec![1, 2, 3], &[1.0; 3], &[0.1, 0.9, 0.5], 2), [2, 3]);
    }
}
