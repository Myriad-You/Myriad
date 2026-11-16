//! An uploaded portrait read for how it is framed, and a redraw of it checked
//! against it. The rig wants a picture square to the camera, arms down,
//! nothing held, on a plain backdrop; a picture drawn otherwise is redrawn
//! that way in its own style, and the owner sees whether the redraw kept the
//! character before taking it.

use super::vision::run_vision_call;
use super::*;

/// How an uploaded portrait is framed, and what of it the rig cannot take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortraitFraming {
    /// What kind of picture it is: only an `illustration` becomes a figure.
    pub medium: &'static str,
    pub facing: &'static str,
    pub backdrop: &'static str,
    pub arms: &'static str,
    pub holding: bool,
    /// What a redraw square to the camera would put right: `turned`,
    /// `backdrop`, `held`, `arms`. Empty when the picture can be used as it is.
    pub reasons: Vec<&'static str>,
}

/// What a redraw of a portrait kept of it, and what changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UprightCheck {
    pub same_character: bool,
    pub same_style: bool,
    pub upright: bool,
    pub differences: Vec<String>,
}

const MEDIA: [&str; 6] = [
    "illustration",
    "photo",
    "3d_render",
    "pixel_art",
    "monochrome_comic",
    "other",
];
const FACINGS: [&str; 5] = [
    "front",
    "slightly_turned",
    "three_quarter",
    "profile",
    "back",
];
const BACKDROPS: [&str; 4] = ["plain", "gradient", "pattern", "scene"];
const ARMS: [&str; 5] = ["down", "raised", "near_face", "crossed", "out_of_frame"];
const MAX_DIFFERENCES: usize = 5;
const MAX_DIFFERENCE_CHARS: usize = 60;
/// The two pictures are compared side by side at this height.
const COMPARE_HEIGHT: u32 = 1024;
const COMPARE_GAP: u32 = 24;

const FRAMING_PROMPT: &str = r#"Look at this character picture and report what kind of picture it is and how it is framed. Answer with JSON only:
{"medium": "illustration" | "photo" | "3d_render" | "pixel_art" | "monochrome_comic" | "other",
 "facing": "front" | "slightly_turned" | "three_quarter" | "profile" | "back",
 "backdrop": "plain" | "gradient" | "pattern" | "scene",
 "arms": "down" | "raised" | "near_face" | "crossed" | "out_of_frame",
 "holding": true | false}

medium — illustration: a finished drawing or painting in color, whatever its style (anime, cel shading, painterly, watercolor, semi-realistic painting). photo: a photograph, or anything photorealistic. 3d_render: a render of a 3D model or a game screenshot of one. pixel_art: drawn in visible pixels. monochrome_comic: a black-and-white or grayscale comic, sketch or line drawing without color. other: none of these.
facing — where the face and upper body point. front: square to the viewer (a slight head tilt is still front). slightly_turned: turned a little, both eyes and both cheeks still clearly visible. three_quarter: one cheek clearly foreshortened, the far eye partly hidden or at the face's edge. profile: a side view. back: seen from behind.
backdrop — what is behind the character. plain: one flat color, light or dark, or transparent. gradient: only a smooth change of color. pattern: repeated shapes, frames, text or decoration. scene: a place, furniture, objects, sky, light effects or anything else drawn behind the character.
arms — down: hanging or resting at the sides, or cut off below the crop. raised: a hand above the shoulder. near_face: a hand at the face, hair or chin. crossed: arms folded, or hands held together in front of the chest. out_of_frame: no arm is visible.
holding — true when a hand holds or carries any object (a weapon, cup, book, phone, flower, prop); false for what is worn."#;

/// Reads how an uploaded portrait is framed.
pub async fn read_portrait_framing(
    image: &ImageReference,
) -> Result<PortraitFraming, OnboardingAiError> {
    let raw = run_vision_call(FRAMING_PROMPT, image).await?;
    parse_portrait_framing(&raw)
}

