use std::collections::HashSet;

use serde_json::{json, Map, Value};

/// Saved outfits for one character. Face and hair stay on the character module.
pub const MAX_WARDROBE_ITEMS: usize = 8;
pub const MAX_WARDROBE_ID_CHARS: usize = 64;
pub const MAX_WARDROBE_NAME_CHARS: usize = 40;
/// Owner notes on visual profile / onboarding (`extraRequirements`,
/// `personaExtraRequirements`, and visual-design requirements).
pub const MAX_VISUAL_NOTES_CHARS: usize = 1_000;
/// Master-portrait outfit. Cannot be renamed or removed.
pub const DEFAULT_WARDROBE_ID: &str = "default";

/// Why a visual profile (or one of its nested values) was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualProfileReason {
    NotObject,
    NotArray,
    Empty,
    TooLong { max_chars: usize },
    ControlChar,
    UnknownStyle,
    DuplicateId,
    TooMany { max: usize },
    NeutralizedEmpty,
    Invalid,
}

impl VisualProfileReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotObject => "not_object",
            Self::NotArray => "not_array",
            Self::Empty => "empty",
            Self::TooLong { .. } => "too_long",
            Self::ControlChar => "control_char",
            Self::UnknownStyle => "unknown_style",
            Self::DuplicateId => "duplicate_id",
            Self::TooMany { .. } => "too_many",
            Self::NeutralizedEmpty => "neutralized_empty",
            Self::Invalid => "invalid",
        }
    }
}

/// Field path plus stable reason. Paths are relative to the value being sanitized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualProfileIssue {
    pub field: String,
    pub reason: VisualProfileReason,
}

impl VisualProfileIssue {
    pub fn new(field: impl Into<String>, reason: VisualProfileReason) -> Self {
        Self {
            field: field.into(),
            reason,
        }
    }

    pub fn prefixed(self, prefix: &str) -> Self {
        if prefix.is_empty() {
            return self;
        }
        if self.field.is_empty() {
            Self {
                field: prefix.to_string(),
                reason: self.reason,
            }
        } else {
            Self {
                field: format!("{prefix}.{}", self.field),
                reason: self.reason,
            }
        }
    }

    pub fn message(&self) -> String {
        let field = if self.field.is_empty() {
            "visualProfile"
        } else {
            self.field.as_str()
        };
        match self.reason {
            VisualProfileReason::NotObject => format!("{field} must be an object"),
            VisualProfileReason::NotArray => format!("{field} must be an array"),
            VisualProfileReason::Empty => format!("{field} is empty"),
            VisualProfileReason::TooLong { max_chars } => {
                format!("{field} exceeds {max_chars} characters")
            }
            VisualProfileReason::ControlChar => {
                format!("{field} contains a control character")
            }
            VisualProfileReason::UnknownStyle => {
                format!("{field} is not a known clothing style")
            }
            VisualProfileReason::DuplicateId => format!("{field} is duplicated"),
            VisualProfileReason::TooMany { max } => {
                format!("{field} has more than {max} items")
            }
            VisualProfileReason::NeutralizedEmpty => {
                format!("{field} was emptied by style-lock neutralization")
            }
            VisualProfileReason::Invalid => format!("{field} is invalid"),
        }
    }
}

fn join_field(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else if key.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

/// Required visual-identity fields for a close upper-body Merope portrait.
/// Lower-body garments, footwear, and articulated limb design intentionally do
/// not belong to this contract.
///
/// Field caps must stay in sync with frontend `UPPER_BODY_VISUAL_IDENTITY_LIMITS`.
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
    ("faceDesign", 500),
    ("eyeDesign", 500),
    ("hairShape", 500),
    ("hairLayerPlan", 700),
];

