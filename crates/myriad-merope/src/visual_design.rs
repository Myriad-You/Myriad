use serde_json::{json, Map, Value};

/// Required visual-identity fields for a close upper-body Merope portrait.
/// Lower-body garments, footwear, and articulated limb design intentionally do
/// not belong to this contract.
pub const CLOTHING_STYLES: [&str; 17] = [
    "everyday",
    "uniform",
    "fantasy",
    "urban",
    "east-asian",
    "japanese",
    "sci-fi",
    "formal",
    "sport",
    "idol",
    "gothic",
    "lounge",
    "royal",
    "mystic",
    "travel",
    "vintage",
    "rain",
];

pub const CHARACTER_VISUAL_FIELDS: [(&str, usize); 4] = [
    ("faceDesign", 260),
    ("eyeDesign", 360),
    ("hairShape", 300),
    ("hairLayerPlan", 380),
];

pub const OUTFIT_VISUAL_FIELDS: [(&str, usize); 7] = [
    ("upperBodySilhouette", 320),
    ("outfitConstruction", 520),
    ("sleeveArmDesign", 360),
    ("materialPlan", 420),
    ("heroAccessory", 420),
    ("paletteHint", 340),
    ("motif", 280),
];

/// Flat field list for prompts and language checks. Order is character then outfit.
pub const UPPER_BODY_VISUAL_IDENTITY_FIELDS: [(&str, usize); 11] = [
    CHARACTER_VISUAL_FIELDS[0],
    CHARACTER_VISUAL_FIELDS[1],
    CHARACTER_VISUAL_FIELDS[2],
    CHARACTER_VISUAL_FIELDS[3],
    OUTFIT_VISUAL_FIELDS[0],
    OUTFIT_VISUAL_FIELDS[1],
    OUTFIT_VISUAL_FIELDS[2],
    OUTFIT_VISUAL_FIELDS[3],
    OUTFIT_VISUAL_FIELDS[4],
    OUTFIT_VISUAL_FIELDS[5],
    OUTFIT_VISUAL_FIELDS[6],
];

pub fn normalize_clothing_style(raw: &str) -> Option<&'static str> {
    let value = raw.trim();
    CLOTHING_STYLES.iter().copied().find(|id| *id == value)
}

