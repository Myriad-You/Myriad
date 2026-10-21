//! Writing the script: one model call, one retry that names what was wrong,
//! then the local fallback. Every call is billed to the site owner under
//! source `reports`.

use serde_json::{Value, json};

use crate::api::reports::facts::Fact;
use crate::api::reports::prompts::{ReportPrompt, build_report_prompt};
use crate::api::reports::script::{self, ModelReport, ReportScript};
use crate::config::ModelTier;
use crate::services::ai::create_ai_analyzer_for_tier;
use crate::services::report_styles::ReportStyle;
use crate::services::smart_filter::SmartFilteredData;

/// What one report call is for, as the cost ledger records it.
#[derive(Clone, Copy)]
pub(crate) enum Purpose {
    Report,
    StylePreview,
}

impl Purpose {
    fn operation(self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::StylePreview => "report_style_preview",
        }
    }

    fn task_id(self, platform: &str) -> String {
        match self {
            Self::Report => format!("report:{platform}"),
            Self::StylePreview => format!("report-style-preview:{platform}"),
        }
    }
}

#[derive(Debug)]
pub(crate) enum WriteError {
    /// No Standard-tier model is configured.
    NoModel,
    /// The provider call itself failed.
    CallFailed,
    /// Two answers in a row failed to parse or validate.
    Rejected(Vec<String>),
}

/// The script a report is stored with.
pub(crate) struct Written {
    pub script: ReportScript,
    pub card_visuals: Value,
    /// False for the local fallback, which ignores the style.
    pub by_model: bool,
}

pub(crate) struct WriteRequest<'a> {
    pub metadata: &'a SmartFilteredData,
    pub platform: &'a str,
    pub locale: &'a str,
    pub style: &'a ReportStyle,
    pub facts: &'a [Fact],
    pub only: Option<&'a str>,
    pub purpose: Purpose,
}

/// Ask the model; repair what needs no model; retry once with what is left
/// named; give up after that.
pub(crate) async fn ask_model(request: &WriteRequest<'_>) -> Result<ModelReport, WriteError> {
    let Some(analyzer) = create_ai_analyzer_for_tier(ModelTier::Standard).await else {
        tracing::warn!("AI analyzer unavailable for Standard tier");
        return Err(WriteError::NoModel);
    };
    let data = crate::api::reports::prompt_data::serialize_for_report_prompt(request.metadata)
        .map_err(|error| WriteError::Rejected(vec![error]))?;
    let schema = script::schema(request.style, request.only);

    let admin_id = match crate::services::process_db::database() {
        Ok(db) => crate::services::site_owner::site_owner_user_id(&db)
            .await
            .unwrap_or(1),
        _ => 1,
    };

    let mut problems: Vec<String> = Vec::new();
    for attempt in 0..2 {
        let prompt = build_report_prompt(&ReportPrompt {
            platform: request.platform,
            data: &data,
            locale: request.locale,
            style: request.style,
            facts: request.facts,
            only: request.only,
            problems: &problems,
        });
        // 全站费用账本：source=reports，主体记在站长；含管理员触发
        let attr = crate::services::ai_cost_ledger::AiLedgerAttribution {
            subject_id: admin_id,
            owner_id: admin_id,
            source: "reports".into(),
            operation: request.purpose.operation().into(),
            tapp_id: None,
            task_id: request.purpose.task_id(request.platform),
        };
        let answer = crate::services::ai_cost_ledger::with_ai_ledger_attribution(attr, async {
            analyzer
                .analyze_json("", &prompt, "platform_report", Some(&schema))
                .await
        })
        .await;
        problems = match answer {
            Ok(text) => match script::parse(&text) {
                Ok(mut report) => {
                    // Mechanical misses are fixed here; only what needs the
                    // model again (a missing chapter, a bad headline) costs a
                    // second call with the whole Data.
                    let repairs = script::repair(
                        &mut report.script,
                        request.style,
                        request.facts,
                        request.locale,
                        request.only,
                    );
                    if !repairs.is_empty() {
                        tracing::info!(
                            platform = request.platform,
                            attempt,
                            ?repairs,
                            "report script repaired locally"
                        );
                    }
                    match script::validate(
                        &report.script,
                        request.style,
                        request.facts,
                        request.locale,
                        request.only,
                    ) {
                        Ok(()) => return Ok(report),
                        Err(problems) => problems,
                    }
                }
                Err(error) => vec![format!(
                    "the answer was not the JSON object asked for: {error}"
                )],
            },
            // A failed call is not a bad answer: no retry, the caller falls back.
            Err(error) => {
                tracing::error!(platform = request.platform, %error, "report model call failed");
                return Err(WriteError::CallFailed);
            }
        };
        tracing::warn!(
            platform = request.platform,
            attempt,
            retry = attempt == 0,
            ?problems,
            "report script rejected"
        );
    }
    Err(WriteError::Rejected(problems))
}

/// The local fallback, laid out in the style's chapters.
pub(crate) fn mock(
    metadata: &SmartFilteredData,
    platform: &str,
    locale: &str,
    style: &ReportStyle,
) -> Written {
    let (summary, insights, card_visuals) =
        crate::api::reports::mock::generate_mock_report(metadata, platform, locale)
            .unwrap_or_else(|_| (String::new(), vec![], json!({})));
    Written {
        script: script::from_mock(&summary, &insights, style, locale),
        card_visuals,
        by_model: false,
    }
}

/// The stored report's script: the model's if it passes, else the fallback.
pub(crate) async fn write_or_mock(request: &WriteRequest<'_>) -> Written {
    match ask_model(request).await {
        Ok(report) => {
            tracing::info!("✅ AI report script written for {}", request.platform);
            Written {
                script: report.script,
                card_visuals: report.card_visuals,
                by_model: true,
            }
        }
        Err(error) => {
            tracing::warn!(
                "⚠️ No usable AI report for {}: {:?}, using mock",
                request.platform,
                error
            );
            mock(
                request.metadata,
                request.platform,
                request.locale,
                request.style,
            )
        }
    }
}
