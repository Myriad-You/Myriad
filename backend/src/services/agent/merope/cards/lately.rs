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
    /// How she is told of it: a fact, never their words. A title already
    /// says what kind of card it is (「要你定：…」「做完了：…」), so the
    /// kind is not said again; only a card with no title is named by it.
    pub fn told(&self) -> String {
        let named = if self.title.trim().is_empty() {
            match self.kind.as_str() {
                "decide" => "要你定",
                "nod" => "你点头就行",
                "status" => "进度",
                "progress" => "仓库进度",
                other => other,
            }
        } else {
            &self.title
        };
        match (&self.chosen, self.waiting) {
            (Some((label, true)), _) => format!("卡片「{named}」：站长选了「{label}」"),
            (Some((label, false)), true) => {
                format!("卡片「{named}」：站长在更早的一版上选了「{label}」，新的一版还在等站长")
            }
            (Some((label, false)), false) => {
                format!("卡片「{named}」：站长在更早的一版上选了「{label}」")
            }
            (None, true) => format!("卡片「{named}」：还在等站长"),
            (None, false) => format!("卡片「{named}」：不用站长答"),
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

#[cfg(test)]
mod tests {
    use super::Lately;

    fn card(kind: &str, title: &str, waiting: bool, chosen: Option<(&str, bool)>) -> Lately {
        Lately {
            id: "card_1".to_string(),
            at: 0,
            title: title.to_string(),
            kind: kind.to_string(),
            waiting,
            chosen: chosen.map(|(label, now)| (label.to_string(), now)),
        }
    }

    #[test]
    fn a_card_is_told_by_its_title_and_its_kind_only_when_it_has_none() {
        let told = [
            card("decide", "要你定：现在推送吗", false, Some(("推送", true))),
            card("nod", "等你点头：拆开存储", true, Some(("好", false))),
            card("progress", "接着在做：整理记忆", false, None),
            card("status", "卡住了：导入立绘", true, None),
        ]
        .map(|one| one.told());
        assert_eq!(
            told,
            [
                "卡片「要你定：现在推送吗」：站长选了「推送」",
                "卡片「等你点头：拆开存储」：站长在更早的一版上选了「好」，新的一版还在等站长",
                "卡片「接着在做：整理记忆」：不用站长答",
                "卡片「卡住了：导入立绘」：还在等站长",
            ]
        );
        for line in &told {
            for kind in ["（要你定）", "（你点头就行）", "（进度）", "（仓库进度）"]
            {
                assert!(!line.contains(kind), "{line}");
            }
        }

        let untitled = [
            card("decide", "", true, None),
            card("nod", " ", false, Some(("好", false))),
            card("status", "", false, None),
            card("progress", "", true, None),
            card("question", "", true, None),
        ]
        .map(|one| one.told());
        assert_eq!(
            untitled,
            [
                "卡片「要你定」：还在等站长",
                "卡片「你点头就行」：站长在更早的一版上选了「好」",
                "卡片「进度」：不用站长答",
                "卡片「仓库进度」：还在等站长",
                "卡片「question」：还在等站长",
            ]
        );
    }
}
