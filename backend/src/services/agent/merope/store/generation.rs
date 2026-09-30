//! Portrait and sticker-avatar generation leases on the persona row.

use super::*;

/// Acquire the single site-portrait generation lease only while the exact
/// visual inputs still match. The lease lives in the existing JSON document so
/// it is shared by every backend replica without adding a second source of
/// truth. A crashed request becomes replaceable after fifteen minutes.
pub async fn acquire_portrait_generation<C>(
    db: &C,
    expected_name: &str,
    expected_visual_profile: &Value,
    pending: &Value,
) -> Result<bool, anyhow::Error>
where
    C: ConnectionTrait,
{
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE agent_persona
SET portrait_generation = jsonb_set(
        COALESCE(portrait_generation, '{}'::jsonb),
        '{pending}',
        $1::jsonb,
        true
    ),
    updated_at = CURRENT_TIMESTAMP
WHERE id = $2
  AND name = $3
  AND visual_profile = $4::jsonb
  AND (
      portrait_generation IS NULL
      OR NOT (portrait_generation ? 'pending')
      OR updated_at < CURRENT_TIMESTAMP - INTERVAL '15 minutes'
  )
"#,
            vec![
                pending.clone().into(),
                PERSONA_ROW_ID.into(),
                expected_name.into(),
                expected_visual_profile.clone().into(),
            ],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Remove only this request's lease while preserving the last confirmed
/// portrait contract, if any. Another request's error path cannot unlock this token.
pub async fn release_portrait_generation<C>(db: &C, token: &str) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
UPDATE agent_persona
SET portrait_generation = CASE
        WHEN (portrait_generation - 'pending') = '{}'::jsonb THEN NULL
        ELSE portrait_generation - 'pending'
    END,
    updated_at = CURRENT_TIMESTAMP
WHERE id = $1
  AND portrait_generation #>> '{pending,token}' = $2
"#,
        vec![PERSONA_ROW_ID.into(), token.into()],
    ))
    .await?;
    Ok(())
}

/// Commit generated pixels only if this request still owns the lease and the
/// visual inputs have not changed. Spoken-persona fields are intentionally not
/// written here, so edits made during a slow image request are preserved.
pub async fn complete_portrait_generation<C>(
    db: &C,
    expected_name: &str,
    expected_visual_profile: &Value,
    token: &str,
    portrait_asset_id: &str,
    portrait_generation: &Value,
    updated_by: i32,
) -> Result<bool, anyhow::Error>
where
    C: ConnectionTrait,
{
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE agent_persona
SET portrait_asset_id = $1,
    portrait_generation = $2::jsonb,
    avatar_asset_id = NULL,
    avatar_generation = NULL,
    updated_by = $3,
    updated_at = CURRENT_TIMESTAMP
WHERE id = $4
  AND name = $5
  AND visual_profile = $6::jsonb
  AND portrait_generation #>> '{pending,token}' = $7
"#,
            vec![
                portrait_asset_id.into(),
                portrait_generation.clone().into(),
                updated_by.into(),
                PERSONA_ROW_ID.into(),
                expected_name.into(),
                expected_visual_profile.clone().into(),
                token.into(),
            ],
        ))
        .await?;
    let committed = result.rows_affected() == 1;
    if committed {
        // 新主立绘把旧贴纸头像一起作废了，选它的人不能停在旧脸上。
        resync_persona_avatar_snapshots(db, None).await?;
    }
    Ok(committed)
}

