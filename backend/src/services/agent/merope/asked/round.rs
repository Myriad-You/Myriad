//! One going over of some items: part by part, then one by one what is not
//! told together.

use std::ops::Range;
use std::sync::atomic::Ordering;

use myriad_merope::not_to_keep::parts;

use super::{By, CallFailure, LEFT_OUT, Told, UNANSWERED, judged, saying_something};

pub(super) struct Round<'a> {
    user_id: i32,
    asked: &'a [String],
    by: &'a By<'a>,
    /// Those that say something, and the place of each among all of them.
    items: Vec<String>,
    places: Vec<usize>,
    told: Told,
    /// How the last not told of ended.
    failure: Option<CallFailure>,
    /// Whether any ask was answered, be it with nothing that reads.
    answers: bool,
}

impl<'a> Round<'a> {
    pub(super) fn of(user_id: i32, asked: &'a [String], by: &'a By<'a>, items: &[String]) -> Self {
        let (places, items) = saying_something(items);
        Self {
            user_id,
            asked,
            by,
            items,
            places,
            told: Told::default(),
            failure: None,
            answers: false,
        }
    }

    /// Goes over all of them, till one could not be asked about.
    pub(super) async fn gone_over(&mut self) {
        // All of a part refused, one by one too, and nothing answered yet: it
        // may be who asks, and the rest are not asked about so till something
        // is answered.
        let mut apart = false;
        let mut together = Vec::new();
        for part in parts(&self.items) {
            let all = part.end..self.items.len();
            let part: Vec<usize> = part.collect();
            let Err(failure) = self.ask(part[0]..part[0] + part.len()).await else {
                continue;
            };
            if !failure.of_what_was_shown() {
                self.not_told(&part, failure);
                return self.not_told(&all.collect::<Vec<_>>(), failure);
            }
            if part.len() == 1 || (apart && !self.answers) {
                self.not_told(&part, failure);
                together.extend((part.len() > 1).then_some(part));
                continue;
            }
            apart = true;
            if let Err(failure) = self.one_by_one(&part).await {
                return self.not_told(&all.collect::<Vec<_>>(), failure);
            }
        }
        if self.any_told() {
            let _ = self.one_by_one(&together.concat()).await;
        }
    }

    /// Why not all of them are told of, if not.
    pub(super) fn failed(&self) -> Option<CallFailure> {
        (self.failure).filter(|_| !self.told.untold.is_empty())
    }

    /// Whether any of them was told of.
    pub(super) fn any_told(&self) -> bool {
        self.told.untold.len() < self.items.len()
    }

    /// What it came to, counted.
    pub(super) fn told(mut self) -> Told {
        self.told.failure = self.failed().map(Into::into);
        LEFT_OUT.fetch_add(self.told.out.len() as u64, Ordering::Relaxed);
        UNANSWERED.fetch_add(self.told.untold.len() as u64, Ordering::Relaxed);
        if !self.told.untold.is_empty() {
            tracing::warn!(
                user_id = self.user_id,
                untold = self.told.untold.len(),
                of = self.items.len(),
                "[Merope] what not to keep could not be told of all of it"
            );
        }
        self.told
    }

    /// Asks about `part` together.
    async fn ask(&mut self, part: Range<usize>) -> Result<(), CallFailure> {
        let items = &self.items[part.clone()];
        match judged(self.user_id, self.asked, self.by, items).await {
            Ok(drop) => {
                self.answers = true;
                for index in part.clone() {
                    self.told.untold.remove(&self.places[index]);
                }
                let first = part.start;
                (self.told.out).extend(drop.into_iter().map(|index| self.places[first + index]));
                Ok(())
            }
            Err(unanswered) => Err(self.told.unanswered(unanswered)),
        }
    }

    /// Asks about each of `indexes` alone, till one could not be asked about
    /// (`Err`).
    async fn one_by_one(&mut self, indexes: &[usize]) -> Result<(), CallFailure> {
        for (nth, &index) in indexes.iter().enumerate() {
            let Err(failure) = self.ask(index..index + 1).await else {
                continue;
            };
            if !failure.of_what_was_shown() {
                self.not_told(&indexes[nth..], failure);
                return Err(failure);
            }
            self.answers |= failure != CallFailure::Refused;
            self.not_told(&[index], failure);
        }
        Ok(())
    }

    fn not_told(&mut self, indexes: &[usize], failure: CallFailure) {
        let places = indexes.iter().map(|&index| self.places[index]);
        self.told.untold.extend(places);
        self.failure = Some(failure);
    }
}
