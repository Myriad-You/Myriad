use sea_orm_migration::prelude::*;

/// OAuth Identities + 解禁本地账号
///
/// 详见 docs/development/OAUTH.md
///
/// 改动：
/// 1. 新建 user_identities 表（多对一：一个 user 可挂多个 OAuth identity）
/// 2. 回填现有 users.github_id / linked_github_id → user_identities
/// 3. 解除限制：drop idx_local_admin、drop check_admin_local_only
/// 4. auth_provider 允许值扩展为 ('local','github','oidc','federated')
/// 5. 加 username 全局唯一索引（大小写不敏感，含冲突预处理）
/// 6. 加 allow_local_registration 配置项（默认 false）
/// 7. 用户生命周期：`user_lifecycle.sql`（FK 级联 + 主体表删除触发器）
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ==================== 1. 创建 user_identities 表 ====================
        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS user_identities (
                id                SERIAL PRIMARY KEY,
                user_id           INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                provider          TEXT NOT NULL,
                provider_user_id  TEXT NOT NULL,
                provider_username TEXT,
                email             TEXT,
                email_verified    BOOLEAN NOT NULL DEFAULT false,
                avatar_url        TEXT,
                profile_url       TEXT,
                raw_profile       JSONB,
                access_token      TEXT,
                refresh_token     TEXT,
                token_expires_at  TIMESTAMPTZ,
                is_primary        BOOLEAN NOT NULL DEFAULT false,
                linked_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_login_at     TIMESTAMPTZ
            )
            "#,
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_user_identities_provider_uid \
             ON user_identities(provider, provider_user_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_user_identities_user ON user_identities(user_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_user_identities_provider_email \
             ON user_identities(provider, email)",
        )
        .await?;

        // ==================== 2. 回填现有 GitHub identity ====================
        // 2a. Backfill from `github_id IS NOT NULL`. `is_primary` is auth_provider='github'. Bound locals are 2b (`linked_github_id`).
        db.execute_unprepared(
            r#"
            INSERT INTO user_identities (
                user_id, provider, provider_user_id, provider_username,
                email, avatar_url, profile_url, raw_profile, is_primary, linked_at
            )
            SELECT
                id,
                'github',
                github_id::TEXT,
                username,
                email,
                avatar_url,
                github_profile_url,
                jsonb_strip_nulls(jsonb_build_object(
                    'bio', bio,
                    'location', location,
                    'company', company,
                    'display_name', display_name
                )),
                (auth_provider = 'github'),
                created_at
            FROM users
            WHERE github_id IS NOT NULL
            ON CONFLICT (provider, provider_user_id) DO NOTHING
            "#,
        )
        .await?;

        // 2b. 从 linked_github_id 列回填（本地 admin 绑定的 GitHub）
        //     注意：linked_github_id 与 github_id 可能指向同一个 GitHub 账户，
        //     ON CONFLICT 会去重。这里只补充那些没在 github_id 列中出现过的。
        db.execute_unprepared(
            r#"
            INSERT INTO user_identities (
                user_id, provider, provider_user_id, is_primary, linked_at
            )
            SELECT id, 'github', linked_github_id::TEXT, false, COALESCE(updated_at, NOW())
            FROM users
            WHERE linked_github_id IS NOT NULL
              AND auth_provider = 'local'
            ON CONFLICT (provider, provider_user_id) DO NOTHING
            "#,
        )
        .await?;

        // ==================== 3. 解除限制 ====================
        // 3a. 拆掉 "全库只能 1 个 local 用户" 的索引
        db.execute_unprepared("DROP INDEX IF EXISTS idx_local_admin")
            .await?;

        // 3b. 拆掉 "admin 必须 local" 的约束
        db.execute_unprepared("ALTER TABLE users DROP CONSTRAINT IF EXISTS check_admin_local_only")
            .await?;

        // 3c. 扩展 auth_provider 取值范围
        db.execute_unprepared("ALTER TABLE users DROP CONSTRAINT IF EXISTS check_auth_provider")
            .await?;
        db.execute_unprepared(
            "ALTER TABLE users ADD CONSTRAINT check_auth_provider \
             CHECK (auth_provider IN ('local','github','oidc','federated'))",
        )
        .await?;

        // ==================== 4. username 全局唯一（大小写不敏感）====================
        // 4a. 预处理冲突：把 LOWER(username) 重复的行，除第一行外加 _<id> 后缀
        //     第一行 = 最早创建的（ORDER BY id ASC）
        db.execute_unprepared(
            r#"
            WITH dups AS (
                SELECT id, username,
                       ROW_NUMBER() OVER (PARTITION BY LOWER(username) ORDER BY id) AS rn
                FROM users
            )
            UPDATE users u
            SET username = u.username || '_' || u.id::TEXT,
                updated_at = NOW()
            FROM dups d
            WHERE u.id = d.id AND d.rn > 1
            "#,
        )
        .await?;

        // 4b. 创建唯一索引
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_users_username_unique \
             ON users(LOWER(username))",
        )
        .await?;

        // ==================== 5. 配置项 allow_local_registration ====================
        db.execute_unprepared(
            r#"
            INSERT INTO configurations (key, value, description, category, is_encrypted, is_public)
            VALUES (
                'allow_local_registration',
                'false'::jsonb,
                '是否允许公开本地账号注册（关闭时仅 admin 可创建）',
                'auth',
                false,
                true
            )
            ON CONFLICT (key) DO NOTHING
            "#,
        )
        .await?;

        // ==================== 6. 用户生命周期（依赖 001–006 全部表） ====================
        db.execute_unprepared(include_str!("user_lifecycle.sql"))
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // 6. 用户生命周期
        db.execute_unprepared(
            "DROP TRIGGER IF EXISTS trg_users_delete_subject_rows ON users; \
             DROP FUNCTION IF EXISTS delete_user_subject_rows();",
        )
        .await?;

        // 5. 删除配置项
        db.execute_unprepared("DELETE FROM configurations WHERE key = 'allow_local_registration'")
            .await?;

        // 4. 拆掉 username 唯一索引
        db.execute_unprepared("DROP INDEX IF EXISTS idx_users_username_unique")
            .await?;

        // 3. 恢复约束（按 001_initial_schema 原状）
        db.execute_unprepared("ALTER TABLE users DROP CONSTRAINT IF EXISTS check_auth_provider")
            .await?;
        db.execute_unprepared(
            "ALTER TABLE users ADD CONSTRAINT check_auth_provider \
             CHECK (auth_provider IN ('local','github'))",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE users ADD CONSTRAINT check_admin_local_only \
             CHECK (NOT is_admin OR auth_provider = 'local')",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_local_admin \
             ON users(auth_provider) WHERE auth_provider = 'local'",
        )
        .await?;

        // DROP TABLE drops table-local indexes; CASCADE drops dependent objects.
        db.execute_unprepared("DROP TABLE IF EXISTS user_identities CASCADE")
            .await?;

        Ok(())
    }
}
