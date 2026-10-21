//! Report fact layer: every number and name a report shows comes from here,
//! computed from the platform data without a model call.
//!
//! [`PlatformFacts::facts`] are what the script may cite (`beat.fact` names a
//! fact id). [`PlatformFacts::card`] are the `card_visuals` keys the system
//! owns; [`card_visuals`] lays them over the model's tags, so a model never
//! writes a count the card shows.

mod catalog;
mod games;
mod social;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::services::smart_filter::{ContentAnalysis, SmartFilteredData};

#[cfg(test)]
pub(crate) use catalog::{anime_status_counts_five, github_contribution_level};
#[cfg(test)]
pub(crate) use games::normalize_steam_player_type;

/// One citable fact. `label` is English and only describes the fact to the
/// model; clients name a fact by its `id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Fact {
    pub id: String,
    pub label: String,
    pub value: Value,
    /// A cover, icon or avatar that pictures the fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct PlatformFacts {
    pub facts: Vec<Fact>,
    pub card: Map<String, Value>,
}

impl PlatformFacts {
    fn fact(&mut self, id: &str, label: &str, value: impl Into<Value>) {
        self.fact_with_media(id, label, value, None);
    }

    fn fact_with_media(
        &mut self,
        id: &str,
        label: &str,
        value: impl Into<Value>,
        media: Option<String>,
    ) {
        let value = value.into();
        let blank = value.is_null() || value.as_str().is_some_and(|s| s.trim().is_empty());
        if blank || self.facts.iter().any(|fact| fact.id == id) {
            return;
        }
        self.facts.push(Fact {
            id: id.to_string(),
            label: label.to_string(),
            value,
            media: media.filter(|url| !url.trim().is_empty()),
        });
    }

    fn card(&mut self, key: &str, value: impl Into<Value>) {
        self.card.insert(key.to_string(), value.into());
    }
}

/// The fact layer for one platform report.
pub(crate) async fn build(platform: &str, metadata: &SmartFilteredData) -> PlatformFacts {
    match platform {
        "steam" => games::steam(metadata).await,
        "xbox" => games::xbox(metadata),
        "psn" => games::psn(metadata),
        "github" => catalog::github(metadata).await,
        "netease" => catalog::netease(metadata).await,
        "bangumi" => catalog::bangumi(metadata).await,
        "mal" => catalog::mal(metadata).await,
        "bilibili" => social::bilibili(metadata).await,
        "youtube" => social::youtube(metadata),
        "x" => social::x(metadata),
        "discord" => social::discord(metadata),
        _ => PlatformFacts::default(),
    }
}

/// The stored `card_visuals`: the model's tags, clamped and backfilled, under
/// the system's card fields, with media URLs routed through the image proxy.
pub(crate) fn card_visuals(
    platform: &str,
    model_visuals: Value,
    facts: &PlatformFacts,
    metadata: &SmartFilteredData,
    locale: &str,
) -> Value {
    let mut obj = match model_visuals {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    normalize_model_visuals(platform, &mut obj, metadata, locale);
    for (key, value) in &facts.card {
        obj.insert(key.clone(), value.clone());
    }
    let mut visuals = Value::Object(obj);
    crate::api::profile::normalize_json_media_urls(&mut visuals);
    visuals
}

/// Fact media goes through the same proxy as card media.
pub(crate) fn proxied_facts(facts: &[Fact]) -> Vec<Fact> {
    facts
        .iter()
        .cloned()
        .map(|mut fact| {
            if let Some(url) = fact.media.as_mut() {
                *url = crate::api::profile::proxy_image_url(url);
            }
            fact
        })
        .collect()
}

/// Model-owned card tags: clamp list sizes the card lays out, map labels to
/// stable enums, and fill a tag the model left empty.
fn normalize_model_visuals(
    platform: &str,
    obj: &mut Map<String, Value>,
    metadata: &SmartFilteredData,
    locale: &str,
) {
    match (platform, &metadata.content_analysis) {
        ("steam", ContentAnalysis::Steam(_)) => games::normalize_steam_visuals(obj),
        ("netease", _) => truncate_array(obj, "mood_keywords", 6),
        ("x", ContentAnalysis::X(_)) => {
            if let Some(circles) = obj
                .get_mut("interest_circles")
                .and_then(|v| v.as_array_mut())
            {
                circles.truncate(4);
                for circle in circles.iter_mut() {
                    if let Some(accounts) =
                        circle.get_mut("accounts").and_then(|v| v.as_array_mut())
                    {
                        accounts.truncate(3);
                    }
                }
            }
            truncate_array(obj, "following_highlights", 5);
        }
        ("discord", ContentAnalysis::Discord(analysis)) => {
            truncate_array(obj, "community_tags", 3);
            crate::api::reports::extract::normalize_discord_guild_takes(
                obj,
                &analysis.guilds_preview,
                locale,
            );
        }
        ("xbox", ContentAnalysis::Xbox(analysis)) if is_blank(obj, "gamer_type") => {
            obj.insert(
                "gamer_type".to_string(),
                games::xbox_gamer_type_fallback(
                    locale,
                    analysis.completed_games,
                    analysis.average_completion,
                    analysis.gamerscore,
                )
                .into(),
            );
        }
        ("psn", ContentAnalysis::Psn(analysis)) if is_blank(obj, "hunter_type") => {
            obj.insert(
                "hunter_type".to_string(),
                games::psn_hunter_type_fallback(
                    locale,
                    analysis.platinum_count,
                    analysis.average_progress,
                )
                .into(),
            );
        }
        _ => {}
    }
}

fn truncate_array(obj: &mut Map<String, Value>, key: &str, len: usize) {
    if let Some(items) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
        items.truncate(len);
    }
}

fn is_blank(obj: &Map<String, Value>, key: &str) -> bool {
    obj.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .is_empty()
}

fn one_line(text: &str, max: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

/// Fact lines for the prompt: `id — label: value`.
pub(crate) fn prompt_lines(facts: &[Fact]) -> String {
    facts
        .iter()
        .map(|fact| {
            let value = match &fact.value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            // Titles come from third parties: one line each, never a heading.
            format!("{} — {}: {}", fact.id, fact.label, one_line(&value, 80))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