/// Stable costume grammar for the design model. Language must not substitute this.
pub fn clothing_style_grammar(id: &str) -> Option<&'static str> {
    Some(match normalize_clothing_style(id)? {
        "everyday" => {
            "Contemporary casual wear built around one changing upper-body silhouette driver: a wrapped layer, cropped overshirt, shaped knit panel, relaxed vest, or utility yoke. Combine familiar cloth construction with one crisp color-block interruption and a small practical fastening. Keep an open neckline so the neck from jaw to collarbone stays uncovered. Palette: open. Owner extras and persona may pick any readable game-production palette."
        }
        "uniform" => {
            "An original academy or service uniform with disciplined repeating trim and one strong structural driver such as a split yoke, offset tabard, short mantle, fitted vest, or layered overshirt. Make the open neckline, closure, and shoulder line belong to the same invented institution. Never cover the neck with a closed high neckline. Palette: institutional dyes such as navy, bottle green, charcoal, and cream, plus one school or service accent. Not festival red-gold, gothic mourning black-wine, or idol holographic candy."
        }
        "fantasy" => {
            "Original fantasy-adventure wear combining a practical inner layer with one silhouette-changing outer device such as a split mantle, plated shoulder petal, wrapped harness panel, floating oversleeve, or open sculpted neckline. Integrate the hero ornament into a clasp, hinge, chain, or frame. The neck stays fully visible. Palette: default weathered adventure dyes, travel leather, muted metal, and one magical jewel accent. Owner extras may override this default."
        }
        "urban" => {
            "Contemporary city wear or smart streetwear driven by an offset lapel, modular shoulder panel, cropped technical layer, diagonal placket, or open hood resting off the neck. Use purposeful hardware and graphic color blocking with a clean everyday fit. Keep the neck unobstructed. Palette: open. Owner extras and persona may pick any readable game-production palette."
        }
        "east-asian" => {
            "Chinese-inspired layered traditional or modern-hanfu fusion selected explicitly by the owner. Vary the upper-body skeleton through open overlapping lapels, cloud-shoulder geometry, sleeveless beizi layers, or wrapped short jackets that leave the neck uncovered, with culturally coherent closures and trim. Palette: ink, cinnabar, lacquer, jade, tea, indigo, bone, and gold thread. Not idol candy neon, holographic pastel, or cyber magenta-cyan."
        }
        "japanese" => {
            "Japanese-inspired traditional, shrine, or modern-wa fusion selected explicitly by the owner. Build a fresh upper-body silhouette from open haori lapels, kosode wrapping that stays off the neck, obi-linked upper structures, or modern tailored wa details with coherent cords and fastenings. Do not stack closed inner collars over the neck. Palette: indigo, shrine vermilion, unbleached linen, black, gold, muted moss or restrained sakura. Not cyber neon or a full European mourning lace kit."
        }
        "sci-fi" => {
            "Science-fiction or futurist wear built from soft technical garments plus one dominant engineered structure: an open engineered neckline, asymmetric interface panel, segmented shoulder shell, tension harness, or translucent data layer. Use restrained luminous accents as part of seams and closures. Leave the neck uncovered. Palette: graphite, ice, gunmetal, and one luminous accent such as cyan, white, or pale violet. Not mineral 国风 red-gold-jade, not cute stage candy."
        }
        "formal" => {
            "Contemporary formalwear or evening tailoring centered on one designed line: asymmetric lapel, sculpted drape, corseted waist panel, cape sleeve, open architectural neckline, or layered waistcoat. Let precise tailoring, restrained jewelry, and material contrast carry the focal hierarchy. The neck stays visible. Palette: default black, ivory, champagne, one deep evening jewel, and restrained metal. Owner extras may override this default."
        }
        "sport" => {
            "Athletic or outdoor sport topwear with functional paneling, ventilation zones, compression or shell layering, and one silhouette driver such as an offset wind guard, climbing yoke, protective shoulder cap, or wrap closure. Turn the motif into seam rhythm and hardware rather than a printed brand. Use an open sport neckline that does not cover the neck. Palette: open. Owner extras and persona may pick any readable game-production palette."
        }
        "royal" => {
            "An original fictional court or royal ceremonial costume with controlled hierarchy: open ceremonial neckline or mantle that leaves the neck uncovered, tailored inner coat, one asymmetrical sash or shoulder structure, dimensional insignia hardware, and restrained precious trim. Invent a coherent court language rather than copying a real dynasty. Palette: deep jewel, ivory, gold or silver metal, and one heraldic accent. Not stage-candy pastels, street camo, or a copied real-dynasty flag set."
        }
        "idol" => {
            "Original live-stage performance wear with one strong upper-body silhouette driver: a shoulder fan, ribbon-panel capelet, structured peplum, split oversleeve, open stage neckline, or asymmetric stage drape. Use rhythmic color-block planes, movement-ready layering, and one dimensional seam-anchored hero ornament so the costume reads clearly under stage light. Keep the neck uncovered. Palette: high-chroma stage candy, white or black contrast, and a holographic or metallic accent that reads under lights. Never a 国风 mineral set of cinnabar, imperial yellow, jade, and ink-wash, and never a whole-costume gothic mourning black-wine."
        }
        "gothic" => {
            "Contemporary gothic or dark-romantic wear shaped by an open architectural neckline, corset-derived panel, split lace oversleeve, short mourning cape, or asymmetric ruffle cascade. Balance dark fabric masses with one jewel tone and dimensional metal or enamel hardware. Never hide the neck under a high collar. Palette: black, charcoal, bone, one jewel such as wine, amethyst, or emerald, and oxidized silver. Not idol candy pastel, not bright 国风 red-gold-jade."
        }
        "lounge" => {
            "Soft indoor loungewear or knit home clothes using enveloping but designed layers: a wrapped knit, open knit neckline, quilted shoulder panel, loose open henley, or soft cropped robe. Create identity through knit direction, piping, pocket or tie construction, and one tactile accessory. The neck stays uncovered. Palette: open. Owner extras and persona may pick any readable game-production palette."
        }
        "mystic" => {
            "An original fictional mystic or ritual costume organized around one readable apparatus: orbiting shoulder frame that leaves the neck uncovered, layered stole, geometric shoulder veil, talisman harness, or split ceremonial oversleeve. Integrate symbols into cutouts, clasps, chains, and borders without borrowing a real faith's vestments. Palette: dusk violet, bone, tarnished gold, ink, and one sigil accent. Not sports neon, not cute idol rainbow."
        }
        "travel" => {
            "Layered traveler or expedition outerwear with a practical inner layer and one silhouette-changing weather or carrying system: map-pocket yoke, short storm cape, crossed strap frame, open storm flap, or reinforced shoulder wrap. Keep fastenings and accessories usable and geographically neutral. Do not wrap a scarf or collar over the neck. Palette: open. Owner extras and persona may pick any readable game-production palette."
        }
        "vintage" => {
            "Vintage or retro mid-century civilian wear built from era-aware tailoring, knit, pleat, piping, and button rhythm. Vary the silhouette through a shaped bolero, diagonal blouse drape, fitted waistcoat, open period neckline, or short cape sleeve, then add one period-coherent dimensional accessory. Keep the neck visible. Palette: open. Owner extras and persona may pick any readable game-production palette."
        }
        "rain" => {
            "Raincoat or trench-family outerwear using waterproof layering, sealed closures, and a changing weather silhouette such as an asymmetric storm shield, translucent shoulder cape, open trench neckline, belted wrap panel, or modular cuff guard. Make reflective and translucent details follow construction seams. Never fold a hood or high collar over the neck. Palette: open. Owner extras and persona may pick any readable game-production palette."
        }
        _ => return None,
    })
}

