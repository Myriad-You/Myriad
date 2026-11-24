//! A turned drawing whose front hair the fit could not follow.
//!
//! The image model sometimes redraws the fringe in one turn (longer, lower over
//! the forehead, a lock moved): that drawing's front hair is a different
//! shape, and keys fitted onto it make the hair grow or shrink as the head
//! turns that way. The Space's picture check (`check_turns`) does not always
//! see it; the fit's own report does: the front hair's outline is far further
//! off on that side than on the opposite one.

use serde_json::Value;

use super::TURN_SIDES;

/// A side is redrawn past what keys can follow when its front hair outline is
/// off this many times its opposite's, and by this many pixels more.
const AGAINST_OPPOSITE: f64 = 1.6;
const MORE_PIXELS: f64 = 5.0;

/// The front hair's mean outline distance (px) on a side, from a fit's keys JSON.
pub(super) fn hair_outline(keyforms: &[u8], side: &str) -> Option<f64> {
    let keys: Value = serde_json::from_slice(keyforms).ok()?;
    keys.pointer(&format!("/fit/front-hair/{side}/outline"))?
        .as_f64()
}

/// The index (in `TURN_SIDES`) of the turned drawing whose front hair the fit
/// followed worst, if it is far worse than its opposite turn's.
pub(super) fn misfit_side(keyforms: &[u8]) -> Option<usize> {
    misfit_of(std::array::from_fn(|index| {
        hair_outline(keyforms, TURN_SIDES[index])
    }))
}

/// The same, of the four sides' front hair outline distances themselves (in
/// `TURN_SIDES` order; none where a side has none): the one rule, whether the
/// distances are read from a fit's report or measured ahead of it.
pub(super) fn misfit_of(outlines: [Option<f64>; 4]) -> Option<usize> {
    let opposite = |index: usize| index ^ 1;
    (0..TURN_SIDES.len())
        .filter_map(|index| {
            let own = outlines[index]?;
            let other = outlines[opposite(index)]?;
            (own >= AGAINST_OPPOSITE * other && own >= other + MORE_PIXELS).then_some((index, own))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn keys(plus: f64, minus: f64, up: f64, down: f64) -> Vec<u8> {
        serde_json::to_vec(&json!({ "fit": { "front-hair": {
            "plus": { "outline": plus }, "minus": { "outline": minus },
            "up": { "outline": up }, "down": { "outline": down },
        } } }))
        .unwrap()
    }

    #[test]
    fn the_side_far_worse_than_its_opposite_is_the_misfit() {
        // The figure whose left-turn drawing grew its fringe (2026-10-10).
        assert_eq!(misfit_side(&keys(13.4, 25.5, 2.7, 4.6)), Some(1));
        assert_eq!(
            hair_outline(&keys(13.4, 25.5, 2.7, 4.6), "minus"),
            Some(25.5)
        );
    }

    #[test]
    fn small_or_even_misfits_are_left_as_drawn() {
        assert_eq!(
            misfit_side(&keys(3.0, 4.0, 2.7, 4.6)),
            None,
            "both turns alike"
        );
        assert_eq!(
            misfit_side(&keys(1.0, 3.0, 2.0, 2.0)),
            None,
            "three times, but 2 px"
        );
        assert_eq!(misfit_side(b"{}"), None);
    }

    #[test]
    fn the_rule_is_the_same_of_distances_measured_ahead_of_the_fit() {
        let some = |outlines: [f64; 4]| outlines.map(Some);
        for outlines in [
            [13.4, 25.5, 2.7, 4.6],
            [3.0, 4.0, 2.7, 4.6],
            [1.0, 3.0, 2.0, 2.0],
            [9.0, 2.0, 30.0, 4.0],
            // At the bound in pixels, just short of it, and short of the bound in times.
            [5.0, 10.0, 2.0, 2.0],
            [5.0, 9.9, 2.0, 2.0],
            [10.0, 15.9, 2.0, 2.0],
        ] {
            let [plus, minus, up, down] = outlines;
            assert_eq!(
                misfit_of(some(outlines)),
                misfit_side(&keys(plus, minus, up, down)),
                "{outlines:?}"
            );
        }
        // The worst of two misfit sides.
        assert_eq!(misfit_of(some([9.0, 2.0, 30.0, 4.0])), Some(2));
        assert_eq!(misfit_of(some([5.0, 10.0, 2.0, 2.0])), Some(1));
        assert_eq!(misfit_of(some([5.0, 9.9, 2.0, 2.0])), None);
        assert_eq!(misfit_of(some([10.0, 15.9, 2.0, 2.0])), None);
        // A side with no front hair fitted is no misfit, nor is its opposite.
        assert_eq!(misfit_of([Some(13.4), None, Some(2.7), Some(4.6)]), None);
        assert_eq!(misfit_of([None; 4]), None);
    }
}
