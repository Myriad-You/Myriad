//! The turned references judged before they are fitted (the Space's
//! `judge_turn`): a drawing that redrew what a turn keeps (its fringe, the
//! head's size, the body) is drawn again, up to `JUDGED_REDRAWS` times while
//! it is still faulted, and the best judged kept.

use std::collections::BTreeMap;

use super::{
    Stage, TURN_SIDES, TURNED_REDOS, TurnCache, decompose_checked, draw_turn, turn_prompts, update,
};
use crate::{
    api::merope_rig::ApiResult,
    services::{image_generation, see_through},
};

/// Each turned drawing's judgement, as kept; a side the Space could not judge is left out.
pub(super) type Judgements = BTreeMap<String, see_through::TurnJudgement>;

/// How many times a faulted turn is drawn again. One redraw left a fringe
/// still grown on two sets of the blind test (10-26, h00 and h06 plus: 1.37 and
/// 1.33); each more costs one drawing and one decomposition.
const JUDGED_REDRAWS: u32 = 2;

/// How many times a side was drawn again, from its cache marker (written
/// before each drawing, so a rerun never draws past the limit). The marker of
/// a job from before the limit was raised reads as one.
fn redraws_done(marker: Option<Vec<u8>>) -> u32 {
    match marker {
        None => 0,
        Some(bytes) => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(1),
    }
}

/// Judges the four turned drawings against the front one, draws a faulty one
/// again (decomposed and judged in turn) while it is still faulted, up to
/// `JUDGED_REDRAWS` times, and keeps whichever is judged best.
/// `decompositions` and `pictures` are the front's and the four turns', in
/// `TURN_SIDES` order after the front; replaced drawings go into the cache and
/// `redrawn`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn judge_turns(
    id: &str,
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    reference: &image_generation::ImageReference,
    (width, height): (u32, u32),
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
    options: see_through::DecomposeOptions,
    decompositions: &mut [Vec<u8>; 5],
    pictures: &mut [(Vec<u8>, String); 5],
    redrawn: &mut Vec<String>,
) -> ApiResult<Judgements> {
    let mut judged = Judgements::new();
    let front = (decompositions[0].clone(), pictures[0].clone());
    for (index, side) in TURN_SIDES.iter().enumerate() {
        let judge = |psd: Vec<u8>, picture: (Vec<u8>, String)| {
            client.judge_turn(front.clone(), (psd, picture), side)
        };
        let mut best = match judge(
            decompositions[index + 1].clone(),
            pictures[index + 1].clone(),
        )
        .await
        {
            Ok(Some(judgement)) => judgement,
            Ok(None) => return Ok(judged),
            Err(error) => {
                tracing::warn!(side, %error, "turned drawing could not be judged; fitted as drawn");
                continue;
            }
        };
        let marker = format!("turned-{index}.judged-again");
        let mut done = redraws_done(cache.read(&marker).await);
        while !best.faults.is_empty() && done < JUDGED_REDRAWS {
            done += 1;
            cache.write(&marker, done.to_string().as_bytes()).await;
            tracing::info!(side, attempt = done, faults = ?best.faults, fringe = ?best.fringe, head = ?best.head, "turned drawing redrew what a turn keeps; drawing it again");
            let again = match draw_turn(
                user_id,
                generation,
                &turn_prompts()[index],
                reference,
                width,
                height,
            )
            .await
            {
                Ok(png) => png,
                Err(error) => {
                    tracing::warn!(side, ?error, "turned drawing could not be drawn again");
                    break;
                }
            };
            update(id, |job| job.stage = Stage::Decomposing);
            let bytes = axum::body::Bytes::from(again.clone());
            let (psd, settled) =
                decompose_checked(client, &bytes, "image/png", options, &TURNED_REDOS, None)
                    .await?;
            let picture = (again.clone(), "image/png".to_string());
            let Ok(Some(next)) = judge(psd.clone(), picture.clone()).await else {
                break;
            };
            if next.badness >= best.badness {
                tracing::info!(
                    side,
                    kept = best.badness,
                    drawn = next.badness,
                    "the turn drawn again was no better; kept the one before"
                );
                continue;
            }
            tracing::info!(side, kept = best.badness, drawn = next.badness, faults = ?next.faults, "the turn drawn again keeps more; kept it");
            cache.write(&format!("turned-{index}.png"), &again).await;
            cache
                .write(&format!("decomposition-{}.psd", index + 1), &psd)
                .await;
            if settled {
                cache
                    .write(&format!("decomposition-{}.checked", index + 1), b"checked")
                    .await;
            }
            cache
                .write(&format!("turned-{index}.redrawn"), b"redrawn")
                .await;
            if !redrawn.iter().any(|known| known == side) {
                redrawn.push((*side).to_string());
            }
            decompositions[index + 1] = psd;
            pictures[index + 1] = picture;
            best = next;
        }
        judged.insert((*side).to_string(), best);
    }
    Ok(judged)
}

/// The keys JSON with each turned drawing's judgement beside the fit's report
/// (`references`), to read back what the keys were fitted to.
pub(super) fn with_judgements(keyforms: Vec<u8>, judged: &Judgements) -> Vec<u8> {
    if judged.is_empty() {
        return keyforms;
    }
    let Ok(mut keys) = serde_json::from_slice::<serde_json::Value>(&keyforms) else {
        return keyforms;
    };
    let Some(object) = keys.as_object_mut() else {
        return keyforms;
    };
    object.insert(
        "references".to_string(),
        serde_json::to_value(judged).unwrap_or_default(),
    );
    serde_json::to_vec(&keys).unwrap_or(keyforms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redraws_are_counted_from_the_marker() {
        assert_eq!(redraws_done(None), 0);
        assert_eq!(redraws_done(Some(b"2".to_vec())), 2);
        // A job's marker from before the count was kept: drawn again once.
        assert_eq!(redraws_done(Some(b"judged".to_vec())), 1);
    }

    #[test]
    fn judgements_go_beside_the_fit_report() {
        let mut judged = Judgements::new();
        judged.insert(
            "minus".to_string(),
            see_through::TurnJudgement {
                fringe: Some(1.38),
                faults: vec!["fringe".to_string()],
                badness: 0.104,
                ..Default::default()
            },
        );
        let keys = with_judgements(br#"{"fit":{}}"#.to_vec(), &judged);
        let keys: serde_json::Value = serde_json::from_slice(&keys).unwrap();
        assert_eq!(keys["references"]["minus"]["faults"][0], "fringe");
        assert_eq!(keys["fit"], serde_json::json!({}));
        assert_eq!(with_judgements(b"null".to_vec(), &judged), b"null");
    }
}
