//! A group's own words (see `myriad_merope::jargon`): a word she was unsure
//! of and could not look up is watched in that group; as it keeps coming up
//! she works out what it means there, beside what it usually means, and
//! keeps it as the group's when the two differ. Worked out on the site
//! owner's account, who hosts her in the groups.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

#[cfg(test)]
use myriad_merope::jargon::Known;
use myriad_merope::jargon::{
    HERE_SCHEMA, USUAL_SCHEMA, Watched, hear, here_input, here_schema, here_system, parse_here,
    parse_usual, usual_input, usual_schema, usual_system, watch,
};
use sea_orm::DatabaseConnection;
use serde_json::json;

use super::call::{self, Voice};
use crate::services::agent::memory::unified;

/// Rows of a group's own words, kept in that group.
pub const SOURCE: &str = "jargon";
const NAMESPACE: &str = "merope_jargon_watched";
/// Watched words of a group no one used for this long are let go.
const WATCHED_FOR_DAYS: i64 = 30;
/// A group's word no one brought up again in this long fades.
const FADES_AFTER: chrono::Duration = chrono::Duration::days(90);
const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// Words of the groups read back.
#[cfg(test)]
const READ_BACK: u64 = 300;

/// Each group's watched words, as last read or written.
static WATCHED: LazyLock<Mutex<HashMap<String, Vec<Watched>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn identity() -> crate::services::runtime_registry::RegistryIdentity<'static> {
    crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

/// The group `venue`'s watched words, read back once per process.
async fn watched_in(db: &DatabaseConnection, venue: &str) -> Vec<Watched> {
    if let Some(watched) = WATCHED.lock().ok().and_then(|all| all.get(venue).cloned()) {
        return watched;
    }
    let watched = crate::services::runtime_registry::get::<Vec<Watched>>(db, NAMESPACE, venue)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    if let Ok(mut all) = WATCHED.lock() {
        all.entry(venue.to_string())
            .or_insert_with(|| watched.clone());
    }
    watched
}

async fn keep_watched(db: &DatabaseConnection, venue: &str, watched: Vec<Watched>) {
    if let Ok(mut all) = WATCHED.lock() {
        all.insert(venue.to_string(), watched.clone());
    }
    let until = (crate::clock::now() + chrono::Duration::days(WATCHED_FOR_DAYS)).timestamp();
    if let Err(error) =
        crate::services::runtime_registry::put(db, NAMESPACE, venue, identity(), &watched, until)
            .await
    {
        tracing::warn!(%error, "[Merope] could not keep the words she watches in a group");
    }
}

/// Forget the words she watches in every group, with the persona.
pub async fn forget_watched<C: sea_orm::ConnectionTrait>(db: &C) -> Result<u64, sea_orm::DbErr> {
    if let Ok(mut all) = WATCHED.lock() {
        all.clear();
    }
    crate::services::runtime_registry::delete_matching(db, NAMESPACE, None, None, None, None).await
}

/// Watch `term` in the group `venue` (as sessions keep it): she was unsure
/// of it and could not look it up. `used` are lines of the talk using it;
/// hers (`you：…`) do not count.
pub(crate) async fn nominate(db: &DatabaseConnection, venue: &str, term: &str, used: &[String]) {
    let theirs: Vec<String> = used
        .iter()
        .filter(|line| !line.starts_with("you："))
        .cloned()
        .collect();
    let mut watched = watched_in(db, venue).await;
    watch(&mut watched, term, &theirs);
    let due: Vec<Watched> = watched.iter().filter(|word| word.due()).cloned().collect();
    keep_watched(db, venue, watched).await;
    for word in due {
        work_out(venue.to_string(), word);
    }
}

/// A line someone else said in the group `venue`: the watched words in it
/// count, and those that came up enough are worked out.
pub fn heard(venue: &str, line: &str) {
    let has_any = WATCHED.lock().ok().is_some_and(|all| {
        all.get(venue)
            .is_some_and(|watched| watched.iter().any(|word| line.contains(&word.term)))
    });
    let known = WATCHED
        .lock()
        .ok()
        .is_some_and(|all| all.contains_key(venue));
    if known && !has_any {
        return;
    }
    let (venue, line) = (venue.to_string(), line.to_string());
    super::background::spawn("a word heard in a group", async move {
        let Ok(db) = crate::services::process_db::database() else {
            return;
        };
        let mut watched = watched_in(&db, &venue).await;
        let due = hear(&mut watched, &line);
        if due.is_empty() && !watched.iter().any(|word| line.contains(&word.term)) {
            return;
        }
        let due: Vec<Watched> = due
            .into_iter()
            .map(|index| watched[index].clone())
            .collect();
        keep_watched(&db, &venue, watched).await;
        for word in due {
            work_out(venue.clone(), word);
        }
    });
}

