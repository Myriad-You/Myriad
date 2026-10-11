//! The turned references judged before they are fitted (the Space's
//! `judge_turn`): a drawing that redrew what a turn keeps (its fringe, the
//! head's size, the body) is drawn once more, and the better of the two kept.

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

/// Judges the four turned drawings against the front one, draws a faulty one
/// once more (decomposed and judged in turn) and keeps whichever is judged
/// better. `decompositions` and `pictures` are the front's and the four
/// turns', in `TURN_SIDES` order after the front; replaced drawings go into
/// the cache and `redrawn`.
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
        let first = match judge(
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
        if first.faults.is_empty() || cache.read(&marker).await.is_some() {
            judged.insert((*side).to_string(), first);
            continue;
        }
        cache.write(&marker, b"judged").await;
        tracing::info!(side, faults = ?first.faults, fringe = ?first.fringe, head = ?first.head, "turned drawing redrew what a turn keeps; drawing it again");
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
                judged.insert((*side).to_string(), first);
                continue;
            }
        };
        update(id, |job| job.stage = Stage::Decomposing);
        let bytes = axum::body::Bytes::from(again.clone());
        let (psd, settled) =
            decompose_checked(client, &bytes, "image/png", options, &TURNED_REDOS, None).await?;
        let picture = (again.clone(), "image/png".to_string());
        let second = match judge(psd.clone(), picture.clone()).await {
            Ok(Some(judgement)) => judgement,
            _ => {
                judged.insert((*side).to_string(), first);
                continue;
            }
        };
        if second.badness >= first.badness {
            tracing::info!(
                side,
                first = first.badness,
                second = second.badness,
                "the turn drawn again was no better; kept the first"
            );
            judged.insert((*side).to_string(), first);
            continue;
        }
        tracing::info!(side, first = first.badness, second = second.badness, faults = ?second.faults, "the turn drawn again keeps more; kept it");
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
        judged.insert((*side).to_string(), second);
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
