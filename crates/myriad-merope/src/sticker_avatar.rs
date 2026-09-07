//! 贴纸头像（sticker avatar）的提示词与输入契约。
//!
//! 主立绘那套 `MEROPE_VISUAL_SCHOOL` 明确写着 non-chibi，所以这里不复用它——
//! Q 版是另一种造型语言，混用只会让两边互相拉扯。身份不靠文字复述，靠把已确认
//! 的主立绘当身份锚附上去；文字只补那些在重绘中最容易漂掉的颜色与标志物。

use serde_json::{json, Value};

/// 造型语言或参考图角色变了就 bump。它进指纹，旧头像不会被当成新契约的产物。
pub const STICKER_AVATAR_CONTRACT_VERSION: &str = "logo-sticker-chibi-v3";

/// 正方形。头像位全是圆形或方形裁切，非 1:1 一定被裁掉耳朵或发梢。
pub const STICKER_AVATAR_SIZE: u32 = 1024;

/// 项目 logo 本身就是这套贴纸风格的样板，后端按这个摘要绑定那份字节。
pub const MEROPE_STICKER_STYLE_REFERENCE_SHA256: &str =
    "1015f09392ee01823ce6dc1a78521d04bb734d0585864e7668f380420a4b106f";

/// 从身份里挑进提示词的字段。只留重绘时最容易漂的那几项——
/// 脸型和体型在 Q 版里本来就要重新概括，写进去反而打架。领口和上装会把模型
/// 往胸像上拉，头像只要头，所以也不带衣服结构。
const STICKER_IDENTITY_FIELDS: &[(&str, &str)] = &[
    ("hairShape", "Hair color and cut"),
    ("eyeDesign", "Eye color and acting"),
    ("heroAccessory", "Signature head accessories to keep"),
    ("paletteHint", "Palette"),
    ("motif", "Motif"),
];

const STICKER_AVATAR_SCHOOL: &str = "One die-cut chibi sticker of a single character, drawn in the same souvenir-sticker language as the attached style reference: a floating Q-style super-deformed head that fills most of the square, large glossy jewel eyes with layered irises and bright catchlights, tiny simplified nose and mouth, soft round cheeks with a light blush, hair built from a few broad tapered ribbon masses with clean overlaps and broad specular bands, thin colored linework, pastel high-key palette, and clean cel-to-gradient shading with a luminous finish.";

/// 头像位要的是一张头，不是胸像。肩、颈、领口一旦写进画面，圆形裁切里就会
/// 剩下一截身子。头饰可以留，身子不行。
const STICKER_AVATAR_FRAMING: &str = "Head only. Draw the complete head and hair, including ears and any hair clips, earrings, or other ornaments that sit on the head. Do not draw a neck, shoulders, collarbone, chest, torso, arms, hands, collar, garment, or any other body. The sticker is a floating head, not a bust, not a chibi figure, and not a head-and-shoulder crop. Ignore any accessory that would need a torso to exist.";

const STICKER_AVATAR_CUT: &str = "Finish it as a physical die-cut sticker: one thick uniform white cut border tracing the whole head-and-hair silhouette including hair tips and head-worn accessories, a soft narrow drop shadow just outside that border, and nothing else. The head plus its white border must sit fully inside the square with even margins on all four sides; do not crop the hair, ears, or accessories at the edge. The area outside the white border is fully transparent — no backdrop, no card, no frame, no text, no watermark, no signature, no second character, no held props, no body, no ground shadow beyond the sticker's own.";

const STICKER_AVATAR_ANCHOR: &str = "The attached portrait is the immutable anchor for who this character is — not for how they are posed or framed. Keep the same person: hair color and cut, eye color, skin tone, and signature head-worn ornaments all carry over unchanged. Restyle the proportions into a chibi head and warm the expression into a small friendly smile; do not redesign the identity, do not age the character up or down, and do not invent a neck, body, collar, or outfit under the head.";

/// 主立绘是 zero-yaw 严格正视的参考视图，那是为了给骨骼编译当底图。贴纸没有
/// 这个负担：不明说姿态自由，模型会连正视一起抄，画出一个僵着的正脸 Q 版。
const STICKER_AVATAR_POSE: &str = "Do not copy the reference photo's locked square-frontal camera. Pose the chibi head the way a character-head sticker is posed: a small head tilt and a gentle three-quarter turn of the face are welcome. Keep both eyes visible and readable, and keep the face turned enough toward the viewer to stay friendly. Pick one lively head pose rather than a neutral reference stance. There is no shoulder line to square up because there is no body.";

