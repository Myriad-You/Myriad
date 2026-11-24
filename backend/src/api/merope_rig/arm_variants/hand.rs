//! The hand on a drawing: the pose model looks for it, and what it finds is moved onto the portrait's pixels.
//! A look the pose model could not make is told apart from one that found no hand: only the second is the
//! drawing's fault, and only it is answered by drawing again.

use super::*;

/// A hand point the pose model is at least this sure of.
const HAND_SURE: f32 = 0.3;
/// A drawn hand shows at least this many such points.
const HAND_POINTS: usize = 5;
/// The pose model is asked about a drawing this often before it is taken as unable to look.
const LOOKS: usize = 2;

/// What the pose model found on a drawing, in the portrait's pixels: the
/// points of the hand it shows most surely (the cut seeds there), and every
/// keypoint with its score (the 3D arm is measured from them).
pub(super) struct Found {
    pub(super) hand: Vec<[f32; 2]>,
    pub(super) keypoints: Vec<[f32; 3]>,
}

/// None when no hand is drawn well enough to find; an error when the pose model could not look (`looked`).
/// `raised` names a full body's gesturing side: both its hands are drawn, and the surer may be the hanging one
/// (a bust's hanging hand is off the picture).
pub(super) async fn find_pose(
    png: &[u8],
    size: (u32, u32),
    raised: Option<&str>,
) -> ApiResult<Option<Found>> {
    let Some(pose) = looked(|| pose_estimation::estimate(png)).await? else {
        return Ok(None);
    };
    let drawn = (pose.width, pose.height);
    let hand = match raised {
        Some(side) => sided_hand(&pose.keypoints, side, drawn, size),
        None => surest_hand(&pose.keypoints, drawn, size),
    };
    Ok(hand.map(|hand| Found {
        hand,
        keypoints: in_portrait(&pose.keypoints, drawn, size),
    }))
}

/// What the pose model sees on a drawing: None when it looked and there is no figure on it to find a hand on.
/// When it could not look (its process did not start, hung or broke, its model file is missing) it is asked
/// once more about the same drawing, and failing that too the gesture ends there: the drawing is not at
/// fault, so another would be paid for, checked, and looked at no better.
async fn looked<L>(mut look: impl FnMut() -> L) -> ApiResult<Option<pose_estimation::Pose>>
where
    L: Future<Output = Result<pose_estimation::Pose, pose_estimation::PoseError>>,
{
    for attempt in 1..=LOOKS {
        match look().await {
            Ok(pose) => return Ok(Some(pose)),
            Err(pose_estimation::PoseError::Empty | pose_estimation::PoseError::Image(_)) => {
                return Ok(None);
            }
            Err(error) => {
                tracing::warn!(%error, attempt, "arm variant: the pose model could not look");
            }
        }
    }
    Err(pose_unavailable())
}

/// What a gesture ends with when the pose model could not look. The pose model's own words stay in the log:
/// they name paths on this machine.
pub(super) fn pose_unavailable() -> crate::api::merope_rig::ApiError {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "The check of the drawn hand is unavailable right now; try again shortly",
            "code": "service_unavailable"
        })),
    )
}

/// Keypoints found on a drawing `drawn` large, moved onto the portrait's `size`.
pub(super) fn in_portrait(
    keypoints: &[[f32; 3]],
    drawn: (u32, u32),
    size: (u32, u32),
) -> Vec<[f32; 3]> {
    let scale = (
        size.0 as f32 / drawn.0.max(1) as f32,
        size.1 as f32 / drawn.1.max(1) as f32,
    );
    keypoints
        .iter()
        .map(|&[x, y, score]| [x * scale.0, y * scale.1, score])
        .collect()
}

/// COCO-WholeBody's two hands are points 91–111 and 112–132.
fn surest_hand(
    keypoints: &[[f32; 3]],
    drawn: (u32, u32),
    size: (u32, u32),
) -> Option<Vec<[f32; 2]>> {
    let hands = [keypoints.get(91..112)?, keypoints.get(112..133)?];
    let sure = |hand: &[[f32; 3]]| hand.iter().map(|point| point[2]).sum::<f32>();
    let best = if sure(hands[0]) >= sure(hands[1]) {
        hands[0]
    } else {
        hands[1]
    };
    hand_on_portrait(best, drawn, size)
}

/// The hand on the picture's `side` of a figure facing the viewer: its own right (112–132) on the left.
fn sided_hand(
    keypoints: &[[f32; 3]],
    side: &str,
    drawn: (u32, u32),
    size: (u32, u32),
) -> Option<Vec<[f32; 2]>> {
    let hand = if side == "left" {
        keypoints.get(112..133)?
    } else {
        keypoints.get(91..112)?
    };
    hand_on_portrait(hand, drawn, size)
}