pub(super) fn parse_portrait_framing(raw: &str) -> Result<PortraitFraming, OnboardingAiError> {
    let parsed = parse_json_object(raw).ok_or(OnboardingAiError::UnusableResponse(
        "portrait framing was not valid JSON",
    ))?;
    let pick = |key: &str, options: &[&'static str]| {
        let value = parsed
            .get(key)
            .and_then(Value::as_str)?
            .trim()
            .to_ascii_lowercase();
        options.iter().copied().find(|option| *option == value)
    };
    let (Some(medium), Some(facing), Some(backdrop), Some(arms)) = (
        pick("medium", &MEDIA),
        pick("facing", &FACINGS),
        pick("backdrop", &BACKDROPS),
        pick("arms", &ARMS),
    ) else {
        return Err(OnboardingAiError::UnusableResponse(
            "portrait framing named an unknown category",
        ));
    };
    let holding = parsed
        .get("holding")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut reasons = Vec::new();
    // A face turned a little still rigs; one cheek foreshortened does not.
    if matches!(facing, "three_quarter" | "profile" | "back") {
        reasons.push("turned");
    }
    // The import tells the figure from a flat backdrop only.
    if backdrop != "plain" {
        reasons.push("backdrop");
    }
    if holding {
        reasons.push("held");
    }
    if matches!(arms, "raised" | "near_face" | "crossed") {
        reasons.push("arms");
    }
    Ok(PortraitFraming {
        medium,
        facing,
        backdrop,
        arms,
        holding,
        reasons,
    })
}

/// Checks a redraw against the portrait it was redrawn from. `language` is
/// the interface's, for the notes on what changed.
pub async fn check_upright_redraw(
    language: &str,
    original: &ImageReference,
    redraw: &ImageReference,
) -> Result<UprightCheck, OnboardingAiError> {
    let pair = side_by_side(&original.bytes, &redraw.bytes).ok_or(
        OnboardingAiError::UnusableResponse("the redraw could not be compared"),
    )?;
    let pair = ImageReference::new(pair, "image/png")
        .map_err(|_| OnboardingAiError::UnusableResponse("the redraw could not be compared"))?;
    let raw = run_vision_call(&upright_check_prompt(language), &pair).await?;
    parse_upright_check(&raw)
}

fn upright_check_prompt(language: &str) -> String {
    let language = match language {
        "zh-CN" => "Simplified Chinese",
        "zh-TW" => "Traditional Chinese",
        "ja-JP" => "Japanese",
        "ko-KR" => "Korean",
        "fr-FR" => "French",
        "de-DE" => "German",
        _ => "English",
    };
    format!(
        r#"The image shows two pictures side by side. LEFT is a character illustration the owner drew or commissioned. RIGHT is a redraw of it, meant to be the same character in the same art style, square to the viewer, arms relaxed, nothing held, on a plain light backdrop. Answer with JSON only:
{{"sameCharacter": true | false, "sameStyle": true | false, "upright": true | false, "differences": ["..."]}}

sameCharacter — false when the face, eye color, hair cut or color, costume, palette or any accessory clearly changed (lost, added or recolored). Ignore what the left picture hid by its angle, its hands or its crop.
sameStyle — false when the line work, coloring or shading clearly changed, for example a painterly picture became flat cel shading, or the reverse.
upright — false when the right picture is not square to the viewer, a hand is raised or holds something, or its backdrop is not plain.
differences — up to five short notes in {language}, each under 30 characters, naming what changed from left to right; an empty list when nothing notable changed."#
    )
}

