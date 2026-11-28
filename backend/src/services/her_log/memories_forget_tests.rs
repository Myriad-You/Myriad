//! A memory another end forgot, taken in here: what she said in her answer
//! to the line it came from goes too (a database of its own; skipped without
//! `MYRIAD_MEDIA_TEST_DATABASE_URL`). Every word here is made up.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use super::super::keys::test_root::TestRoot;
use super::super::{state, store};
use super::take_in;

const OWNER: i32 = 41;
const PARTNER: &str = "partner_019b76daa80000112233445566778899";

async fn texts(db: &DatabaseConnection, sql: &str) -> Vec<String> {
    db.query_all_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get_by_index::<String>(0).unwrap())
        .collect()
}

#[tokio::test]
async fn what_she_said_in_her_answer_goes_with_a_memory_forgotten_elsewhere() {
    let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
        return;
    };
    let schema = crate::db::IsolatedSchema::migrated(&url, "her_log_said_answer").await;
    let db = &schema.db;
    db.execute_unprepared(&format!(
        "INSERT INTO users (id, username, is_owner) VALUES ({OWNER}, 'owner', true);
         INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
           created_at, last_active_at) \
         VALUES ('ses_a', {OWNER}, 'a', '{{\"mode\":\"chat\"}}', 4, false, NOW(), NOW());
         INSERT INTO agent_messages (id, session_id, role, content, metadata, created_at) VALUES
           (901, 'ses_a', 'user', '我上个月确诊了糖尿病', '{{\"told\":[\"diagnosis\"]}}', \
            NOW() - INTERVAL '60 minutes'),
           (902, 'ses_a', 'assistant', '那你记得每天按时吃药', '{{\"told\":[\"meds\"]}}', \
            NOW() - INTERVAL '59 minutes'),
           (903, 'ses_a', 'user', '今天天气不错', NULL, NOW() - INTERVAL '50 minutes'),
           (904, 'ses_a', 'assistant', '适合出去走走', NULL, NOW() - INTERVAL '49 minutes');
         INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, \
           venue, audience, concepts, valid_from, created_at, updated_at) VALUES
           ('diagnosis', {OWNER}, 'fact', '上个月确诊了糖尿病', '我上个月确诊了糖尿病', 'user', \
            'chat', 'private', '[{OWNER}]', '[]', NOW() - INTERVAL '58 minutes', \
            NOW() - INTERVAL '58 minutes', NOW()),
           ('meds', {OWNER}, 'fact', '提醒过按时吃药', '记得每天按时吃药', 'agent', 'said', \
            'private', '[{OWNER}]', '[]', NOW() - INTERVAL '55 minutes', \
            NOW() - INTERVAL '55 minutes', NOW()),
           ('tea', {OWNER}, 'preference', '喜欢茶', '喜欢茶', 'user', 'chat', 'private', \
            '[{OWNER}]', '[]', NOW(), NOW(), NOW());"
    ))
    .await
    .unwrap();
    store::start(db, OWNER).await.unwrap();
    super::super::test_partner::give(db, OWNER, PARTNER).await;
    let root = TestRoot([9; 32]);
    let owner = super::super::owner(db, OWNER).await.unwrap();
    state::sync(db, &root, OWNER).await.unwrap();
    // Forgotten on another end.
    store::forget(db, &owner, &owner.personal(), "memory:diagnosis")
        .await
        .unwrap();
    take_in(db, &root, &owner).await.unwrap();
    assert_eq!(
        texts(db, "SELECT id FROM agent_memories ORDER BY id").await,
        ["tea"]
    );
    assert_eq!(
        texts(db, "SELECT content FROM agent_messages ORDER BY created_at").await,
        ["今天天气不错", "适合出去走走"]
    );
    schema.drop().await;
}