pub fn sanitize_upper_body_visual_identity(value: &Value) -> Option<Value> {
    sanitize_modular_identity(value).or_else(|| wrap_flat_identity(value))
}

pub fn upper_body_visual_identity_is_complete(value: &Value) -> bool {
    sanitize_upper_body_visual_identity(value).is_some()
}

pub fn flatten_visual_identity(value: &Value) -> Option<Value> {
    let modular = sanitize_upper_body_visual_identity(value)?;
    let mut flat = Map::new();
    copy_module_fields(
        &mut flat,
        modular.get("character")?.as_object()?,
        &CHARACTER_VISUAL_FIELDS,
    )?;
    copy_module_fields(
        &mut flat,
        modular.get("outfit")?.as_object()?,
        &OUTFIT_VISUAL_FIELDS,
    )?;
    Some(Value::Object(flat))
}

pub fn character_module(value: &Value) -> Option<Value> {
    sanitize_upper_body_visual_identity(value)?
        .get("character")
        .cloned()
}

pub fn clothing_style_of(value: &Value) -> Option<&'static str> {
    let root = value.get("visualIdentity").unwrap_or(value);
    [
        value.get("clothingStyle"),
        root.get("clothingStyle"),
        root.get("outfit")
            .and_then(|outfit| outfit.get("clothingStyle")),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str)
    .and_then(normalize_clothing_style)
}

pub fn stamp_clothing_style(identity: &mut Value, style: &str) -> Option<&'static str> {
    let style = normalize_clothing_style(style)?;
    identity
        .get_mut("outfit")
        .and_then(Value::as_object_mut)?
        .insert("clothingStyle".into(), json!(style));
    Some(style)
}

fn sanitize_modular_identity(value: &Value) -> Option<Value> {
    let root = value.get("visualIdentity").unwrap_or(value);
    let character = sanitize_fields(root.get("character")?, &CHARACTER_VISUAL_FIELDS)?;
    let mut outfit = sanitize_fields(root.get("outfit")?, &OUTFIT_VISUAL_FIELDS)?;
    if let Some(style) = root
        .get("outfit")
        .and_then(|outfit| outfit.get("clothingStyle"))
        .and_then(Value::as_str)
        .and_then(normalize_clothing_style)
    {
        outfit
            .as_object_mut()?
            .insert("clothingStyle".into(), json!(style));
    }
    Some(json!({
        "character": character,
        "outfit": outfit,
    }))
}

