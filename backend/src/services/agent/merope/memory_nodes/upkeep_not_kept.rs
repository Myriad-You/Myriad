//! What her upkeep writes of someone's private talk, without what they asked
//! her not to keep (`asked`): a version of hers (a summary, a view, what is
//! not wrapped up, how their life stands, what she means to do) that says
//! any of it is left out, with every one of the same run written on it. What
//! she files or joins is of what was kept already, and what she closes goes
//! as it would.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{RowIn, audience};

/// The words of a version that says something new.
fn words(row: &RowIn) -> Option<String> {
    let body = &row.body;
    let says = matches!(
        row.kind.as_str(),
        "node.revised" | "node.view_changed" | "node.open_item" | "node.standing" | "node.intent"
    );
    if !says
        || body["closed"] == true
        || body["dropped"] == true
        || body["status"]
            .as_str()
            .is_some_and(|status| status != "waiting")
    {
        return None;
    }
    let mut said: Vec<&str> = Vec::new();
    said.extend(body["text"].as_str());
    match &body["value"] {
        Value::String(text) => said.push(text),
        Value::Array(sentences) => said.extend(
            sentences
                .iter()
                .filter_map(|sentence| sentence["text"].as_str().or(sentence.as_str())),
        ),
        value => said.extend(value["text"].as_str()),
    }
    let said = said.join(" ");
    (!said.trim().is_empty()).then_some(said)
}

/// `rows` of `user_id`'s `audience` without what they asked her not to keep;
/// `None` when that could not be told. A group's are written as they are.
pub(super) async fn leave_out(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    audience_of: &str,
    rows: Vec<RowIn>,
) -> Option<Vec<RowIn>> {
    let theirs =
        audience_of == audience::owners_talk() || audience_of == format!("personal:{user_id}");
    if !theirs {
        return Some(rows);
    }
    let (ids, items): (Vec<String>, Vec<String>) = rows
        .iter()
        .filter_map(|row| Some((row.id.clone(), words(row)?)))
        .unzip();
    let out = super::super::asked::to_leave_out_or_again(db, user_id, &items)
        .await
        .ok()?;
    let mut out: BTreeSet<String> = out.into_iter().map(|index| ids[index].clone()).collect();
    if out.is_empty() {
        return Some(rows);
    }
    loop {
        let more: Vec<String> = rows
            .iter()
            .filter(|row| !out.contains(&row.id))
            .filter(|row| {
                row.body["base"].as_array().is_some_and(|base| {
                    base.iter()
                        .any(|on| on.as_str().is_some_and(|on| out.contains(on)))
                })
            })
            .map(|row| row.id.clone())
            .collect();
        if more.is_empty() {
            break;
        }
        out.extend(more);
    }
    let mut rows = rows;
    rows.retain(|row| !out.contains(&row.id));
    // Her mark says what the run wrote.
    for row in rows.iter_mut().filter(|row| row.kind == "node.upkept") {
        if let Some(wrote) = row.body["wrote"].as_array_mut() {
            wrote.retain(|id| id.as_str().is_none_or(|id| !out.contains(id)));
        }
    }
    Some(rows)
}

#[cfg(test)]
#[path = "upkeep_not_kept_tests.rs"]
mod tests;
