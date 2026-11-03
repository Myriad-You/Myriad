//! A gesture arm drawn on the portrait (the rig's arm variants) checked
//! against it: the model was asked to change one arm and nothing else, and
//! now and then it changes the sleeve too, drops armour, or copies the other
//! sleeve onto it. A variant that changed the design is drawn again or not
//! used.

use super::upright::side_by_side;
use super::vision::run_vision_call;
use super::*;

/// What a gesture arm kept of the portrait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmVariantCheck {
    /// The sleeve, glove or armour of the changed arm, and everything else, as drawn.
    pub same_design: bool,
    /// The changed arm does what it was asked to.
    pub gesture_shown: bool,
    pub differences: Vec<String>,
}

const MAX_DIFFERENCES: usize = 5;
const MAX_DIFFERENCE_CHARS: usize = 60;

/// Checks `variant`, the portrait with the arm on the viewer's `side`
/// ("left" or "right") redrawn doing `gesture` (words), against `original`.
pub async fn check_arm_variant(
    original: &ImageReference,
    variant: &ImageReference,
    side: &str,
    gesture: &str,
) -> Result<ArmVariantCheck, OnboardingAiError> {
    let pair = side_by_side(&original.bytes, &variant.bytes).ok_or(
        OnboardingAiError::UnusableResponse("the variant could not be compared"),
    )?;
    let pair = ImageReference::new(pair, "image/png")
        .map_err(|_| OnboardingAiError::UnusableResponse("the variant could not be compared"))?;
    let raw = run_vision_call(&arm_check_prompt(side, gesture), &pair).await?;
    parse_arm_check(&raw)
}

fn arm_check_prompt(side: &str, gesture: &str) -> String {
    let other = if side == "left" { "right" } else { "left" };
    format!(
        r#"The image shows two pictures side by side. LEFT is a character illustration. RIGHT is meant to be the same picture with only the arm on the viewer's {side} side changed: {gesture}. Answer with JSON only:
{{"sameDesign": true | false, "gestureShown": true | false, "differences": ["..."]}}

sameDesign — false when anything besides that arm's pose changed: the changed arm's sleeve, cuff, glove, armour or bracelet lost, added, lengthened, shortened or recolored; the {side} sleeve now drawn like the {other} one; the face, hair, outfit, the {other} arm or the background changed. Parts of the sleeve the new pose shows that the old one hid may be newly drawn if they match the rest of it.
gestureShown — false when the arm on the viewer's {side} side does not plainly do that, or the hand is badly drawn (wrong number of fingers, a broken wrist).
differences — up to five short English notes, each under 30 characters, naming what changed besides the pose; an empty list when nothing did."#
    )
}

fn parse_arm_check(raw: &str) -> Result<ArmVariantCheck, OnboardingAiError> {
    let parsed = parse_json_object(raw).ok_or(OnboardingAiError::UnusableResponse(
        "arm variant check was not valid JSON",
    ))?;
    let flag = |key: &str| {
        parsed
            .get(key)
            .and_then(Value::as_bool)
            .ok_or(OnboardingAiError::UnusableResponse(
                "arm variant check left out a verdict",
            ))
    };
    let differences = parsed
        .get("differences")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|note| {
                    note.trim()
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(MAX_DIFFERENCE_CHARS)
                        .collect::<String>()
                })
                .filter(|note| !note.is_empty())
                .take(MAX_DIFFERENCES)
                .collect()
        })
        .unwrap_or_default();
    Ok(ArmVariantCheck {
        same_design: flag("sameDesign")?,
        gesture_shown: flag("gestureShown")?,
        differences,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_names_the_changed_side_and_the_other() {
        let prompt = arm_check_prompt("right", "a friendly wave");
        assert!(prompt.contains("viewer's right side changed: a friendly wave"));
        assert!(prompt.contains("the right sleeve now drawn like the left one"));
    }

    #[test]
    fn a_verdict_is_read_with_its_notes_and_a_missing_one_is_refused() {
        let check = parse_arm_check(
            r#"```json
{"sameDesign": false, "gestureShown": true, "differences": ["armour lost on forearm", ""]}
```"#,
        )
        .unwrap();
        assert!(!check.same_design && check.gesture_shown);
        assert_eq!(check.differences, ["armour lost on forearm"]);
        assert!(parse_arm_check(r#"{"sameDesign": true}"#).is_err());
    }
}
