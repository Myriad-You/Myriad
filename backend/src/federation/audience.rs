//! Audience 与对象归属规则
//!
//! 这里集中三件判断：
//!
//! 1. **本地发布的 visibility** —— 只有明确建模过的 visibility 才允许发布。
//! 2. **入站 sharedInbox 的定向** —— 只有寻址到 Public 或该 Actor 自己的
//! followers collection 的活动才可以进入粉丝的首页时间线。
//! 3. **对象归属** —— 签名只证明"某个 key 签了这个请求"，还需要证明
//! 签名 Actor 有权创建/修改/删除该对象。
//!
//! 放在独立模块而不是 `types.rs`/`inbox.rs`，是为了让这些不变量能被单测
//! 直接覆盖，不必启动数据库。

use serde_json::Value;

use crate::federation::types::{normalize_actor_url, same_actor_url, AP_PUBLIC};

/// AP Public 的几种等价写法。
///
/// `as:Public` 与裸 `Public` 是 JSON-LD 压缩形式，历史实现广泛存在，
/// 拒绝它们会误伤真实流量。
const PUBLIC_ALIASES: &[&str] = &[
    AP_PUBLIC,
    "as:Public",
    "Public",
    "https://www.w3.org/ns/activitystreams#public",
];

/// 本地发布支持的 visibility。
///
/// 每一项都必须在 [`resolve_audience`] 与 [`fan_out_scope`] 里有明确建模；
/// 新增取值必须同时补这两处，否则 `parse_visibility` 会拒绝它。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// 寻址 Public，cc 粉丝集合 —— 进入公开 Outbox 与联邦时间线。
    Public,
    /// 只寻址粉丝集合 —— 投递给粉丝，但不进入公开 Outbox。
    Followers,
    /// 作者自寻址 —— 不做粉丝 fan-out（`PublishRequest` 无收件人列表）。
    Direct,
}

impl Visibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Followers => "followers",
            Visibility::Direct => "direct",
        }
    }
}

/// 解析客户端传入的 visibility；未知值返回 `Err(原值)` 交由调用方拒绝。
pub fn parse_visibility(raw: &str) -> Result<Visibility, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "public" => Ok(Visibility::Public),
        "followers" | "unlisted" | "private" => Ok(Visibility::Followers),
        "direct" | "mentioned" => Ok(Visibility::Direct),
        other => Err(other.to_string()),
    }
}

/// 粉丝 fan-out 范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanOutScope {
    /// 投递给全部已接受的粉丝。
    AllFollowers,
    /// 不做粉丝 fan-out（发布路径排队 0 条投递）。
    ExplicitRecipientsOnly,
}

/// visibility → fan-out 范围。
///
/// `Direct` 必须落在 `ExplicitRecipientsOnly`。
pub fn fan_out_scope(visibility: Visibility) -> FanOutScope {
    match visibility {
        Visibility::Public | Visibility::Followers => FanOutScope::AllFollowers,
        Visibility::Direct => FanOutScope::ExplicitRecipientsOnly,
    }
}

/// 判断某个 IRI 是否是 AP Public。
pub fn is_public_address(value: &str) -> bool {
    let trimmed = value.trim();
    PUBLIC_ALIASES
        .iter()
        .any(|alias| trimmed.eq_ignore_ascii_case(alias))
}

/// 收集活动的收件人 IRI（`to`/`cc`/`bto`/`bcc`/`audience`）。
///
/// 每个字段都可能是字符串、对象（取 `id`）或二者混合的数组 —— AS2 允许全部
/// 这些形态，只处理 `to`/`cc` 的字符串形式会漏掉真实流量。
pub fn collect_recipients(activity: &Value) -> Vec<String> {
    const FIELDS: &[&str] = &["to", "cc", "bto", "bcc", "audience"];
    let mut out = Vec::new();

    // 活动本身与内嵌 object 上的寻址字段都要看：很多实现只在 object 上寻址。
    let sources = [activity, &activity["object"]];
    for source in sources {
        for field in FIELDS {
            let Some(val) = source.get(field) else {
                continue;
            };
            push_iris(val, &mut out);
        }
    }

    out.sort();
    out.dedup();
    out
}

fn push_iris(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            if !s.trim().is_empty() {
                out.push(s.trim().to_string());
            }
        }
        Value::Array(arr) => {
            for item in arr {
                push_iris(item, out);
            }
        }
        Value::Object(obj) => {
            if let Some(Value::String(id)) = obj.get("id") {
                if !id.trim().is_empty() {
                    out.push(id.trim().to_string());
                }
            }
        }
        _ => {}
    }
}

