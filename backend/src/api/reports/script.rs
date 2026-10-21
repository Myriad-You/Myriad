//! The report script: a headline and ordered chapters of one-line beats. A
//! beat may cite one fact by id; it never carries a number of its own.
//!
//! The old `summary` / `insights` fields are derived from the script so every
//! reader of `platform_reports.report` keeps working unchanged.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::facts::Fact;
use crate::services::report_styles::{LengthBudget, ReportStyle, chapter_meaning};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Beat {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Chapter {
    pub key: String,
    pub title: String,
    pub beats: Vec<Beat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportScript {
    pub headline: String,
    pub chapters: Vec<Chapter>,
}

/// What the report model returns: the script plus the card tags it owns.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelReport {
    pub script: ReportScript,
    pub card_visuals: Value,
}

/// The JSON schema for one model call. `only` narrows it to a single chapter
/// (the style preview).
pub(crate) fn schema(style: &ReportStyle, only: Option<&str>) -> Value {
    let keys: Vec<&str> = match only {
        Some(key) => vec![key],
        None => style.chapters.iter().map(String::as_str).collect(),
    };
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["headline", "chapters", "card_visuals"],
        "properties": {
            "headline": { "type": "string" },
            "chapters": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["key", "title", "beats"],
                    "properties": {
                        "key": { "type": "string", "enum": keys },
                        "title": { "type": "string" },
                        "beats": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["text", "fact"],
                                "properties": {
                                    "text": { "type": "string" },
                                    "fact": { "type": ["string", "null"] }
                                }
                            }
                        }
                    }
                }
            },
            "card_visuals": { "type": "object" }
        }
    })
}

#[derive(Deserialize)]
struct RawReport {
    #[serde(default)]
    headline: String,
    #[serde(default)]
    chapters: Vec<RawChapter>,
    #[serde(default)]
    card_visuals: Value,
}

#[derive(Deserialize)]
struct RawChapter {
    #[serde(default)]
    key: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    beats: Vec<RawBeat>,
}

#[derive(Deserialize)]
struct RawBeat {
    #[serde(default)]
    text: String,
    #[serde(default)]
    fact: Option<String>,
}

/// Parse the model's text into a script. Shape only; [`validate`] judges it.
pub(crate) fn parse(raw: &str) -> Result<ModelReport, String> {
    let candidate = super::prompt_data::extract_json_object(raw)
        .ok_or_else(|| "no JSON object in response".to_string())?;
    let parsed: RawReport = serde_json::from_str(candidate).map_err(|e| e.to_string())?;
    let card_visuals = if parsed.card_visuals.is_object() {
        parsed.card_visuals
    } else {
        json!({})
    };
    let chapters = parsed
        .chapters
        .into_iter()
        .map(|chapter| Chapter {
            key: chapter.key.trim().to_string(),
            title: chapter.title.trim().to_string(),
            beats: chapter
                .beats
                .into_iter()
                .map(|beat| Beat {
                    text: beat.text.trim().to_string(),
                    fact: beat
                        .fact
                        .map(|id| id.trim().to_string())
                        .filter(|id| !id.is_empty()),
                })
                .collect(),
        })
        .collect();
    Ok(ModelReport {
        script: ReportScript {
            headline: parsed.headline.trim().to_string(),
            chapters,
        },
        card_visuals,
    })
}

/// Latin-script languages run about twice as many characters per idea as CJK.
pub(crate) fn char_slack(locale: &str) -> usize {
    match locale.split('-').next().unwrap_or("") {
        "zh" | "ja" | "ko" => 1,
        _ => 2,
    }
}

/// The hard cap a line may reach. The prompt asks for `limit`; a line a
/// little over it is kept, one far over it is a miss.
fn hard_cap(limit: usize, locale: &str) -> usize {
    limit * char_slack(locale) * 3 / 2
}

fn expected_keys<'a>(style: &'a ReportStyle, only: Option<&'a str>) -> Vec<&'a str> {
    match only {
        Some(key) => vec![key],
        None => style.chapters.iter().map(String::as_str).collect(),
    }
}

/// A chapter's display title in `locale`, else its key.
pub(crate) fn chapter_title(key: &str, locale: &str) -> String {
    let title = crate::i18n::reports(locale, &format!("chapter.{key}"));
    if title.starts_with("chapter.") || title.is_empty() {
        key.to_string()
    } else {
        title
    }
}