fn hand_on_portrait(
    best: &[[f32; 3]],
    drawn: (u32, u32),
    size: (u32, u32),
) -> Option<Vec<[f32; 2]>> {
    let scale = (
        size.0 as f32 / drawn.0.max(1) as f32,
        size.1 as f32 / drawn.1.max(1) as f32,
    );
    let points: Vec<[f32; 2]> = best
        .iter()
        .filter(|point| point[2] >= HAND_SURE)
        .map(|point| [point[0] * scale.0, point[1] * scale.1])
        .collect();
    (points.len() >= HAND_POINTS).then_some(points)
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use super::*;
    use crate::services::pose_estimation::{Pose, PoseError};

    #[test]
    fn the_surer_hand_is_taken_in_the_portrait_s_pixels() {
        let mut keypoints = vec![[0.0, 0.0, 0.0]; 133];
        for point in &mut keypoints[112..133] {
            *point = [100.0, 50.0, 0.9];
        }
        for point in &mut keypoints[91..95] {
            *point = [10.0, 10.0, 0.95];
        }
        let hand = surest_hand(&keypoints, (500, 500), (1000, 1000)).unwrap();
        assert_eq!(hand.len(), 21);
        assert_eq!(hand[0], [200.0, 100.0]);
        // Four sure points are not a hand.
        assert!(surest_hand(&keypoints[..112], (500, 500), (1000, 1000)).is_none());
        let mut few = vec![[0.0, 0.0, 0.0]; 133];
        for point in &mut few[91..95] {
            *point = [10.0, 10.0, 0.95];
        }
        assert!(surest_hand(&few, (500, 500), (1000, 1000)).is_none());
    }

    #[test]
    fn a_figure_s_gesture_hand_is_the_one_on_its_side_however_sure_the_other() {
        let mut keypoints = vec![[0.0, 0.0, 0.0]; 133];
        // Its own left hand, hanging on the picture's right, found surer than the raised one.
        for point in &mut keypoints[91..112] {
            *point = [400.0, 450.0, 0.95];
        }
        for point in &mut keypoints[112..133] {
            *point = [100.0, 50.0, 0.6];
        }
        let left = sided_hand(&keypoints, "left", (500, 500), (1000, 1000)).unwrap();
        assert_eq!(left[0], [200.0, 100.0]);
        let right = sided_hand(&keypoints, "right", (500, 500), (1000, 1000)).unwrap();
        assert_eq!(right[0], [800.0, 900.0]);
    }

    #[test]
    fn every_keypoint_is_kept_in_the_portrait_s_pixels_with_its_score() {
        let keypoints = vec![[100.0, 50.0, 0.9], [0.0, 400.0, 0.1]];
        assert_eq!(
            in_portrait(&keypoints, (500, 1000), (1000, 1000)),
            vec![[200.0, 50.0, 0.9], [0.0, 400.0, 0.1]]
        );
    }

    fn pose() -> Pose {
        Pose {
            model: pose_estimation::POSE_MODEL.to_string(),
            width: 8,
            height: 8,
            keypoints: Vec::new(),
        }
    }

    /// What `looked` makes of these answers of the pose model, and how often it asked.
    async fn looked_over(
        answers: Vec<Result<Pose, PoseError>>,
    ) -> (ApiResult<Option<Pose>>, usize) {
        let given = answers.len();
        let answers = Mutex::new(VecDeque::from(answers));
        let made = looked(|| {
            let answer = answers.lock().unwrap().pop_front();
            async move { answer.expect("asked more often than there are answers") }
        })
        .await;
        let left = answers.lock().unwrap().len();
        (made, given - left)
    }

    #[tokio::test]
    async fn a_look_that_could_not_be_made_is_made_once_more_on_the_same_drawing() {
        let hung = || PoseError::Inference("pose worker timed out".into());
        let missing = || PoseError::Model("No such file or directory".into());
        // The second look sees it: the drawing goes on to be kept as any other.
        let (made, asked) = looked_over(vec![Err(hung()), Ok(pose())]).await;
        assert_eq!((made.unwrap(), asked), (Some(pose()), 2));
        // Neither does: the gesture ends, and the pose model is not asked a third time.
        let (made, asked) = looked_over(vec![Err(missing()), Err(hung()), Ok(pose())]).await;
        let (status, Json(body)) = made.unwrap_err();
        assert_eq!((status, asked), (StatusCode::SERVICE_UNAVAILABLE, 2));
        assert_eq!(body["code"], "service_unavailable");
        // Nothing of this machine in what the page is told.
        assert!(!body.to_string().contains("No such file"));
    }

    #[tokio::test]
    async fn a_look_that_found_no_figure_is_no_failure_of_the_pose_model() {
        for nothing in [PoseError::Empty, PoseError::Image("not a picture".into())] {
            let (made, asked) = looked_over(vec![Err(nothing), Ok(pose())]).await;
            assert_eq!((made.unwrap(), asked), (None, 1));
        }
        let (made, asked) = looked_over(vec![Ok(pose())]).await;
        assert_eq!((made.unwrap(), asked), (Some(pose()), 1));
    }
}