const STICKER_AVATAR_READABILITY: &str = "It will be shown as small as 32 pixels across: keep one clear silhouette, high value contrast between hair and face, no thin floating details that vanish when downscaled, and no fine text-like ornament.";

fn bounded_text(value: &str, max_chars: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

fn identity_line(identity: &Value, key: &str, label: &str) -> Option<String> {
    let value = bounded_text(identity.get(key)?.as_str()?, 400);
    if value.is_empty() {
        return None;
    }
    Some(format!("{label}: {value}"))
}

/// 头像提示词。`visual_profile` 是人设那份完整视觉设定，取的是它归一化后的身份。
pub fn build_sticker_avatar_prompt(name: &str, visual_profile: &Value) -> String {
    let identity = crate::visual_prompt::normalize_visual_identity_for_prompt(visual_profile)
        .and_then(|identity| crate::visual_design::flatten_visual_identity(&identity))
        .unwrap_or(Value::Null);

    let mut parts = vec![
        STICKER_AVATAR_SCHOOL.to_string(),
        STICKER_AVATAR_FRAMING.to_string(),
        STICKER_AVATAR_ANCHOR.to_string(),
        STICKER_AVATAR_POSE.to_string(),
        STICKER_AVATAR_CUT.to_string(),
    ];
    let name = bounded_text(name, 50);
    if !name.is_empty() {
        parts.push(format!("Identity name: {name}."));
    }
    let carried: Vec<String> = STICKER_IDENTITY_FIELDS
        .iter()
        .filter_map(|(key, label)| identity_line(&identity, key, label))
        .collect();
    if !carried.is_empty() {
        parts.push(format!(
            "Identity facts that must survive the restyle:\n{}",
            carried.join("\n")
        ));
    }
    parts.push(STICKER_AVATAR_READABILITY.to_string());
    parts.join("\n\n")
}

/// 输入快照。像素由 URL 标识，这份契约标识那些像素本来该画的是什么。
///
/// `sourcePortraitAssetId` 是血统锚：主立绘换了，旧头像就不是这个人了。
pub fn build_sticker_avatar_contract(
    name: &str,
    visual_profile: &Value,
    source_portrait_asset_id: &str,
) -> Value {
    json!({
        "contractVersion": STICKER_AVATAR_CONTRACT_VERSION,
        "slot": "sticker-avatar",
        "identity": {
            "name": bounded_text(name, 50),
            "visualProfile": crate::visual_contract::appearance_visual_profile(visual_profile),
        },
        "source": {
            "portraitAssetId": bounded_text(source_portrait_asset_id, 512),
            "portraitRole": "identity-anchor",
        },
        "output": {
            "width": STICKER_AVATAR_SIZE,
            "height": STICKER_AVATAR_SIZE,
            "framing": "chibi-head-only",
            "view": "free-sticker-pose",
            "background": "transparent",
            "cut": "white-die-cut-border",
        },
        "rendering": {
            "styleReferenceSha256": MEROPE_STICKER_STYLE_REFERENCE_SHA256,
            "styleReferenceRole": "sticker-cut-and-chibi-proportions",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Value {
        json!({
            "gender": "female",
            "language": "zh-CN",
            "visualIdentity": {
                "hairShape": "短鲍伯，粉转薰衣草渐变",
                "eyeDesign": "紫罗兰色大眼，柔和上睫",
                "heroAccessory": "星形发夹与鸟笼耳坠",
                "paletteHint": "主色薰衣草，辅色白，点缀金",
                "motif": "星与鸟笼",
                "outfitConstruction": "水手领上衣配蝴蝶结",
                "faceDesign": "女性化的圆润下颌",
                "upperBodySilhouette": "合身上身",
                "hairLayerPlan": "三层",
                "sleeveArmDesign": "长袖",
                "materialPlan": "棉与缎"
            }
        })
    }

    #[test]
    fn prompt_carries_the_die_cut_and_the_identity_anchor() {
        let prompt = build_sticker_avatar_prompt("Arael", &profile());
        assert!(prompt.contains("die-cut"));
        assert!(prompt.contains("transparent"));
        assert!(prompt.contains("immutable anchor for who this character is"));
        assert!(prompt.contains("Identity name: Arael."));
    }

    /// 头像只要头。肩、颈、领口一旦写进提示词，模型就会画出一截身子。
    #[test]
    fn prompt_asks_for_a_floating_head_and_forbids_the_body() {
        let prompt = build_sticker_avatar_prompt("Arael", &profile());
        assert!(prompt.contains("Head only"));
        assert!(prompt.contains("floating head"));
        assert!(prompt.contains("Do not draw a neck, shoulders, collarbone, chest, torso"));
        assert!(prompt.contains("There is no shoulder line to square up because there is no body"));
        for leaked in [
            "shoulder sliver",
            "neck and shoulder",
            "asymmetric shoulder",
            "head or shoulders",
            "collar shape",
            "水手领上衣配蝴蝶结",
        ] {
            assert!(
                !prompt.contains(leaked),
                "body/bust cue leaked into the sticker prompt: {leaked}"
            );
        }
        let contract = build_sticker_avatar_contract("Arael", &profile(), "/uploads/a.png");
        assert_eq!(contract["output"]["framing"], "chibi-head-only");
    }

    /// Q 版是这份契约的全部意义。主立绘那边禁 chibi，两份提示词不能串味。
    #[test]
    fn prompt_asks_for_chibi_rather_than_the_master_portrait_school() {
        let prompt = build_sticker_avatar_prompt("Arael", &profile());
        assert!(prompt.contains("chibi"));
        assert!(
            !prompt.contains(crate::visual_prompt::MEROPE_VISUAL_SCHOOL),
            "sticker prompt must not inherit the non-chibi master school"
        );
    }

    #[test]
    fn prompt_carries_the_colors_that_drift_and_drops_the_ones_that_fight_chibi() {
        let prompt = build_sticker_avatar_prompt("Arael", &profile());
        assert!(prompt.contains("短鲍伯，粉转薰衣草渐变"));
        assert!(prompt.contains("星形发夹与鸟笼耳坠"));
        // 体型与脸型在 Q 版里要重新概括，抄过来只会和 chibi 打架。
        assert!(!prompt.contains("合身上身"));
        assert!(!prompt.contains("女性化的圆润下颌"));
    }

    /// 身份锚不是姿态锚。主立绘那张是 zero-yaw 严格正视，不明说姿态自由，
    /// 模型会把正视一起抄过来，画出个僵着的正脸 Q 版。
    #[test]
    fn prompt_anchors_the_identity_without_anchoring_the_pose() {
        let prompt = build_sticker_avatar_prompt("Arael", &profile());
        assert!(prompt.contains("anchor for who this character is"));
        assert!(prompt.contains("Do not copy the reference photo\'s locked square-frontal camera"));
        assert!(prompt.contains("three-quarter turn"));
        // 主立绘那套正视锁死的措辞一个字都不能漏进来。
        for locked in [
            "zero head yaw",
            "strict centered eye-level frontal",
            "square to the camera",
            "shoulders are level",
        ] {
            assert!(
                !prompt.contains(locked),
                "master-portrait framing lock leaked into the sticker prompt: {locked}"
            );
        }
    }

    #[test]
    fn prompt_survives_a_persona_without_a_confirmed_visual_identity() {
        let prompt = build_sticker_avatar_prompt("", &json!({}));
        assert!(prompt.contains("die-cut"));
        assert!(!prompt.contains("Identity name"));
        assert!(!prompt.contains("Identity facts"));
    }

    /// 主立绘换了就是换了个人。指纹必须跟着动，否则旧头像会被当成还配套。
    #[test]
    fn contract_fingerprint_follows_the_source_portrait() {
        let first = build_sticker_avatar_contract("Arael", &profile(), "/uploads/a.png");
        let second = build_sticker_avatar_contract("Arael", &profile(), "/uploads/b.png");
        assert_ne!(
            crate::visual_contract::character_asset_contract_fingerprint(&first),
            crate::visual_contract::character_asset_contract_fingerprint(&second)
        );
    }

    #[test]
    fn contract_fingerprint_follows_the_confirmed_appearance() {
        let base = build_sticker_avatar_contract("Arael", &profile(), "/uploads/a.png");
        let mut changed = profile();
        changed["visualIdentity"]["hairShape"] = json!("长直发，黑色");
        let after = build_sticker_avatar_contract("Arael", &changed, "/uploads/a.png");
        assert_ne!(
            crate::visual_contract::character_asset_contract_fingerprint(&base),
            crate::visual_contract::character_asset_contract_fingerprint(&after)
        );
    }
}