fn wrap_flat_identity(value: &Value) -> Option<Value> {
    let source = value.get("visualIdentity").unwrap_or(value);
    let character = sanitize_fields(source, &CHARACTER_VISUAL_FIELDS)?;
    let mut outfit = sanitize_fields(source, &OUTFIT_VISUAL_FIELDS)?;
    if let Some(style) = source
        .get("clothingStyle")
        .and_then(Value::as_str)
        .and_then(normalize_clothing_style)
        .or_else(|| {
            value
                .get("clothingStyle")
                .and_then(Value::as_str)
                .and_then(normalize_clothing_style)
        })
    {
        outfit
            .as_object_mut()?
            .insert("clothingStyle".into(), json!(style));
    }
    Some(json!({
        "character": character,
        "outfit": outfit,
    }))
}

fn sanitize_fields(source: &Value, fields: &[(&str, usize)]) -> Option<Value> {
    let source = source.as_object()?;
    let mut sanitized = Map::new();
    for (key, max_chars) in fields {
        let raw = source.get(*key)?.as_str()?.trim();
        if raw.is_empty() || raw.chars().count() > *max_chars || raw.chars().any(char::is_control) {
            return None;
        }
        sanitized.insert((*key).to_string(), Value::String(raw.to_string()));
    }
    Some(Value::Object(sanitized))
}

