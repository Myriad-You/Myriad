//! Whether she joins in a group's talk when nobody called her by name: the
//! rules are `myriad_merope::joining`, and her group mind
//! (`myriad_merope::group`) gathers what she has that touches the talk and
//! asks. What is left here is the judgment as the semantic suite asks it.

#[cfg(test)]
use myriad_merope::joining::{Material, Standing, input, parse, schema, system};

/// The judgment as production asks it, for the semantic suite.
#[cfg(test)]
pub(crate) fn probe(
    soul: &str,
    conversation: &[String],
    material: &[Material],
) -> (String, serde_json::Value, String) {
    (
        system(soul),
        schema(),
        input(
            conversation,
            &Standing::default(),
            super::doing::now_text(crate::clock::now()).as_deref(),
            material,
        ),
    )
}

#[cfg(test)]
pub(crate) fn verdict(
    raw: &str,
    material: usize,
) -> Option<Option<myriad_merope::joining::Decision>> {
    parse(raw, material)
}