/// Fix what needs no model: an unknown fact id is cleared, empty beats and
/// over-long beats are dropped while their chapter keeps another, beats past
/// the cap are cut, unknown or repeated chapters dropped, chapters put in
/// style order, an empty title takes the default one. Returns what changed,
/// for the log. A missing chapter, a bad headline, or a chapter with no usable
/// beat is left for [`validate`] to name, and only those cost a retry.
pub(crate) fn repair(
    script: &mut ReportScript,
    style: &ReportStyle,
    facts: &[Fact],
    locale: &str,
    only: Option<&str>,
) -> Vec<String> {
    let budget = style.length.budget();
    let cap = hard_cap(budget.beat_chars, locale);
    let expected = expected_keys(style, only);
    let mut changes = Vec::new();

    let mut seen = std::collections::HashSet::new();
    let before = script.chapters.len();
    script.chapters.retain(|chapter| {
        expected.contains(&chapter.key.as_str()) && seen.insert(chapter.key.clone())
    });
    if script.chapters.len() != before {
        changes.push(format!(
            "dropped {} unknown or repeated chapters",
            before - script.chapters.len()
        ));
    }
    let order = |key: &str| expected.iter().position(|known| *known == key);
    if !script
        .chapters
        .is_sorted_by_key(|chapter| order(&chapter.key))
    {
        script.chapters.sort_by_key(|chapter| order(&chapter.key));
        changes.push("put chapters in style order".to_string());
    }

    for chapter in &mut script.chapters {
        if chapter.title.is_empty() {
            chapter.title = chapter_title(&chapter.key, locale);
            changes.push(format!("{}: default title", chapter.key));
        }
        for beat in &mut chapter.beats {
            if let Some(id) = beat.fact.take() {
                if facts.iter().any(|fact| fact.id == id) {
                    beat.fact = Some(id);
                } else {
                    changes.push(format!("{}: cleared unknown fact {id}", chapter.key));
                }
            }
        }
        let usable = |beat: &Beat| {
            let len = beat.text.chars().count();
            len > 0 && len <= cap
        };
        if chapter.beats.iter().any(usable) && !chapter.beats.iter().all(usable) {
            let before = chapter.beats.len();
            chapter.beats.retain(usable);
            changes.push(format!(
                "{}: dropped {} empty or over-long beats",
                chapter.key,
                before - chapter.beats.len()
            ));
        }
        if chapter.beats.len() > budget.max_beats {
            chapter.beats.truncate(budget.max_beats);
            changes.push(format!(
                "{}: cut to {} beats",
                chapter.key, budget.max_beats
            ));
        }
    }
    changes
}