pub(super) fn parse_upright_check(raw: &str) -> Result<UprightCheck, OnboardingAiError> {
    let parsed = parse_json_object(raw).ok_or(OnboardingAiError::UnusableResponse(
        "redraw check was not valid JSON",
    ))?;
    let flag = |key: &str| {
        parsed
            .get(key)
            .and_then(Value::as_bool)
            .ok_or(OnboardingAiError::UnusableResponse(
                "redraw check left out a verdict",
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
    Ok(UprightCheck {
        same_character: flag("sameCharacter")?,
        same_style: flag("sameStyle")?,
        upright: flag("upright")?,
        differences,
    })
}

/// The two pictures at one height, left and right, on white: one image any
/// vision provider takes.
pub(super) fn side_by_side(left: &[u8], right: &[u8]) -> Option<Vec<u8>> {
    use image::{GenericImage, ImageFormat, Rgba, RgbaImage, imageops::FilterType};
    let fit = |bytes: &[u8]| {
        let picture = image::load_from_memory(bytes).ok()?;
        let width = (picture.width() as u64 * COMPARE_HEIGHT as u64
            / picture.height().max(1) as u64)
            .clamp(1, 4 * COMPARE_HEIGHT as u64) as u32;
        Some(
            picture
                .resize_exact(width, COMPARE_HEIGHT, FilterType::Triangle)
                .to_rgba8(),
        )
    };
    let (left, right) = (fit(left)?, fit(right)?);
    let mut canvas = RgbaImage::from_pixel(
        left.width() + COMPARE_GAP + right.width(),
        COMPARE_HEIGHT,
        Rgba([255, 255, 255, 255]),
    );
    canvas.copy_from(&left, 0, 0).ok()?;
    canvas
        .copy_from(&right, left.width() + COMPARE_GAP, 0)
        .ok()?;
    let mut out = std::io::Cursor::new(Vec::new());
    canvas.write_to(&mut out, ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turned_or_busy_picture_names_what_a_redraw_puts_right() {
        let framing = parse_portrait_framing(
            r#"{"medium":"illustration","facing":"three_quarter","backdrop":"scene","arms":"near_face","holding":true}"#,
        )
        .unwrap();
        assert_eq!(framing.reasons, vec!["turned", "backdrop", "held", "arms"]);
        let ready = parse_portrait_framing(
            "```json\n{\"medium\":\"illustration\",\"facing\":\"slightly_turned\",\"backdrop\":\"plain\",\"arms\":\"down\",\"holding\":false}\n```",
        )
        .unwrap();
        assert!(ready.reasons.is_empty(), "{ready:?}");
        let photo = parse_portrait_framing(
            r#"{"medium":"photo","facing":"front","backdrop":"plain","arms":"down","holding":false}"#,
        )
        .unwrap();
        assert_eq!(photo.medium, "photo");
        assert!(
            parse_portrait_framing(r#"{"facing":"sideways","backdrop":"plain","arms":"down"}"#)
                .is_err()
        );
    }

    #[test]
    fn the_redraw_check_keeps_its_verdicts_and_short_notes() {
        let check = parse_upright_check(
            r#"{"sameCharacter":true,"sameStyle":false,"upright":true,"differences":["发饰从左边移到右边","  ","线条变细","a","b","c","d"]}"#,
        )
        .unwrap();
        assert!(check.same_character && !check.same_style && check.upright);
        assert_eq!(check.differences.len(), MAX_DIFFERENCES);
        assert_eq!(check.differences[0], "发饰从左边移到右边");
        assert!(parse_upright_check(r#"{"sameCharacter":true,"upright":true}"#).is_err());
    }

    #[test]
    fn two_pictures_are_put_side_by_side_at_one_height() {
        let encode = |width: u32, height: u32| {
            let mut out = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]))
                .write_to(&mut out, image::ImageFormat::Png)
                .unwrap();
            out.into_inner()
        };
        let pair = side_by_side(&encode(300, 400), &encode(600, 400)).unwrap();
        let pair = image::load_from_memory(&pair).unwrap();
        assert_eq!(pair.height(), COMPARE_HEIGHT);
        assert_eq!(pair.width(), 768 + COMPARE_GAP + 1536);
        assert!(side_by_side(b"not an image", &encode(10, 10)).is_none());
    }
}
