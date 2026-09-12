//! 群邻实例（room peer instance）—— 与本实例共处同一群聊的实例集合。
//!
//! 判定落在**实例（domain）**这一层，不是成员这一层：只要某个 domain 在某个
//! 本实例已加入的房间里有过一名活跃成员，该 domain 上的**全部**用户都算群邻。
//! 三个实例共处一个群聊时，三边首页看到的是同一批人。
//!
//! 集中在这里是因为同一份判定有三个调用方，各写一遍 SQL 必然漂移：
//!
//! 1. 出站扇出 —— 本地公开帖除粉丝外还要投给群邻实例（`publish_content` → `fan_out_to_room_peers`）；
//! 2. 入站放行 —— 共享收件箱是否留存「没有任何本地粉丝」的公开帖（`inbox::receive`）；
//! 3. 首页查询 —— Aro Home 按群邻 domain 过滤联邦活动（`api::tapp_runtime::federation`）。
//!
//! 与关注无关：关注关系走 `federation_follows`，那条线喂的是订阅时间线
//! （`federation_timeline`），不经过这里。

use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};

/// 从 actor URL 取 authority 段的 Postgres 表达式（`ra.domain` 缺失时的兜底）。
///
/// LEFT JOIN fallback when `ra.domain` is NULL: take `actor_url` authority.
/// Only `COALESCE(membership_status,'active')='active'` (pending invites are out).
const ACTOR_URL_DOMAIN: &str = "substring({col} from '^[a-zA-Z][a-zA-Z0-9+.-]*://([^/]+)')";

fn actor_url_domain(col: &str) -> String {
    ACTOR_URL_DOMAIN.replace("{col}", col)
}

/// 本实例已加入的房间 —— 至少有一名活跃本地成员。
///
/// 用 `is_local = true` 而不是「某个具体用户是成员」：需求按实例算，站长和
/// 普通用户加入的群聊都把对方实例拉进群邻集合。
const JOINED_ROOMS: &str = r#"SELECT room_id FROM federation_room_members
                              WHERE is_local = true
                                AND COALESCE(membership_status, 'active') = 'active'"#;

/// 群邻 domain 集合的子查询，产出单列 `domain`（小写、非空）。
///
/// `local_domain_param` 是调用方绑定本实例 domain 的占位符（如 `"$3"`）。本实例
/// 始终在集合里 —— 一个房间都没加入时首页也该看得到本站用户的帖子。
pub(crate) fn room_peer_domains_sql(local_domain_param: &str) -> String {
    format!(
        r#"SELECT d.domain FROM (
               SELECT lower({local_domain_param}::text) AS domain
               UNION
               SELECT DISTINCT lower(COALESCE(NULLIF(ra.domain, ''), {member_domain}))
               FROM federation_room_members m
               LEFT JOIN federation_remote_actors ra ON ra.actor_url = m.actor_url
               WHERE COALESCE(m.membership_status, 'active') = 'active'
                 AND m.room_id IN ({joined_rooms})
           ) d
           WHERE d.domain IS NOT NULL AND d.domain <> ''"#,
        local_domain_param = local_domain_param,
        member_domain = actor_url_domain("m.actor_url"),
        joined_rooms = JOINED_ROOMS,
    )
}

/// 某个 domain 是否是群邻。入站放行用。
///
/// 判定按 domain 而不是 actor：群邻实例上**任何**用户的公开帖都要留存，
/// 包括从没在群里发过言的那些。
pub async fn is_room_peer_domain(
    db: &impl ConnectionTrait,
    local_domain: &str,
    domain: &str,
) -> Result<bool, DbErr> {
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() {
        return Ok(false);
    }
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM ({peers}) p WHERE p.domain = $2) AS hit",
        peers = room_peer_domains_sql("$1"),
    );
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            [local_domain.to_ascii_lowercase().into(), domain.into()],
        ))
        .await?;
    Ok(row
        .and_then(|r| r.try_get::<bool>("", "hit").ok())
        .unwrap_or(false))
}

/// 一个群邻实例的投递目标。
pub struct RoomPeerInbox {
    pub inbox_url: String,
    pub domain: String,
}

/// 投递目标 SQL：有共享收件箱的实例产出一行，没有的按成员逐个产出。
///
/// 与 [`room_peer_domains_sql`] 一样单拆出来，便于不连库就检查语句。
pub(crate) fn room_peer_inboxes_sql() -> String {
    format!(
        r#"WITH members AS (
               SELECT DISTINCT
                      lower(COALESCE(NULLIF(ra.domain, ''), {member_domain})) AS domain,
                      NULLIF(ra.shared_inbox_url, '') AS shared_inbox_url,
                      NULLIF(ra.inbox_url, '') AS inbox_url
               FROM federation_room_members m
               JOIN federation_remote_actors ra ON ra.actor_url = m.actor_url
               WHERE m.is_local = false
                 AND COALESCE(m.membership_status, 'active') = 'active'
                 AND m.room_id IN ({joined_rooms})
           ),
           shared AS (
               SELECT DISTINCT domain, shared_inbox_url AS inbox_url
               FROM members
               WHERE shared_inbox_url IS NOT NULL AND domain IS NOT NULL
           )
           SELECT domain, inbox_url FROM shared
           UNION
           SELECT m.domain, m.inbox_url
           FROM members m
           WHERE m.inbox_url IS NOT NULL
             AND m.domain IS NOT NULL
             AND NOT EXISTS (SELECT 1 FROM shared s WHERE s.domain = m.domain)"#,
        member_domain = actor_url_domain("m.actor_url"),
        joined_rooms = JOINED_ROOMS,
    )
}

/// 群邻实例的投递入口 —— 有共享收件箱的按实例投一次，没有的按成员逐个投。
///
/// 优先共享收件箱是关键：群邻的语义是「整个实例」，逐个成员投递既漏掉没进群的
/// 用户，又在大群里把同一条活动重复塞进同一个实例。
///
/// 被 block 的实例不在这里过滤 —— 投递线程出站前会走
/// [`crate::federation::trust::enforce_outbound`]，两处都判会让封禁策略有两个真相。
pub async fn room_peer_inboxes(db: &impl ConnectionTrait) -> Result<Vec<RoomPeerInbox>, DbErr> {
    let sql = room_peer_inboxes_sql();
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            [],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let inbox_url: String = r.try_get("", "inbox_url").ok()?;
            let domain: String = r.try_get("", "domain").unwrap_or_default();
            (!inbox_url.trim().is_empty()).then_some(RoomPeerInbox { inbox_url, domain })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_domain_subquery_binds_local_domain_and_scopes_to_joined_rooms() {
        let sql = room_peer_domains_sql("$1");
        // 本实例 domain 必须直接进集合，否则一个群都没加入时首页会连本站帖子都看不到。
        assert!(sql.contains("lower($1::text)"));
        // 群邻范围必须由「有本地活跃成员的房间」界定，不是全部已知房间。
        assert!(sql.contains("is_local = true"));
        assert!(sql.contains("COALESCE(membership_status, 'active') = 'active'"));
    }

    #[test]
    fn peer_domain_subquery_falls_back_to_actor_url_authority() {
        let sql = room_peer_domains_sql("$2");
        // remote_actors 还没抓到 actor 文档时 ra.domain 是 NULL，成员仍要算进群邻。
        assert!(sql.contains("NULLIF(ra.domain, '')"));
        assert!(sql.contains("substring(m.actor_url from"));
    }
}
