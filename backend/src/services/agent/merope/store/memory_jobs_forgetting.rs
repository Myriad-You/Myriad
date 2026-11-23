//! What is forgotten, taken out of what is still queued.

use super::*;

/// Forgetting: drops every job of `owner`'s still holding `quote` in the
/// words it is to go over (all a line of them says, when it is a whole
/// line's), and every stretch of a group's talk put by with it, whoever it
/// is kept under, so nothing still queued writes it back. A job being gone
/// over is fenced too: its finish finds no job and writes nothing. Run after
/// [`lock`], on the forgetting's transaction.
pub(in crate::services::agent) async fn forget_holding(
    db: &impl ConnectionTrait,
    owner: i32,
    quote: &crate::services::agent::memory::origins::Quote,
) -> Result<u64, sea_orm::DbErr> {
    use crate::services::agent::memory::origins::folded;
    let words = [
        "data,Chat,user_text",
        "data,Chat,reply",
        "next,Chat,user_text",
        "next,Chat,reply",
    ]
    .map(|path| format!("payload #>> '{{{path}}}'"));
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT record_id, ARRAY_REMOVE(ARRAY[{}], NULL) AS words FROM runtime_registry \
                 WHERE namespace = $1 AND (payload ->> 'owner')::int = $2 \
                 UNION ALL \
                 SELECT record_id, ARRAY(SELECT line ->> 2 FROM jsonb_array_elements( \
                   payload #> '{{data,GroupHeard,stretch}}') AS line) \
                 FROM runtime_registry WHERE namespace = $3 \
                   AND payload #> '{{data,GroupHeard,stretch}}' IS NOT NULL",
                words.join(", "),
            ),
            [NAMESPACE.into(), owner.into(), ASKED_NAMESPACE.into()],
        ))
        .await?;
    let mut held: Vec<String> = Vec::new();
    for row in rows {
        // What was said a line at a time is folded into one, a line each.
        let words: Vec<String> = row.try_get("", "words")?;
        if words.iter().any(|words| {
            quote.is_in(&folded(words)) || words.lines().any(|line| quote.is_in(&folded(line)))
        }) {
            held.push(row.try_get("", "record_id")?);
        }
    }
    if held.is_empty() {
        return Ok(0);
    }
    Ok(db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM runtime_registry WHERE namespace = ANY($1) AND record_id = ANY($2)",
            [
                vec![NAMESPACE.to_string(), ASKED_NAMESPACE.to_string()].into(),
                held.into(),
            ],
        ))
        .await?
        .rows_affected())
}
