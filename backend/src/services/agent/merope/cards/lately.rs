//! The cards she put before the owner lately, as she is told of them: a
//! fact each, never their words.

use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};
use serde_json::Value;

use super::Held;

/// A card she put before the owner lately, as she is told of it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Lately {
    pub id: String,
    pub at: i64,
    pub title: String,
    pub kind: String,
    /// Waiting on them, or the label of what they chose (and whether it
    /// was on an older version).
    pub waiting: bool,
    pub chosen: Option<(String, bool)>,
}

impl Lately {
    /// How she is told of it: a fact, never their words.
    pub fn told(&self) -> String {
        let kind = match self.kind.as_str() {
            "decide" => "要你定",
            "nod" => "你点头就行",
            "status" => "进度",
            "progress" => "仓库进度",
            other => other,
        };
        match (&self.chosen, self.waiting) {
            (Some((label, true)), _) => {
                format!("卡片「{}」（{kind}）：站长选了「{label}」", self.title)
            }
            (Some((label, false)), true) => format!(
                "卡片「{}」（{kind}）：站长在更早的一版上选了「{label}」，新的一版还在等站长",
                self.title
            ),
            (Some((label, false)), false) => format!(
                "卡片「{}」（{kind}）：站长在更早的一版上选了「{label}」",
                self.title
            ),
            (None, true) => format!("卡片「{}」（{kind}）：还在等站长", self.title),
            (None, false) => format!("卡片「{}」（{kind}）：不用站长答", self.title),
        }
    }
}

/// The cards she put before the owner lately, newest first.
pub(crate) async fn lately<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    limit: i64,
) -> Result<Vec<Lately>, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT metadata FROM agent_proactive_messages \
             WHERE user_id = $1 AND metadata->'card' IS NOT NULL \
             ORDER BY created_at DESC, id DESC LIMIT $2",
            [owner.into(), limit.into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| row.try_get::<Option<Value>>("", "metadata").ok().flatten())
        .filter_map(|metadata| Held::of(&metadata))
        .map(|held| {
            // The answer to what stands now, else the last they gave.
            let answer = held
                .card_answers
                .iter()
                .rev()
                .find(|answer| answer.version == held.card.version)
                .or(held.card_answers.last());
            let chosen = answer.and_then(|answer| {
                let label = held
                    .card_versions
                    .iter()
                    .find(|version| version.id == answer.version)?
                    .actions
                    .iter()
                    .find(|action| action.id == answer.action)?
                    .label
                    .clone();
                Some((label, answer.version == held.card.version))
            });
            Lately {
                id: held.card.card.id.clone(),
                at: held.card.card.at,
                title: held.card.card.title.clone(),
                kind: held.card.card.kind.clone(),
                waiting: held.card.waiting,
                chosen,
            }
        })
        .collect())
}