/// 入站活动是否可以分发到「关注该 Actor 的本地用户」的首页时间线。
///
/// 规则：必须寻址到 Public，或寻址到该 Actor 自己名下的某个 collection
/// （`{actor_url}/followers` 等）。只寻址给具体个人的活动不进粉丝时间线 ——
/// 这是 C3 的修复点。
///
/// 不依赖数据库里缓存的 followers URL（当前 schema 没有这一列）。
/// 「与 Actor 同源且在 Actor 路径之下」的前缀判断；Mastodon/Pleroma/Misskey
/// 的 followers collection 都是 `{actor}/followers`。
pub fn may_distribute_to_followers(activity: &Value, actor_url: &str) -> bool {
    let recipients = collect_recipients(activity);
    if recipients.is_empty() {
        // 完全没有收件人的活动无法判断意图 —— 不扩散。
        return false;
    }

    let actor_norm = normalize_actor_url(actor_url);
    recipients.iter().any(|r| {
        if is_public_address(r) {
            return true;
        }
        // Actor 名下的 collection（followers 等）。
        let r_norm = normalize_actor_url(r);
        r_norm.len() > actor_norm.len() && r_norm.starts_with(&format!("{}/", actor_norm))
    })
}

/// 从 `attributedTo` 取第一个 IRI（字符串 / 对象 / 数组皆可）。
pub fn attributed_to_id(object: &Value) -> Option<String> {
    let mut ids = Vec::new();
    if let Some(v) = object.get("attributedTo") {
        push_iris(v, &mut ids);
    }
    ids.into_iter().next()
}

/// 取对象 id（对象形态取 `id`，字符串形态即其本身）。
pub fn object_id(object: &Value) -> Option<String> {
    match object {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Object(_) => object
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// 两个 IRI 是否同源（scheme + host + port 相同）。
///
/// AP 的标准"同源规则"：Activity 只能操作与自己同域的对象。
pub fn same_origin(left: &str, right: &str) -> bool {
    fn origin(raw: &str) -> Option<String> {
        let parsed = url::Url::parse(raw.trim()).ok()?;
        let host = parsed.host_str()?.to_ascii_lowercase();
        Some(match parsed.port() {
            Some(p) => format!("{}://{}:{}", parsed.scheme(), host, p),
            None => format!("{}://{}", parsed.scheme(), host),
        })
    }
    match (origin(left), origin(right)) {
        (Some(l), Some(r)) => l == r,
        _ => false,
    }
}

/// 对象归属校验失败的原因。
#[derive(Debug, PartialEq, Eq)]
pub enum OwnershipError {
    /// `attributedTo` 指向了签名 Actor 以外的主体。
    AttributedToMismatch { expected: String, found: String },
    /// 对象 id 与签名 Actor 不同源。
    CrossOriginObject { actor: String, object: String },
}

impl std::fmt::Display for OwnershipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OwnershipError::AttributedToMismatch { expected, found } => write!(
                f,
                "object attributedTo {} does not match signing actor {}",
                found, expected
            ),
            OwnershipError::CrossOriginObject { actor, object } => write!(
                f,
                "object {} is not same-origin with signing actor {}",
                object, actor
            ),
        }
    }
}

/// 校验签名 Actor 有权创建/修改该对象。
///
/// - `attributedTo` 存在时必须等于签名 Actor。
/// - 对象 id 存在时必须与签名 Actor 同源。
///
/// 两者缺失都放行：AS2 不强制要求，缺失时活动的 `actor` 仍然是唯一归属来源。
/// 这是 H4 的修复点。仅用于 `Create`/`Update` —— `Announce`/`Like` 的对象本来
/// 就属于别人，不能套用。
pub fn verify_object_ownership(actor_url: &str, object: &Value) -> Result<(), OwnershipError> {
    if let Some(attributed) = attributed_to_id(object) {
        if !same_actor_url(&attributed, actor_url) {
            return Err(OwnershipError::AttributedToMismatch {
                expected: actor_url.to_string(),
                found: attributed,
            });
        }
    }

    if let Some(oid) = object_id(object) {
        if !same_origin(&oid, actor_url) {
            return Err(OwnershipError::CrossOriginObject {
                actor: actor_url.to_string(),
                object: oid,
            });
        }
    }

    Ok(())
}