fn copy_module_fields(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    fields: &[(&str, usize)],
) -> Option<()> {
    for (key, _) in fields {
        target.insert((*key).to_string(), source.get(*key)?.clone());
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn complete_flat() -> Value {
        json!({
            "visualIdentity": {
                "faceDesign": "柔和的鹅蛋脸，鼻唇简洁，面部比例成熟而非幼态",
                "eyeDesign": "紫蓝宝石感大眼，深色上睫与多层虹膜高光",
                "hairShape": "粉色齐颌短发，空气刘海，侧发包住脸颊",
                "hairLayerPlan": "后发形成完整轮廓，前刘海、左右侧发和顶部呆毛可分层",
                "upperBodySilhouette": "窄肩与清晰领口，胸像轮廓紧凑，左右袖片伸入画面",
                "outfitConstruction": "水手领内搭叠短外套，领巾形成胸前主形，结构止于高腰",
                "sleeveArmDesign": "宽松袖口包住局部前臂，左右形状不完全对称，手可以不出现",
                "materialPlan": "哑光布料为主，丝带带柔和光泽，金属与宝石只用于小面积焦点",
                "heroAccessory": "左侧星形发夹与胸前星形扣形成一次呼应",
                "paletteHint": "粉色头发，淡紫与白为主体，深紫压边，少量金色点缀",
                "motif": "星轨与小型鸟笼，集中在发饰和胸前，不铺满服装"
            }
        })
    }

    #[test]
    fn complete_upper_body_design_is_normalized() {
        let sanitized = sanitize_upper_body_visual_identity(&complete_flat()).unwrap();
        assert!(sanitized.get("character").is_some());
        assert!(sanitized.get("outfit").is_some());
        assert!(sanitized.get("footwear").is_none());
        assert!(upper_body_visual_identity_is_complete(&sanitized));
        let flat = flatten_visual_identity(&sanitized).unwrap();
        assert_eq!(flat.as_object().unwrap().len(), 11);
        assert_eq!(flat["hairShape"], "粉色齐颌短发，空气刘海，侧发包住脸颊");
        assert_eq!(
            flat["outfitConstruction"],
            "水手领内搭叠短外套，领巾形成胸前主形，结构止于高腰"
        );
    }

    #[test]
    fn modular_identity_round_trips() {
        let modular = json!({
            "character": complete_flat()["visualIdentity"].as_object().unwrap()
                .iter()
                .filter(|(key, _)| CHARACTER_VISUAL_FIELDS.iter().any(|(field, _)| field == key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Map<_, _>>(),
            "outfit": {
                "clothingStyle": "everyday",
                "upperBodySilhouette": complete_flat()["visualIdentity"]["upperBodySilhouette"],
                "outfitConstruction": complete_flat()["visualIdentity"]["outfitConstruction"],
                "sleeveArmDesign": complete_flat()["visualIdentity"]["sleeveArmDesign"],
                "materialPlan": complete_flat()["visualIdentity"]["materialPlan"],
                "heroAccessory": complete_flat()["visualIdentity"]["heroAccessory"],
                "paletteHint": complete_flat()["visualIdentity"]["paletteHint"],
                "motif": complete_flat()["visualIdentity"]["motif"]
            }
        });
        let sanitized = sanitize_upper_body_visual_identity(&modular).unwrap();
        assert_eq!(sanitized["outfit"]["clothingStyle"], "everyday");
        assert_eq!(
            flatten_visual_identity(&modular).unwrap()["faceDesign"],
            complete_flat()["visualIdentity"]["faceDesign"]
        );
    }

    #[test]
    fn missing_or_control_text_rejects_the_design() {
        let mut missing = complete_flat();
        missing["visualIdentity"]
            .as_object_mut()
            .unwrap()
            .remove("eyeDesign");
        assert!(!upper_body_visual_identity_is_complete(&missing));
        let mut control = complete_flat();
        control["visualIdentity"]["motif"] = json!("星轨\u{0000}");
        assert!(!upper_body_visual_identity_is_complete(&control));
    }

    #[test]
    fn clothing_style_is_an_explicit_opt_in() {
        assert_eq!(normalize_clothing_style(" fantasy "), Some("fantasy"));
        assert_eq!(normalize_clothing_style("idol"), Some("idol"));
        assert_eq!(normalize_clothing_style("国风"), None);
        assert_eq!(normalize_clothing_style("zh-CN"), None);
        let grammar = clothing_style_grammar("everyday").unwrap();
        assert!(grammar.contains("silhouette driver"));
        assert!(clothing_style_grammar("east-asian")
            .unwrap()
            .contains("selected explicitly by the owner"));
        assert!(clothing_style_grammar("royal")
            .unwrap()
            .contains("coherent court language"));
        assert!(clothing_style_grammar("idol")
            .unwrap()
            .contains("live-stage performance wear"));
        assert!(clothing_style_grammar("idol")
            .unwrap()
            .contains("dimensional seam-anchored hero ornament"));
        assert!(clothing_style_grammar("uniform")
            .unwrap()
            .contains("strong structural driver"));
        assert!(clothing_style_grammar("rain")
            .unwrap()
            .contains("weather silhouette"));
        for id in CLOTHING_STYLES {
            let grammar = clothing_style_grammar(id).unwrap();
            assert!(
                !grammar.contains("standing collar")
                    && !grammar.contains("turtleneck")
                    && !grammar.contains("shawl collar")
                    && !grammar.contains("sculpted collar")
                    && !grammar.contains("eri collar"),
                "{id} grammar still offers a neck-covering collar"
            );
            assert!(
                grammar.contains("neck"),
                "{id} grammar must state the uncovered-neck lock"
            );
            assert!(
                grammar.contains("Palette:"),
                "{id} grammar must state a common-sense palette family"
            );
        }
    }

    #[test]
    fn clothing_style_survives_on_the_outfit_module() {
        let mut identity = sanitize_upper_body_visual_identity(&complete_flat()).unwrap();
        assert_eq!(
            stamp_clothing_style(&mut identity, "sci-fi"),
            Some("sci-fi")
        );
        assert_eq!(identity["outfit"]["clothingStyle"], "sci-fi");
        assert_eq!(clothing_style_of(&identity), Some("sci-fi"));
        assert_eq!(
            clothing_style_of(&json!({
                "clothingStyle": "urban",
                "visualIdentity": identity
            })),
            Some("urban")
        );
    }

    #[test]
    fn character_module_stays_when_outfit_changes() {
        let first = sanitize_upper_body_visual_identity(&complete_flat()).unwrap();
        let character = character_module(&first).unwrap();
        let mut swapped = first.clone();
        swapped["outfit"]["outfitConstruction"] =
            json!("敞开领口内搭叠短风衣，胸前只有一条结构线，止于高腰");
        stamp_clothing_style(&mut swapped, "urban");
        assert_eq!(character_module(&swapped).unwrap(), character);
        assert_ne!(
            sanitize_upper_body_visual_identity(&swapped).unwrap()["outfit"]["outfitConstruction"],
            first["outfit"]["outfitConstruction"]
        );
        assert_eq!(clothing_style_of(&swapped), Some("urban"));
    }
}
