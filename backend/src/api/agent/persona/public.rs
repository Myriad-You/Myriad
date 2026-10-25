//! What a visitor may read about her: the name and the persona as written.
//!
//! Only the persona document's descriptive fields leave here. The raw
//! personality prompt, the visual profile, her zone and the generation
//! records stay owner-only (`GET /api/agent/persona`).

use super::*;

const TEXT_LIMIT: usize = 2000;
const ITEM_LIMIT: usize = 120;
const LIST_LIMIT: usize = 12;

fn public_text(source: &Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| source.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .unwrap_or_default()
        .chars()
        .take(TEXT_LIMIT)
        .collect()
}

fn public_list(source: &Map<String, Value>, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| source.get(*key).and_then(Value::as_array))
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| item.chars().take(ITEM_LIMIT).collect())
                .take(LIST_LIMIT)
                .collect()
        })
        .unwrap_or_default()
}

/// The descriptive part of the persona document, or `None` when nothing in it
/// is worth showing. Older documents used `traits` and `voice`.
pub(crate) fn public_persona_profile(persona: Option<&Value>) -> Option<Value> {
    let source = persona?.as_object()?;
    let summary = public_text(source, &["summary"]);
    let temperament = public_list(source, &["temperament", "traits"]);
    let likes = public_list(source, &["likes"]);
    let drives = public_list(source, &["drives"]);
    let social_style = public_text(source, &["socialStyle"]);
    let speech_style = public_text(source, &["speechStyle", "voice"]);
    let empty = summary.is_empty()
        && temperament.is_empty()
        && likes.is_empty()
        && drives.is_empty()
        && social_style.is_empty()
        && speech_style.is_empty();
    (!empty).then(|| {
        json!({
            "summary": summary,
            "temperament": temperament,
            "likes": likes,
            "drives": drives,
            "socialStyle": social_style,
            "speechStyle": speech_style,
        })
    })
}

/// GET /api/agent/persona/public
///
/// Public. With the persona off there is only the product name and no profile.
pub async fn get_public_persona(
    State(db): State<DatabaseConnection>,
) -> Result<Json<Value>, HttpError> {
    let enabled = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved();
    if !enabled {
        return Ok(Json(json!({
            "name": merope::public_persona_name(false, None),
            "profile": Value::Null,
        })));
    }
    let persona = merope::get_persona(&db)
        .await
        .map_err(|error| persona_store_http("load public persona", error))?;
    Ok(Json(json!({
        "name": merope::public_persona_name(true, persona.as_ref().map(|row| row.name.as_str())),
        "profile": public_persona_profile(persona.as_ref().and_then(|row| row.persona_json.as_ref())),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_descriptive_fields_leave() {
        let profile = public_persona_profile(Some(&json!({
            "summary": "  quiet  ",
            "temperament": ["calm", " ", 3],
            "likes": ["rain"],
            "drives": [],
            "socialStyle": "listens first",
            "speechStyle": "short",
            "sourceTags": ["private"],
            "personaExtraRequirements": "private",
        })))
        .unwrap();
        assert_eq!(profile["summary"], "quiet");
        assert_eq!(profile["temperament"], json!(["calm"]));
        assert_eq!(profile["drives"], json!([]));
        assert!(profile.get("sourceTags").is_none());
        assert!(profile.get("personaExtraRequirements").is_none());
        assert_eq!(profile.as_object().unwrap().len(), 6);
    }

    #[test]
    fn older_keys_and_empty_documents() {
        let profile =
            public_persona_profile(Some(&json!({ "traits": ["shy"], "voice": "soft" }))).unwrap();
        assert_eq!(profile["temperament"], json!(["shy"]));
        assert_eq!(profile["speechStyle"], "soft");
        assert!(public_persona_profile(Some(&json!({ "summary": " " }))).is_none());
        assert!(public_persona_profile(Some(&json!("text"))).is_none());
        assert!(public_persona_profile(None).is_none());
    }

    #[test]
    fn long_values_are_capped() {
        let items: Vec<String> = (0..20).map(|i| format!("{i}{}", "x".repeat(200))).collect();
        let profile = public_persona_profile(Some(&json!({
            "summary": "y".repeat(5000),
            "likes": items,
        })))
        .unwrap();
        assert_eq!(
            profile["summary"].as_str().unwrap().chars().count(),
            TEXT_LIMIT
        );
        let likes = profile["likes"].as_array().unwrap();
        assert_eq!(likes.len(), LIST_LIMIT);
        assert_eq!(likes[0].as_str().unwrap().chars().count(), ITEM_LIMIT);
    }
}