/// 校验 `Delete` 的目标与签名 Actor 同源。
///
/// 删除类活动的对象通常已经被压缩成裸 IRI 或 Tombstone，没有 `attributedTo`
/// 可依赖，所以只做同源判断；真正的所有权由调用方在 SQL 里用
/// `remote_actor_id` 再收一次。
pub fn verify_object_same_origin(actor_url: &str, object: &Value) -> Result<(), OwnershipError> {
    match object_id(object) {
        Some(oid) if !same_origin(&oid, actor_url) => Err(OwnershipError::CrossOriginObject {
            actor: actor_url.to_string(),
            object: oid,
        }),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_visibility_maps_known_values_and_rejects_unknown() {
        assert_eq!(parse_visibility("public"), Ok(Visibility::Public));
        assert_eq!(parse_visibility("PUBLIC"), Ok(Visibility::Public));
        assert_eq!(parse_visibility("followers"), Ok(Visibility::Followers));
        assert_eq!(parse_visibility("unlisted"), Ok(Visibility::Followers));
        assert_eq!(parse_visibility("direct"), Ok(Visibility::Direct));
        assert_eq!(parse_visibility("mentioned"), Ok(Visibility::Direct));
        // 拼错的 visibility 必须报错，而不是退化成"空收件人 + 照常 fan-out"
        assert!(parse_visibility("pubIic").is_err());
        assert!(parse_visibility("").is_err());
    }

    #[test]
    fn direct_never_fans_out_to_followers() {
        assert_eq!(
            fan_out_scope(Visibility::Direct),
            FanOutScope::ExplicitRecipientsOnly
        );
        assert_eq!(fan_out_scope(Visibility::Public), FanOutScope::AllFollowers);
        assert_eq!(
            fan_out_scope(Visibility::Followers),
            FanOutScope::AllFollowers
        );
    }

    #[test]
    fn public_aliases_recognised() {
        assert!(is_public_address(AP_PUBLIC));
        assert!(is_public_address("as:Public"));
        assert!(is_public_address("Public"));
        assert!(!is_public_address("https://evil.example/Public"));
    }

    #[test]
    fn collect_recipients_handles_string_array_and_object_forms() {
        let activity = json!({
            "to": "https://a.example/users/x",
            "cc": ["https://b.example/users/y", {"id": "https://c.example/users/z"}],
            "object": { "audience": "https://d.example/users/w" }
        });
        let got = collect_recipients(&activity);
        assert!(got.contains(&"https://a.example/users/x".to_string()));
        assert!(got.contains(&"https://b.example/users/y".to_string()));
        assert!(got.contains(&"https://c.example/users/z".to_string()));
        assert!(got.contains(&"https://d.example/users/w".to_string()));
    }

    #[test]
    fn distribute_allows_public_and_actor_followers_only() {
        let actor = "https://remote.example/users/alice";

        let public = json!({"to": [AP_PUBLIC], "actor": actor});
        assert!(may_distribute_to_followers(&public, actor));

        let followers = json!({"to": ["https://remote.example/users/alice/followers"]});
        assert!(may_distribute_to_followers(&followers, actor));

        // 定向给具体个人 —— 不进粉丝时间线（C3）
        let direct = json!({"to": ["https://myriad.example/users/bob"]});
        assert!(!may_distribute_to_followers(&direct, actor));

        // 完全未寻址 —— 不扩散
        let unaddressed = json!({"type": "Create"});
        assert!(!may_distribute_to_followers(&unaddressed, actor));

        // 别的 Actor 的 followers collection 不算
        let other = json!({"to": ["https://remote.example/users/mallory/followers"]});
        assert!(!may_distribute_to_followers(&other, actor));
    }

    #[test]
    fn ownership_rejects_spoofed_attributed_to() {
        let actor = "https://remote.example/users/alice";
        let spoofed = json!({
            "id": "https://remote.example/notes/1",
            "attributedTo": "https://remote.example/users/victim"
        });
        assert!(matches!(
            verify_object_ownership(actor, &spoofed),
            Err(OwnershipError::AttributedToMismatch { .. })
        ));

        let ok = json!({
            "id": "https://remote.example/notes/1",
            "attributedTo": actor
        });
        assert!(verify_object_ownership(actor, &ok).is_ok());
    }

    #[test]
    fn ownership_rejects_cross_origin_object() {
        let actor = "https://remote.example/users/alice";
        let foreign = json!({"id": "https://victim.example/notes/1"});
        assert!(matches!(
            verify_object_ownership(actor, &foreign),
            Err(OwnershipError::CrossOriginObject { .. })
        ));
        // Delete 常见的裸 IRI 形态同样要挡住
        assert!(matches!(
            verify_object_same_origin(actor, &json!("https://victim.example/notes/1")),
            Err(OwnershipError::CrossOriginObject { .. })
        ));
        assert!(verify_object_same_origin(actor, &json!("https://remote.example/notes/1")).is_ok());
    }

    #[test]
    fn ownership_allows_objects_without_id_or_attribution() {
        let actor = "https://remote.example/users/alice";
        assert!(verify_object_ownership(actor, &json!({"type": "Note", "content": "hi"})).is_ok());
    }

    #[test]
    fn same_origin_compares_scheme_host_port() {
        assert!(same_origin(
            "https://a.example/x",
            "https://A.EXAMPLE/users/alice"
        ));
        assert!(!same_origin("http://a.example/x", "https://a.example/y"));
        assert!(!same_origin(
            "https://a.example:8443/x",
            "https://a.example/y"
        ));
        assert!(!same_origin("not-a-url", "https://a.example/y"));
    }
}