/// Work out what `word` means in the group `venue`, in the background, and
/// keep it as the group's own when it differs from what it usually means.
fn work_out(venue: String, word: Watched) {
    super::background::spawn("a group's own words", async move {
        let Ok(db) = crate::services::process_db::database() else {
            return;
        };
        let Some(owner) = super::call::site_owner().await else {
            return;
        };
        // Worked out at these uses, found or not: not asked again until more.
        let mut watched = watched_in(&db, &venue).await;
        if let Some(kept) = watched.iter_mut().find(|kept| kept.term == word.term) {
            kept.worked_out_at = kept.uses;
        }
        keep_watched(&db, &venue, watched).await;
        let Some(meaning) = own_meaning(owner, &word.term, &word.lines).await else {
            tracing::info!(%venue, "[Merope] a word she watched is not the group's own");
            return;
        };
        keep(&db, &venue, &word, &meaning).await;
    });
}

/// What `term` means as the group uses it in `lines`, when that is the
/// group's own: what it usually means first, from the word alone, then the
/// group's use beside it. Billed to `owner`.
async fn own_meaning(owner: i32, term: &str, lines: &[String]) -> Option<String> {
    let usual = call::Ask::new(Voice::Thorough, owner, "word_usually")
        .within(CALL_TIMEOUT)
        .json_raw(
            usual_system(),
            &usual_input(term),
            USUAL_SCHEMA,
            &usual_schema(),
        )
        .await
        .ok()
        .and_then(|raw| parse_usual(&raw))
        .flatten();
    let raw = call::Ask::new(Voice::Thorough, owner, "word_here")
        .within(CALL_TIMEOUT)
        .json_raw(
            here_system(),
            &here_input(term, lines, usual.as_deref()),
            HERE_SCHEMA,
            &here_schema(),
        )
        .await
        .ok()?;
    parse_here(&raw).flatten()
}