/// 贴纸头像的单次生成租约。和主立绘那把锁同一套形状，只是锚点多一个
/// `portrait_asset_id`——头像的血统在主立绘上，主立绘在生成途中被换掉，
/// 这批像素就已经作废了。崩溃的请求十五分钟后可被顶替。
pub async fn acquire_avatar_generation<C>(
    db: &C,
    expected_name: &str,
    expected_visual_profile: &Value,
    expected_portrait_asset_id: &str,
    pending: &Value,
) -> Result<bool, anyhow::Error>
where
    C: ConnectionTrait,
{
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE agent_persona
SET avatar_generation = jsonb_set(
        COALESCE(avatar_generation, '{}'::jsonb),
        '{pending}',
        $1::jsonb,
        true
    ),
    updated_at = CURRENT_TIMESTAMP
WHERE id = $2
  AND name = $3
  AND visual_profile = $4::jsonb
  AND portrait_asset_id = $5
  AND (
      avatar_generation IS NULL
      OR NOT (avatar_generation ? 'pending')
      OR updated_at < CURRENT_TIMESTAMP - INTERVAL '15 minutes'
  )
"#,
            vec![
                pending.clone().into(),
                PERSONA_ROW_ID.into(),
                expected_name.into(),
                expected_visual_profile.clone().into(),
                expected_portrait_asset_id.into(),
            ],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

/// 只摘掉本次请求的锁，保住上一份已确认的头像契约。旧请求的错误路径永远
/// 解不开新请求的锁。
pub async fn release_avatar_generation<C>(db: &C, token: &str) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
UPDATE agent_persona
SET avatar_generation = CASE
        WHEN (avatar_generation - 'pending') = '{}'::jsonb THEN NULL
        ELSE avatar_generation - 'pending'
    END,
    updated_at = CURRENT_TIMESTAMP
WHERE id = $1
  AND avatar_generation #>> '{pending,token}' = $2
"#,
        vec![PERSONA_ROW_ID.into(), token.into()],
    ))
    .await?;
    Ok(())
}

/// URL normalization changes no visual inputs and must not invalidate generated art.
pub async fn rewrite_persona_media_urls<C: ConnectionTrait>(
    db: &C,
    persona: agent_persona::Model,
    portrait: Option<String>,
    avatar: Option<String>,
) -> Result<agent_persona::Model, anyhow::Error> {
    let mut active: agent_persona::ActiveModel = persona.into();
    active.portrait_asset_id = Set(portrait);
    active.avatar_asset_id = Set(avatar);
    let saved = active.update(db).await?;
    resync_persona_avatar_snapshots(db, saved.avatar_asset_id.as_deref()).await?;
    Ok(saved)
}

/// 只在本次请求仍持锁、且名字、外观与主立绘都没变时落盘。
pub async fn complete_avatar_generation<C>(
    db: &C,
    expected_name: &str,
    expected_visual_profile: &Value,
    expected_portrait_asset_id: &str,
    token: &str,
    avatar_asset_id: &str,
    avatar_generation: &Value,
    updated_by: i32,
) -> Result<bool, anyhow::Error>
where
    C: ConnectionTrait,
{
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE agent_persona
SET avatar_asset_id = $1,
    avatar_generation = $2::jsonb,
    updated_by = $3,
    updated_at = CURRENT_TIMESTAMP
WHERE id = $4
  AND name = $5
  AND visual_profile = $6::jsonb
  AND portrait_asset_id = $7
  AND avatar_generation #>> '{pending,token}' = $8
"#,
            vec![
                avatar_asset_id.into(),
                avatar_generation.clone().into(),
                updated_by.into(),
                PERSONA_ROW_ID.into(),
                expected_name.into(),
                expected_visual_profile.clone().into(),
                expected_portrait_asset_id.into(),
                token.into(),
            ],
        ))
        .await?;
    let committed = result.rows_affected() == 1;
    if committed {
        resync_persona_avatar_snapshots(db, Some(avatar_asset_id)).await?;
    }
    Ok(committed)
}

pub fn avatar_generation_is_pending(value: Option<&Value>) -> bool {
    value
        .and_then(|document| document.get("pending"))
        .and_then(|pending| pending.get("token"))
        .and_then(Value::as_str)
        .is_some_and(|token| !token.is_empty())
}

/// 人设贴纸头像：`avatar_asset_id` trim 后空串视为无图。
pub async fn sticker_avatar_asset_id<C>(db: &C) -> Option<String>
where
    C: ConnectionTrait,
{
    get_persona_on(db)
        .await
        .ok()
        .flatten()
        .and_then(|persona| persona.avatar_asset_id)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 贴纸头像动了就得把选它当画像源的人一起带上。列属于
/// [`crate::services::avatar`]，写入时机只有这里知道，所以由这里去调。
pub(super) async fn resync_persona_avatar_snapshots<C>(
    db: &C,
    avatar: Option<&str>,
) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    crate::services::avatar::resync_persona_avatar_snapshots(db, avatar)
        .await
        .map_err(|message| anyhow::anyhow!(message))
}

pub fn portrait_generation_is_pending(value: Option<&Value>) -> bool {
    value
        .and_then(|document| document.get("pending"))
        .and_then(|pending| pending.get("token"))
        .and_then(Value::as_str)
        .is_some_and(|token| !token.is_empty())
}
