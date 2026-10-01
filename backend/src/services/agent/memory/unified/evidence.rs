//! Evidence kept with a memory: bounded so it fits, and records once cut short mended.

use super::*;

/// Evidence is a quote or a small record of what it was, not the material.
pub(super) const MAX_EVIDENCE_CHARS: usize = 4000;

/// Records exactly this long may be cut mid-way (see `mend_cut_evidence`).
pub(super) const EVIDENCE_ONCE_CUT_AT: i32 = 400;

/// Evidence as kept: whole, since a record cut mid-way cannot be read back.
/// Past the cap, a JSON object loses its largest fields (never `key` or
/// `thing`, what it is) until it fits; anything else is cut.
pub fn bounded_evidence(evidence: &str) -> String {
    if evidence.chars().count() <= MAX_EVIDENCE_CHARS {
        return evidence.to_string();
    }
    if let Ok(serde_json::Value::Object(mut record)) = serde_json::from_str(evidence) {
        loop {
            let text = serde_json::Value::Object(record.clone()).to_string();
            if text.chars().count() <= MAX_EVIDENCE_CHARS {
                return text;
            }
            let largest = record
                .iter()
                .filter(|(field, _)| !matches!(field.as_str(), "key" | "thing"))
                .max_by_key(|(_, value)| value.to_string().len())
                .map(|(field, _)| field.clone());
            match largest {
                Some(field) => {
                    record.remove(&field);
                }
                None => break,
            }
        }
    }
    evidence.chars().take(MAX_EVIDENCE_CHARS).collect()
}

/// A JSON object cut short, made whole by leaving off the field it was cut
/// in: every field before it stays as it was. None if it cannot be.
pub fn mend_cut_record(cut: &str) -> Option<String> {
    if !cut.starts_with('{') || serde_json::from_str::<serde_json::Value>(cut).is_ok() {
        return None;
    }
    // Where each top-level field after the first begins.
    let mut boundaries = Vec::new();
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (at, ch) in cut.char_indices() {
        if in_string {
            match ch {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 1 => boundaries.push(at),
            _ => {}
        }
    }
    boundaries.into_iter().rev().find_map(|at| {
        let whole = format!("{}}}", &cut[..at]);
        matches!(
            serde_json::from_str::<serde_json::Value>(&whole),
            Ok(serde_json::Value::Object(_))
        )
        .then_some(whole)
    })
}

/// Records cut short mid-way at `EVIDENCE_ONCE_CUT_AT`, mended: how many.
pub async fn mend_cut_evidence<C: ConnectionTrait>(db: &C) -> Result<u64, DbErr> {
    let rows = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT id, evidence FROM agent_memories \
             WHERE char_length(evidence) = $1 AND evidence LIKE '{%'",
            [EVIDENCE_ONCE_CUT_AT.into()],
        ))
        .await?;
    let mut mended = 0;
    for row in &rows {
        let (Ok(id), Ok(evidence)) = (
            row.try_get::<String>("", "id"),
            row.try_get::<String>("", "evidence"),
        ) else {
            continue;
        };
        let Some(whole) = mend_cut_record(&evidence) else {
            continue;
        };
        agent_memories::Entity::update_many()
            .col_expr(
                agent_memories::Column::Evidence,
                sea_orm::sea_query::Expr::value(whole),
            )
            .filter(agent_memories::Column::Id.eq(id))
            .exec(db)
            .await?;
        mended += 1;
    }
    Ok(mended)
}