/// Keep what `word` means in the group `venue`, in place of what she made of
/// it before.
async fn keep(db: &DatabaseConnection, venue: &str, word: &Watched, meaning: &str) {
    let stored = unified::Audience::group(venue, 0).venue();
    let marker = format!("\"term\":{}", serde_json::Value::String(word.term.clone()));
    let before = unified::unowned_with_evidence(db, &stored, SOURCE, &marker)
        .await
        .ok()
        .flatten();
    // What someone there told her it means stands over what she made of it.
    if before
        .as_ref()
        .and_then(|before| before.evidence.as_deref())
        .and_then(|evidence| serde_json::from_str::<serde_json::Value>(evidence).ok())
        .is_some_and(|evidence| evidence.get("told").is_some())
    {
        return;
    }
    let evidence = json!({ "term": word.term, "uses": word.uses }).to_string();
    let content = myriad_merope::memes::kept_line(&word.term, meaning);
    match unified::remember_in_venue(db, &stored, &content, &evidence, SOURCE).await {
        Ok(Some(id)) => {
            if let Some(before) = before.filter(|before| before.id != id) {
                let _ = unified::retire_unowned_in_favor_of(
                    db,
                    &stored,
                    &before.id,
                    "superseded",
                    Some(&id),
                )
                .await;
            }
            tracing::info!(%venue, "[Merope] worked out a word of the group's own");
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not keep a group's word"),
    }
    if let Err(error) = unified::fade_source(db, SOURCE, FADES_AFTER).await {
        tracing::warn!(%error, "[Merope] could not let old group words fade");
    }
}

/// Someone in the group `venue` (as sessions keep it) told her what a word
/// of theirs means: kept as the group's, as told, in place of what she made
/// of it, and no longer watched.
pub async fn told(
    db: &DatabaseConnection,
    venue: &str,
    explained: &myriad_merope::group_reading::Explained,
) {
    let stored = unified::Audience::group(venue, 0).venue();
    let marker = format!(
        "\"term\":{}",
        serde_json::Value::String(explained.term.clone())
    );
    let before = unified::unowned_with_evidence(db, &stored, SOURCE, &marker)
        .await
        .ok()
        .flatten();
    let evidence = json!({
        "term": explained.term,
        "told": explained.who,
        "quote": explained.quote,
    })
    .to_string();
    let content = myriad_merope::memes::kept_line(&explained.term, &explained.meaning);
    match unified::remember_in_venue(db, &stored, &content, &evidence, SOURCE).await {
        Ok(Some(id)) => {
            if let Some(before) = before.filter(|before| before.id != id) {
                let _ = unified::retire_unowned_in_favor_of(
                    db,
                    &stored,
                    &before.id,
                    "superseded",
                    Some(&id),
                )
                .await;
            }
            tracing::info!(%venue, "[Merope] kept a group's word as someone there told it");
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not keep a group's word"),
    }
    let mut watched = watched_in(db, venue).await;
    let before = watched.len();
    watched.retain(|word| word.term != explained.term);
    if watched.len() != before {
        keep_watched(db, venue, watched).await;
    }
}

/// The groups' own words `text`, said in the group `here` (as sessions keep
/// it), uses: this group's, and other groups' by their names.
#[cfg(test)]
pub async fn section_for(db: &DatabaseConnection, here: &str, text: &str) -> Option<String> {
    let rows = unified::in_other_groups(db, None, "", &[SOURCE], READ_BACK).await;
    let here_stored = unified::Audience::group(here, 0).venue();
    let mut known = Vec::new();
    for row in rows {
        let Some(word) = myriad_merope::memes::read_kept(&row.content) else {
            continue;
        };
        if !text.contains(&word.term) {
            continue;
        }
        let (group, elsewhere) = if row.venue == here_stored {
            (None, false)
        } else {
            let venue = row.venue.strip_prefix("group:").unwrap_or(&row.venue);
            (crate::services::channel_group::title(venue).await, true)
        };
        known.push(Known {
            term: word.term,
            meaning: word.meaning,
            group,
            elsewhere,
        });
    }
    myriad_merope::jargon::section(&known)
}

#[cfg(test)]
mod tests {
    /// Whether the site's judgment model tells a group's own word from one
    /// used as it usually is, from how people there use it.
    /// `cargo test --bin myriad-backend a_groups_own_word -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "spends on the site's judgment model"]
    async fn a_groups_own_word_is_told_from_a_usual_one() {
        let config_db = super::super::super::semantic_eval::load_configured_lite().await;
        config_db.close().await.ok();
        let cases: [(&str, [&str; 4], bool); 3] = [
            (
                "阿伟",
                [
                    "阿伟又在群里发红包了 群主就是大方",
                    "群公告是阿伟昨天改的",
                    "@阿伟 你这个群主能不能管管刷屏的",
                    "阿伟说周末全群线下聚会 他请客",
                ],
                true,
            ),
            (
                "yyds",
                [
                    "这家火锅yyds",
                    "周杰伦yyds 不接受反驳",
                    "这个插件yyds 省了我一小时",
                    "新出的角色yyds",
                ],
                false,
            ),
            (
                "饭团",
                [
                    "饭团又炸服了 谁去重启一下",
                    "谁还没有饭团的白名单",
                    "饭团今晚十点维护 别上线",
                    "我在饭团里盖了个城堡 坐标发群里了",
                ],
                true,
            ),
        ];
        for (term, lines, own) in cases {
            let lines: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
            let meaning = super::own_meaning(1, term, &lines).await;
            println!("{term}: {meaning:?}");
            assert_eq!(meaning.is_some(), own, "{term}: {meaning:?}");
        }
    }

    /// The whole way, on its own schema with the site's judgment model: a
    /// word she could not look up is watched, counted as people use it,
    /// worked out at the fourth use, kept as the group's own, and known again
    /// when it comes up there, and from another group as that group's.
    /// `MYRIAD_MEDIA_TEST_DATABASE_URL=… cargo test --bin myriad-backend a_word_is_watched -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "spends on the site's judgment model"]
    async fn a_word_is_watched_counted_worked_out_and_known_again() {
        use sea_orm::ConnectionTrait;
        let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("a test database");
        let config_db = super::super::super::semantic_eval::load_configured_lite().await;
        config_db.close().await.ok();
        let isolated = crate::db::IsolatedSchema::migrated(&url, "jargon_whole_way").await;
        let db = isolated.db.clone();
        db.execute_unprepared(
            "INSERT INTO users (username, display_name, is_admin, is_owner) \
             VALUES ('owner', 'owner', true, true)",
        )
        .await
        .unwrap();
        crate::services::process_db::set_process_database(db.clone());
        let venue = "onebot:424242";
        super::nominate(
            &db,
            venue,
            "饭团",
            &["you：饭团是什么".into(), "饭团又炸服了 谁去重启一下".into()],
        )
        .await;
        for line in [
            "谁还没有饭团的白名单",
            "饭团今晚十点维护 别上线",
            "我在饭团里盖了个城堡 坐标发群里了",
        ] {
            super::heard(venue, line);
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        }
        let mut here = None;
        for _ in 0..90 {
            here = super::section_for(&db, venue, "今晚饭团开吗").await;
            if here.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        let here = here.expect("worked out and kept");
        println!("{here}");
        assert!(here.contains("饭团（here）"), "{here}");
        let elsewhere = super::section_for(&db, "onebot:7", "饭团今天开了吗")
            .await
            .expect("known from another group");
        println!("{elsewhere}");
        assert!(elsewhere.contains("饭团（in "), "{elsewhere}");
        assert!(
            super::section_for(&db, venue, "今天吃什么").await.is_none(),
            "only when the talk uses it"
        );
        isolated.drop().await;
    }
}