pub const OUTFIT_VISUAL_FIELDS: [(&str, usize); 7] = [
    ("upperBodySilhouette", 700),
    ("outfitConstruction", 1_200),
    ("sleeveArmDesign", 700),
    ("materialPlan", 1_200),
    ("heroAccessory", 500),
    ("paletteHint", 500),
    ("motif", 500),
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
    sanitize_upper_body_visual_identity_checked(value).ok()
}

pub fn sanitize_upper_body_visual_identity_checked(
    value: &Value,
) -> Result<Value, VisualProfileIssue> {
    match sanitize_modular_identity(value) {
        Ok(identity) => Ok(identity),
        Err(modular_err) => {
            if looks_modular(value) {
                Err(modular_err)
            } else {
                wrap_flat_identity(value)
            }
        }
    }
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

pub fn sanitize_outfit_module(value: &Value) -> Option<Value> {
    sanitize_outfit_module_checked(value).ok()
}

pub fn sanitize_outfit_module_checked(value: &Value) -> Result<Value, VisualProfileIssue> {
    sanitize_fields(value, &OUTFIT_VISUAL_FIELDS, "")
}

/// Owner-saved outfits. Each item is one portrait of the same character in
/// different clothes: replaceable `outfit` module, style, optional master
/// portrait, and optional compiled rig package.
pub fn sanitize_wardrobe(value: &Value) -> Option<Vec<Value>> {
    sanitize_wardrobe_checked(value).ok()
}

pub fn sanitize_wardrobe_checked(value: &Value) -> Result<Vec<Value>, VisualProfileIssue> {
    let items = value
        .as_array()
        .ok_or_else(|| VisualProfileIssue::new("", VisualProfileReason::NotArray))?;
    if items.len() > MAX_WARDROBE_ITEMS {
        return Err(VisualProfileIssue::new(
            "",
            VisualProfileReason::TooMany {
                max: MAX_WARDROBE_ITEMS,
            },
        ));
    }
    let mut out = Vec::with_capacity(items.len());
    let mut seen = HashSet::new();
    for (index, item) in items.iter().enumerate() {
        let prefix = index.to_string();
        if !item.is_object() {
            return Err(VisualProfileIssue::new(
                prefix,
                VisualProfileReason::NotObject,
            ));
        }
        let id = match item.get("id").and_then(Value::as_str).map(str::trim) {
            Some("") => {
                return Err(VisualProfileIssue::new(
                    join_field(&prefix, "id"),
                    VisualProfileReason::Empty,
                ));
            }
            Some(id) if id.chars().count() > MAX_WARDROBE_ID_CHARS => {
                return Err(VisualProfileIssue::new(
                    join_field(&prefix, "id"),
                    VisualProfileReason::TooLong {
                        max_chars: MAX_WARDROBE_ID_CHARS,
                    },
                ));
            }
            Some(id) if id.chars().any(char::is_control) => {
                return Err(VisualProfileIssue::new(
                    join_field(&prefix, "id"),
                    VisualProfileReason::ControlChar,
                ));
            }
            Some(id) if !seen.insert(id.to_string()) => {
                return Err(VisualProfileIssue::new(
                    join_field(&prefix, "id"),
                    VisualProfileReason::DuplicateId,
                ));
            }
            Some(id) => id.to_string(),
            None => {
                return Err(VisualProfileIssue::new(
                    join_field(&prefix, "id"),
                    if item.get("id").is_some() {
                        VisualProfileReason::Invalid
                    } else {
                        VisualProfileReason::Empty
                    },
                ));
            }
        };
        let style = match item.get("clothingStyle").and_then(Value::as_str) {
            Some(raw) => normalize_clothing_style(raw).ok_or_else(|| {
                VisualProfileIssue::new(
                    join_field(&prefix, "clothingStyle"),
                    VisualProfileReason::UnknownStyle,
                )
            })?,
            None => {
                return Err(VisualProfileIssue::new(
                    join_field(&prefix, "clothingStyle"),
                    if item.get("clothingStyle").is_some() {
                        VisualProfileReason::Invalid
                    } else {
                        VisualProfileReason::Empty
                    },
                ));
            }
        };
        let outfit = match item.get("outfit") {
            Some(outfit) => sanitize_fields(outfit, &OUTFIT_VISUAL_FIELDS, "")
                .map_err(|issue| issue.prefixed(&join_field(&prefix, "outfit")))?,
            None => {
                return Err(VisualProfileIssue::new(
                    join_field(&prefix, "outfit"),
                    VisualProfileReason::Empty,
                ));
            }
        };
        let mut saved = json!({
            "id": id,
            "clothingStyle": style,
            "outfit": outfit,
        });
        let obj = saved.as_object_mut().expect("wardrobe item is an object");
        if let Some(portrait) = item
            .get("portraitAssetId")
            .and_then(Value::as_str)
            .and_then(sanitize_wardrobe_portrait)
        {
            obj.insert("portraitAssetId".into(), json!(portrait));
        }
        if let Some(rig) = item
            .get("rigAssetId")
            .and_then(Value::as_str)
            .and_then(sanitize_wardrobe_hex_id)
        {
            obj.insert("rigAssetId".into(), json!(rig));
        }
        if let Some(fingerprint) = item
            .get("generationFingerprint")
            .and_then(Value::as_str)
            .and_then(sanitize_wardrobe_hex_id)
        {
            obj.insert("generationFingerprint".into(), json!(fingerprint));
        }
        if id != DEFAULT_WARDROBE_ID {
            if let Some(name) = item
                .get("name")
                .and_then(Value::as_str)
                .and_then(sanitize_wardrobe_name)
            {
                obj.insert("name".into(), json!(name));
            }
        }
        out.push(saved);
    }
    Ok(out)
}

fn sanitize_wardrobe_name(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    let trimmed: String = value.chars().take(MAX_WARDROBE_NAME_CHARS).collect();
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_default_wardrobe_item(item: &Value) -> bool {
    item.get("id").and_then(Value::as_str) == Some(DEFAULT_WARDROBE_ID)
}

fn default_wardrobe_item(style: &str, outfit: &Value) -> Value {
    json!({
        "id": DEFAULT_WARDROBE_ID,
        "clothingStyle": style,
        "outfit": outfit,
    })
}

/// Keep the master-portrait outfit as the default set.
///
/// Empty wardrobe + visual identity seeds it. A wardrobe that already had
/// `default` cannot drop it. A legacy wardrobe without that id promotes the
/// first stored set.
pub fn ensure_default_wardrobe(profile: &mut Value, previous: Option<&Map<String, Value>>) {
    let Some(root) = profile.as_object() else {
        return;
    };
    let identity = match root.get("visualIdentity") {
        Some(value) if !value.is_null() => value,
        _ => return,
    };
    if !root.contains_key("wardrobe") {
        return;
    }
    let outfit = identity.get("outfit").cloned();
    let style = root
        .get("clothingStyle")
        .and_then(Value::as_str)
        .and_then(normalize_clothing_style)
        .or_else(|| clothing_style_of(identity))
        .map(str::to_string);
    let previous_default = previous
        .and_then(|value| value.get("wardrobe"))
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find(|item| is_default_wardrobe_item(item)))
        .cloned()
        .map(|mut item| {
            if let Some(obj) = item.as_object_mut() {
                obj.remove("name");
            }
            item
        });

    let Some(root) = profile.as_object_mut() else {
        return;
    };
    let mut promoted_from = None;
    let mut seeded = false;
    {
        let Some(items) = root.get_mut("wardrobe").and_then(Value::as_array_mut) else {
            return;
        };
        for item in items.iter_mut() {
            if is_default_wardrobe_item(item) {
                if let Some(obj) = item.as_object_mut() {
                    obj.remove("name");
                }
            }
        }
        if items.iter().any(is_default_wardrobe_item) {
            return;
        }
        if let Some(default_item) = previous_default {
            items.insert(0, default_item);
            while items.len() > MAX_WARDROBE_ITEMS {
                if let Some(index) = (1..items.len())
                    .rev()
                    .find(|&index| !is_default_wardrobe_item(&items[index]))
                {
                    items.remove(index);
                } else {
                    break;
                }
            }
            return;
        }
        if items.is_empty() {
            let (Some(style), Some(outfit)) = (style.as_deref(), outfit.as_ref()) else {
                return;
            };
            items.push(default_wardrobe_item(style, outfit));
            seeded = true;
        } else if previous.is_some() {
            promoted_from = items[0]
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(obj) = items[0].as_object_mut() {
                obj.insert("id".into(), json!(DEFAULT_WARDROBE_ID));
                obj.remove("name");
            }
        }
    }
    if seeded {
        root.insert("activeOutfitId".into(), json!(DEFAULT_WARDROBE_ID));
        return;
    }
    if let Some(old_id) = promoted_from {
        if root.get("activeOutfitId").and_then(Value::as_str) == Some(old_id.as_str()) {
            root.insert("activeOutfitId".into(), json!(DEFAULT_WARDROBE_ID));
        }
    }
}

fn sanitize_wardrobe_hex_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.len() != 64 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(value.to_ascii_lowercase())
}