/// Check a script against its style and its facts. `only` checks a single
/// chapter (the preview). Returns every problem, so a retry can name them.
pub(crate) fn validate(
    script: &ReportScript,
    style: &ReportStyle,
    facts: &[Fact],
    locale: &str,
    only: Option<&str>,
) -> Result<(), Vec<String>> {
    let budget: LengthBudget = style.length.budget();
    let mut problems = Vec::new();
    let headline = script.headline.chars().count();
    if headline == 0 {
        problems.push("headline is empty".to_string());
    } else if headline > hard_cap(budget.headline_chars, locale) {
        problems.push(format!(
            "headline is {headline} characters; keep it within {}",
            budget.headline_chars * char_slack(locale)
        ));
    }
    let expected = expected_keys(style, only);
    let got: Vec<&str> = script.chapters.iter().map(|c| c.key.as_str()).collect();
    if got != expected {
        problems.push(format!(
            "chapters must be exactly [{}] in that order; got [{}]",
            expected.join(", "),
            got.join(", ")
        ));
    }
    for chapter in &script.chapters {
        if chapter.title.is_empty() {
            problems.push(format!("chapter {} has no title", chapter.key));
        }
        let beats = chapter.beats.len();
        if beats < budget.min_beats || beats > budget.max_beats {
            problems.push(format!(
                "chapter {} has {beats} beats; write {}-{}",
                chapter.key, budget.min_beats, budget.max_beats
            ));
        }
        for beat in &chapter.beats {
            let len = beat.text.chars().count();
            if len == 0 {
                problems.push(format!("chapter {} has an empty beat", chapter.key));
            } else if len > hard_cap(budget.beat_chars, locale) {
                problems.push(format!(
                    "a beat in {} is {len} characters; keep beats within {}",
                    chapter.key,
                    budget.beat_chars * char_slack(locale)
                ));
            }
            if let Some(id) = &beat.fact {
                if !facts.iter().any(|fact| &fact.id == id) {
                    problems.push(format!(
                        "beat cites unknown fact {id}; use a listed id or null"
                    ));
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// `summary`: the headline — one short line, as the old portrait was.
pub(crate) fn summary_of(script: &ReportScript) -> String {
    let headline = script.headline.trim();
    if !headline.is_empty() {
        return headline.to_string();
    }
    script
        .chapters
        .iter()
        .flat_map(|chapter| chapter.beats.iter())
        .map(|beat| beat.text.trim().to_string())
        .find(|text| !text.is_empty())
        .unwrap_or_default()
}

/// `insights`: every beat, chapter by chapter, in order.
pub(crate) fn insights_of(script: &ReportScript) -> Vec<String> {
    script
        .chapters
        .iter()
        .flat_map(|chapter| chapter.beats.iter())
        .map(|beat| beat.text.clone())
        .filter(|text| !text.trim().is_empty())
        .collect()
}

/// The local fallback, laid out in the style's chapters: its summary is the
/// headline, its insights fill the chapters in order, so the derived
/// `summary` / `insights` are exactly the fallback's own.
pub(crate) fn from_mock(
    summary: &str,
    insights: &[String],
    style: &ReportStyle,
    locale: &str,
) -> ReportScript {
    let title = |key: &str| chapter_title(key, locale);
    let beat = |text: &str| Beat {
        text: text.to_string(),
        fact: None,
    };
    let keys = &style.chapters;
    let mut chapters: Vec<Chapter> = Vec::new();
    if insights.is_empty() {
        if let Some(first) = keys.first().filter(|_| !summary.trim().is_empty()) {
            chapters.push(Chapter {
                key: first.clone(),
                title: title(first),
                beats: vec![beat(summary)],
            });
        }
    } else if !keys.is_empty() {
        let per = insights.len().div_ceil(keys.len()).max(1);
        for (index, key) in keys.iter().enumerate() {
            let beats: Vec<Beat> = insights
                .iter()
                .skip(index * per)
                .take(per)
                .map(|text| beat(text))
                .collect();
            if !beats.is_empty() {
                chapters.push(Chapter {
                    key: key.clone(),
                    title: title(key),
                    beats,
                });
            }
        }
    }
    ReportScript {
        headline: summary.to_string(),
        chapters,
    }
}

/// Chapter lines for the prompt: `key — meaning`.
pub(crate) fn chapter_lines(keys: &[String]) -> String {
    keys.iter()
        .map(|key| format!("{key} — {}", chapter_meaning(key).unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::report_styles::builtin_styles;

    fn facts() -> Vec<Fact> {
        vec![Fact {
            id: "games_count".into(),
            label: "Games owned".into(),
            value: json!(42),
            media: None,
        }]
    }

    fn good() -> String {
        json!({
            "headline": "只玩少数几款",
            "chapters": [
                { "key": "portrait", "title": "画像", "beats": [{ "text": "时间都压在一款上。", "fact": null }] },
                { "key": "taste", "title": "口味", "beats": [{ "text": "库很大，玩得很少。", "fact": "games_count" }, { "text": "偏爱动作。", "fact": null }] },
                { "key": "highlight", "title": "高光", "beats": [{ "text": "法环一百多小时。", "fact": null }] }
            ],
            "card_visuals": { "player_type": "hardcore" }
        })
        .to_string()
    }

    #[test]
    fn a_good_script_parses_validates_and_derives_old_fields() {
        let style = &builtin_styles()[0];
        let report = parse(&format!("```json\n{}\n```", good())).unwrap();
        validate(&report.script, style, &facts(), "zh-CN", None).unwrap();
        assert_eq!(report.card_visuals["player_type"], "hardcore");
        assert_eq!(summary_of(&report.script), "只玩少数几款");
        assert_eq!(
            insights_of(&report.script),
            vec![
                "时间都压在一款上。",
                "库很大，玩得很少。",
                "偏爱动作。",
                "法环一百多小时。"
            ]
        );
        // One chapter still yields insights, from that chapter alone.
        let mut single = report.script.clone();
        single.chapters.truncate(1);
        assert_eq!(insights_of(&single), vec!["时间都压在一款上。"]);
        let stored = serde_json::to_value(&report.script).unwrap();
        assert!(stored["chapters"][0]["beats"][0].get("fact").is_none());
        assert_eq!(stored["chapters"][1]["beats"][0]["fact"], "games_count");
    }

    #[test]
    fn validation_names_every_miss() {
        let style = &builtin_styles()[0];
        let mut report = parse(&good()).unwrap();
        report.script.chapters[1].beats[0].fact = Some("invented".into());
        report.script.chapters.swap(0, 2);
        report.script.chapters[0].beats = (0..5)
            .map(|_| Beat {
                text: "x".repeat(200),
                fact: None,
            })
            .collect();
        let problems = validate(&report.script, style, &facts(), "zh-CN", None).unwrap_err();
        let all = problems.join("\n");
        assert!(all.contains("unknown fact invented"), "{all}");
        assert!(all.contains("in that order"), "{all}");
        assert!(all.contains("5 beats"), "{all}");
        assert!(all.contains("characters"), "{all}");
        assert!(parse("观看偏好：番剧很多").is_err());
        let empty = parse("{}").unwrap();
        assert!(validate(&empty.script, style, &facts(), "zh-CN", None).is_err());
    }

    #[test]
    fn mechanical_misses_are_fixed_without_the_model() {
        let style = &builtin_styles()[0];
        let mut report = parse(&good()).unwrap();
        report.script.chapters.swap(0, 2);
        report.script.chapters[1].beats[0].fact = Some("invented".into());
        report.script.chapters[1].beats.push(Beat {
            text: "x".repeat(200),
            fact: None,
        });
        report.script.chapters[1].title.clear();
        report.script.chapters.push(Chapter {
            key: "epilogue".into(),
            title: "t".into(),
            beats: vec![],
        });
        assert!(validate(&report.script, style, &facts(), "zh-CN", None).is_err());
        let changes = repair(&mut report.script, style, &facts(), "zh-CN", None);
        assert!(changes.len() >= 4, "{changes:?}");
        validate(&report.script, style, &facts(), "zh-CN", None).unwrap();
        assert_eq!(report.script.chapters[1].title, "口味");
        assert_eq!(report.script.chapters[1].beats[0].fact, None);

        // A missing chapter or a bad headline still needs the model.
        let mut short = parse(&good()).unwrap();
        short.script.chapters.pop();
        short.script.headline.clear();
        repair(&mut short.script, style, &facts(), "zh-CN", None);
        let problems = validate(&short.script, style, &facts(), "zh-CN", None).unwrap_err();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            repair(
                &mut parse(&good()).unwrap().script,
                style,
                &facts(),
                "zh-CN",
                None
            )
            .is_empty()
        );
    }

    #[test]
    fn latin_locales_get_room_for_longer_lines() {
        let style = &builtin_styles()[0];
        let mut report = parse(&good()).unwrap();
        report.script.chapters[0].beats[0].text = "y".repeat(100);
        assert!(validate(&report.script, style, &facts(), "zh-CN", None).is_err());
        assert!(validate(&report.script, style, &facts(), "en-US", None).is_ok());
    }

    #[test]
    fn a_preview_checks_only_its_chapter() {
        let style = &builtin_styles()[0];
        let mut report = parse(&good()).unwrap();
        report.script.chapters.retain(|c| c.key == "taste");
        validate(&report.script, style, &facts(), "zh-CN", Some("taste")).unwrap();
        assert!(validate(&report.script, style, &facts(), "zh-CN", None).is_err());
        let schema = schema(style, Some("taste"));
        assert_eq!(
            schema["properties"]["chapters"]["items"]["properties"]["key"]["enum"],
            json!(["taste"])
        );
    }

    #[test]
    fn the_mock_fills_the_style_chapters_in_order() {
        let style = &builtin_styles()[0];
        let insights = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let script = from_mock("画像", &insights, style, "zh-CN");
        assert_eq!(summary_of(&script), "画像");
        assert_eq!(insights_of(&script), insights);
        let keys: Vec<_> = script.chapters.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, vec!["portrait", "taste", "highlight"]);
        assert!(script.chapters.iter().all(|c| !c.title.is_empty()));

        let mut one = style.clone();
        one.chapters = vec!["portrait".into()];
        let script = from_mock("画像", &insights, &one, "zh-CN");
        assert_eq!(script.chapters.len(), 1);
        assert_eq!(summary_of(&script), "画像");
        assert_eq!(insights_of(&script), insights);

        let bare = from_mock("画像", &[], style, "zh-CN");
        assert_eq!(insights_of(&bare), vec!["画像"]);
    }
}