fn wardrobe_item_id(item: &Value) -> Option<&str> {
    item.get("id").and_then(Value::as_str)
}

fn wardrobe_portrait(item: &Value) -> Option<&str> {
    item.get("portraitAssetId").and_then(Value::as_str)
}

/// Drop a stored rig when that outfit's portrait is no longer the one it was
/// compiled from. Other outfits keep their packages.
pub fn reconcile_wardrobe_rigs(profile: &mut Value, previous: Option<&Map<String, Value>>) {
    let Some(previous_items) = previous
        .and_then(|value| value.get("wardrobe"))
        .and_then(Value::as_array)
    else {
        return;
    };
    let Some(items) = profile
        .as_object_mut()
        .and_then(|root| root.get_mut("wardrobe"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for item in items {
        let Some(id) = wardrobe_item_id(item).map(str::to_string) else {
            continue;
        };
        let previous_item = previous_items
            .iter()
            .find(|candidate| wardrobe_item_id(candidate) == Some(id.as_str()));
        let previous_portrait = previous_item.and_then(wardrobe_portrait);
        if wardrobe_portrait(item) == previous_portrait {
            continue;
        }
        if let Some(obj) = item.as_object_mut() {
            obj.remove("rigAssetId");
            obj.remove("generationFingerprint");
        }
    }
}

/// Bind a compiled package to the outfit currently being worn.
pub fn bind_active_outfit_rig(profile: &mut Value, rig_asset_id: &str) -> bool {
    let Some(rig) = sanitize_wardrobe_hex_id(rig_asset_id) else {
        return false;
    };
    let Some(root) = profile.as_object_mut() else {
        return false;
    };
    let active = root
        .get("activeOutfitId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(items) = root.get_mut("wardrobe").and_then(Value::as_array_mut) else {
        return false;
    };
    let target = active.or_else(|| {
        (items.len() == 1)
            .then(|| wardrobe_item_id(&items[0]).map(str::to_string))
            .flatten()
    });
    let Some(target) = target else {
        return false;
    };
    for item in items {
        if wardrobe_item_id(item) != Some(target.as_str()) {
            continue;
        }
        if let Some(obj) = item.as_object_mut() {
            obj.insert("rigAssetId".into(), json!(rig));
            return true;
        }
    }
    false
}

/// Forget the worn outfit's rig after its portrait changes. Other sets stay.
pub fn detach_active_outfit_rig(profile: &mut Value) {
    let Some(root) = profile.as_object_mut() else {
        return;
    };
    let Some(active) = root
        .get("activeOutfitId")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    let Some(items) = root.get_mut("wardrobe").and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        if wardrobe_item_id(item) != Some(active.as_str()) {
            continue;
        }
        if let Some(obj) = item.as_object_mut() {
            obj.remove("rigAssetId");
            obj.remove("generationFingerprint");
        }
        return;
    }
}

/// Live pointer for the worn outfit. Missing means play the portrait only.
pub fn active_outfit_rig_asset_id(profile: Option<&Value>) -> Option<String> {
    let profile = profile?;
    let active = profile.get("activeOutfitId").and_then(Value::as_str)?;
    profile
        .get("wardrobe")
        .and_then(Value::as_array)?
        .iter()
        .find(|item| wardrobe_item_id(item) == Some(active))
        .and_then(|item| item.get("rigAssetId").and_then(Value::as_str))
        .and_then(sanitize_wardrobe_hex_id)
}

/// Provenance of the worn outfit's portrait, used to match that outfit's rig.
pub fn active_outfit_generation_fingerprint(profile: Option<&Value>) -> Option<String> {
    let profile = profile?;
    let active = profile.get("activeOutfitId").and_then(Value::as_str)?;
    profile
        .get("wardrobe")
        .and_then(Value::as_array)?
        .iter()
        .find(|item| wardrobe_item_id(item) == Some(active))
        .and_then(|item| item.get("generationFingerprint").and_then(Value::as_str))
        .and_then(sanitize_wardrobe_hex_id)
}

fn sanitize_wardrobe_portrait(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > 512
        || value.contains(':')
        || value.contains("..")
        || value.starts_with("//")
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    let bare_id = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if value.starts_with('/') || bare_id {
        Some(value.to_string())
    } else {
        None
    }
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

fn looks_modular(value: &Value) -> bool {
    let root = value.get("visualIdentity").unwrap_or(value);
    root.get("character").is_some() || root.get("outfit").is_some()
}

fn sanitize_modular_identity(value: &Value) -> Result<Value, VisualProfileIssue> {
    let root = value.get("visualIdentity").unwrap_or(value);
    let character = match root.get("character") {
        Some(character) => sanitize_fields(character, &CHARACTER_VISUAL_FIELDS, "character")?,
        None => {
            return Err(VisualProfileIssue::new(
                "character",
                VisualProfileReason::Empty,
            ));
        }
    };
    let mut outfit = match root.get("outfit") {
        Some(outfit) => sanitize_fields(outfit, &OUTFIT_VISUAL_FIELDS, "outfit")?,
        None => {
            return Err(VisualProfileIssue::new(
                "outfit",
                VisualProfileReason::Empty,
            ));
        }
    };
    if let Some(style) = root
        .get("outfit")
        .and_then(|outfit| outfit.get("clothingStyle"))
        .and_then(Value::as_str)
        .and_then(normalize_clothing_style)
    {
        outfit
            .as_object_mut()
            .ok_or_else(|| VisualProfileIssue::new("outfit", VisualProfileReason::NotObject))?
            .insert("clothingStyle".into(), json!(style));
    }
    Ok(json!({
        "character": character,
        "outfit": outfit,
    }))
}

fn wrap_flat_identity(value: &Value) -> Result<Value, VisualProfileIssue> {
    let source = value.get("visualIdentity").unwrap_or(value);
    let character = sanitize_fields(source, &CHARACTER_VISUAL_FIELDS, "")?;
    let mut outfit = sanitize_fields(source, &OUTFIT_VISUAL_FIELDS, "")?;
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
            .as_object_mut()
            .ok_or_else(|| VisualProfileIssue::new("outfit", VisualProfileReason::NotObject))?
            .insert("clothingStyle".into(), json!(style));
    }
    Ok(json!({
        "character": character,
        "outfit": outfit,
    }))
}

fn sanitize_fields(
    source: &Value,
    fields: &[(&str, usize)],
    prefix: &str,
) -> Result<Value, VisualProfileIssue> {
    let source = source.as_object().ok_or_else(|| {
        VisualProfileIssue::new(prefix.to_string(), VisualProfileReason::NotObject)
    })?;
    let mut sanitized = Map::new();
    for (key, max_chars) in fields {
        let field = join_field(prefix, key);
        match source.get(*key) {
            None => {
                return Err(VisualProfileIssue::new(field, VisualProfileReason::Empty));
            }
            Some(Value::String(raw)) => {
                let raw = raw.trim();
                if raw.is_empty() {
                    return Err(VisualProfileIssue::new(field, VisualProfileReason::Empty));
                }
                if raw.chars().count() > *max_chars {
                    return Err(VisualProfileIssue::new(
                        field,
                        VisualProfileReason::TooLong {
                            max_chars: *max_chars,
                        },
                    ));
                }
                if raw.chars().any(char::is_control) {
                    return Err(VisualProfileIssue::new(
                        field,
                        VisualProfileReason::ControlChar,
                    ));
                }
                sanitized.insert((*key).to_string(), Value::String(raw.to_string()));
            }
            Some(_) => {
                return Err(VisualProfileIssue::new(field, VisualProfileReason::Invalid));
            }
        }
    }
    Ok(Value::Object(sanitized))
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
        let missing_err = sanitize_upper_body_visual_identity_checked(&missing).unwrap_err();
        assert_eq!(missing_err.field, "eyeDesign");
        assert_eq!(missing_err.reason, VisualProfileReason::Empty);
        let mut control = complete_flat();
        control["visualIdentity"]["motif"] = json!("星轨\u{0000}");
        assert!(!upper_body_visual_identity_is_complete(&control));
        let control_err = sanitize_upper_body_visual_identity_checked(&control).unwrap_err();
        assert_eq!(control_err.field, "motif");
        assert_eq!(control_err.reason, VisualProfileReason::ControlChar);
    }

    #[test]
    fn visual_identity_accepts_the_frontend_field_caps() {
        let mut identity = complete_flat();
        identity["visualIdentity"]["faceDesign"] = json!("甲".repeat(500));
        assert!(sanitize_upper_body_visual_identity(&identity).is_some());
        identity["visualIdentity"]["faceDesign"] = json!("甲".repeat(501));
        let err = sanitize_upper_body_visual_identity_checked(&identity).unwrap_err();
        assert_eq!(err.field, "faceDesign");
        assert_eq!(err.reason, VisualProfileReason::TooLong { max_chars: 500 });
    }

    #[test]
    fn modular_identity_names_the_missing_character_field() {
        let identity = complete_flat()["visualIdentity"].clone();
        let mut character = Map::new();
        character.insert("faceDesign".into(), identity["faceDesign"].clone());
        let err = sanitize_upper_body_visual_identity_checked(&json!({
            "character": character,
            "outfit": {
                "upperBodySilhouette": identity["upperBodySilhouette"],
                "outfitConstruction": identity["outfitConstruction"],
                "sleeveArmDesign": identity["sleeveArmDesign"],
                "materialPlan": identity["materialPlan"],
                "heroAccessory": identity["heroAccessory"],
                "paletteHint": identity["paletteHint"],
                "motif": identity["motif"]
            }
        }))
        .unwrap_err();
        assert_eq!(err.field, "character.eyeDesign");
        assert_eq!(err.reason, VisualProfileReason::Empty);
        assert_eq!(err.message(), "character.eyeDesign is empty");
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

    #[test]
    fn wardrobe_keeps_outfit_modules_and_rejects_bad_ids() {
        let outfit = json!({
            "upperBodySilhouette": complete_flat()["visualIdentity"]["upperBodySilhouette"],
            "outfitConstruction": complete_flat()["visualIdentity"]["outfitConstruction"],
            "sleeveArmDesign": complete_flat()["visualIdentity"]["sleeveArmDesign"],
            "materialPlan": complete_flat()["visualIdentity"]["materialPlan"],
            "heroAccessory": complete_flat()["visualIdentity"]["heroAccessory"],
            "paletteHint": complete_flat()["visualIdentity"]["paletteHint"],
            "motif": complete_flat()["visualIdentity"]["motif"]
        });
        let wardrobe = json!([
            { "id": "w-a", "clothingStyle": "urban", "outfit": outfit },
            { "id": "w-b", "clothingStyle": "idol", "outfit": outfit }
        ]);
        let with_portrait = json!([
            {
                "id": "w-a",
                "clothingStyle": "urban",
                "outfit": outfit,
                "portraitAssetId": "/uploads/urban.png",
                "ignored": true
            },
            {
                "id": "w-b",
                "clothingStyle": "idol",
                "outfit": outfit,
                "portraitAssetId": "https://cdn.example.com/x.png"
            }
        ]);
        let sanitized = sanitize_wardrobe(&wardrobe).unwrap();
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized[0]["clothingStyle"], "urban");
        assert!(sanitize_outfit_module(&outfit).is_some());
        let portraits = sanitize_wardrobe(&with_portrait).unwrap();
        assert_eq!(portraits[0]["portraitAssetId"], "/uploads/urban.png");
        assert!(portraits[1].get("portraitAssetId").is_none());
        let named = json!([{
            "id": "w-a",
            "clothingStyle": "urban",
            "outfit": outfit,
            "name": "  冬日大衣  "
        }]);
        assert_eq!(sanitize_wardrobe(&named).unwrap()[0]["name"], "冬日大衣");

        let duplicate = json!([
            { "id": "w-a", "clothingStyle": "urban", "outfit": outfit },
            { "id": "w-a", "clothingStyle": "idol", "outfit": outfit }
        ]);
        assert!(sanitize_wardrobe(&duplicate).is_none());
        let duplicate_err = sanitize_wardrobe_checked(&duplicate).unwrap_err();
        assert_eq!(duplicate_err.field, "1.id");
        assert_eq!(duplicate_err.reason, VisualProfileReason::DuplicateId);
        assert!(sanitize_wardrobe(&json!([])).unwrap().is_empty());

        let default_named = json!([{
            "id": DEFAULT_WARDROBE_ID,
            "clothingStyle": "urban",
            "outfit": outfit,
            "name": "想改的名字"
        }]);
        let default_saved = sanitize_wardrobe(&default_named).unwrap();
        assert_eq!(default_saved[0]["id"], DEFAULT_WARDROBE_ID);
        assert!(default_saved[0].get("name").is_none());

        let rig = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let with_rig = json!([{
            "id": "w-a",
            "clothingStyle": "urban",
            "outfit": outfit,
            "portraitAssetId": "/uploads/urban.png",
            "rigAssetId": rig
        }]);
        assert_eq!(sanitize_wardrobe(&with_rig).unwrap()[0]["rigAssetId"], rig);
        let bad_rig = json!([{
            "id": "w-a",
            "clothingStyle": "urban",
            "outfit": outfit,
            "rigAssetId": "not-a-package"
        }]);
        assert!(sanitize_wardrobe(&bad_rig).unwrap()[0]
            .get("rigAssetId")
            .is_none());
    }

    #[test]
    fn wardrobe_keeps_each_outfit_rig_and_drops_it_when_that_portrait_changes() {
        let identity = sanitize_upper_body_visual_identity(&complete_flat()).unwrap();
        let rig_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let rig_b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let mut profile = json!({
            "clothingStyle": "urban",
            "visualIdentity": identity,
            "activeOutfitId": "w-b",
            "wardrobe": [
                {
                    "id": DEFAULT_WARDROBE_ID,
                    "clothingStyle": "urban",
                    "outfit": identity["outfit"],
                    "portraitAssetId": "/uploads/a.png",
                    "rigAssetId": rig_a
                },
                {
                    "id": "w-b",
                    "clothingStyle": "idol",
                    "outfit": identity["outfit"],
                    "portraitAssetId": "/uploads/b.png",
                    "rigAssetId": rig_b,
                    "generationFingerprint": rig_b
                }
            ]
        });
        assert_eq!(
            active_outfit_rig_asset_id(Some(&profile)).as_deref(),
            Some(rig_b)
        );
        assert_eq!(
            active_outfit_generation_fingerprint(Some(&profile)).as_deref(),
            Some(rig_b)
        );
        let previous = profile.as_object().cloned().unwrap();
        profile["wardrobe"][1]["portraitAssetId"] = json!("/uploads/b-next.png");
        reconcile_wardrobe_rigs(&mut profile, Some(&previous));
        assert_eq!(profile["wardrobe"][0]["rigAssetId"], rig_a);
        assert!(profile["wardrobe"][1].get("rigAssetId").is_none());
        assert!(profile["wardrobe"][1]
            .get("generationFingerprint")
            .is_none());
        detach_active_outfit_rig(&mut profile);
        assert_eq!(profile["wardrobe"][0]["rigAssetId"], rig_a);
        bind_active_outfit_rig(&mut profile, rig_b);
        assert_eq!(profile["wardrobe"][1]["rigAssetId"], rig_b);
        assert_eq!(
            active_outfit_rig_asset_id(Some(&profile)).as_deref(),
            Some(rig_b)
        );
    }

    #[test]
    fn default_wardrobe_is_seeded_and_restored() {
        let identity = sanitize_upper_body_visual_identity(&complete_flat()).unwrap();
        let mut empty = json!({
            "clothingStyle": "urban",
            "visualIdentity": identity,
            "wardrobe": []
        });
        ensure_default_wardrobe(&mut empty, None);
        assert_eq!(empty["wardrobe"][0]["id"], DEFAULT_WARDROBE_ID);
        assert_eq!(empty["activeOutfitId"], DEFAULT_WARDROBE_ID);
        assert_eq!(empty["wardrobe"][0]["outfit"], identity["outfit"]);

        let previous = empty.as_object().cloned().unwrap();
        let extra_outfit = identity["outfit"].clone();
        let mut omitted = json!({
            "clothingStyle": "idol",
            "visualIdentity": identity,
            "wardrobe": [{
                "id": "w-new",
                "clothingStyle": "idol",
                "outfit": extra_outfit
            }],
            "activeOutfitId": "w-new"
        });
        ensure_default_wardrobe(&mut omitted, Some(&previous));
        assert_eq!(omitted["wardrobe"][0]["id"], DEFAULT_WARDROBE_ID);
        assert_eq!(omitted["wardrobe"][1]["id"], "w-new");
        assert_eq!(omitted["activeOutfitId"], "w-new");

        let mut later = identity.clone();
        later["outfit"]["outfitConstruction"] =
            json!("敞开领口内搭叠短风衣，胸前只有一条结构线，止于高腰");
        let mut emptied = json!({
            "clothingStyle": "idol",
            "visualIdentity": later,
            "wardrobe": []
        });
        ensure_default_wardrobe(&mut emptied, Some(&previous));
        assert_eq!(emptied["wardrobe"][0]["id"], DEFAULT_WARDROBE_ID);
        assert_eq!(emptied["wardrobe"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            emptied["wardrobe"][0]["outfit"],
            previous["wardrobe"][0]["outfit"]
        );

        let mut legacy = json!({
            "clothingStyle": "urban",
            "visualIdentity": identity,
            "wardrobe": [{
                "id": "w-old",
                "clothingStyle": "urban",
                "outfit": identity["outfit"],
                "name": "旧名字"
            }],
            "activeOutfitId": "w-old"
        });
        ensure_default_wardrobe(&mut legacy, Some(&Map::new()));
        assert_eq!(legacy["wardrobe"][0]["id"], DEFAULT_WARDROBE_ID);
        assert!(legacy["wardrobe"][0].get("name").is_none());
        assert_eq!(legacy["activeOutfitId"], DEFAULT_WARDROBE_ID);
    }
}
