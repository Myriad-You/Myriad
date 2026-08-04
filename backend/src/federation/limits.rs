//! 联邦体积上限的单一事实源
//!
//! # 为什么要集中
//!
//! 这些数字此前散在 5 个文件里（`channel.rs`、`room.rs`、`content.rs`、
//! `file_transfer.rs`、`main.rs` 的路由层），彼此靠注释「与 X 对齐」维持一致。
//! 改一个忘一个的结果不是编译错误，而是**运行期的静默不匹配**：本地允许发出
//! 的消息，对端 inbox 拒收。
//!
//! # 这条链
//!
//! ```text
//! 用户提交 payload  ──►  活动 = payload + 信封
//! │
//! ├─► 本地路由 DefaultBodyLimit   （我们控制）
//! └─► 对端 inbox DefaultBodyLimit （对方控制）
//! ```
//!
//! 实际可用体积 = 链上的**最小值**。所以 [`INBOX_BODY_LIMIT`] 必须显著大于
//! [`MESSAGE_PAYLOAD_LIMIT`]，否则本实例发得出、收不进自己的消息。
//!
//! 注意跨实例时对端可能是旧版本或别的实现，它的 inbox 上限我们无从得知 ——
//! 放宽本端只在双方都升级后才完全生效。
//!
//! # 内存上界
//!
//! inbox 是**远端可达**的表面。一个请求最坏会同时持有：原始字节
//! （≤ [`INBOX_BODY_LIMIT`]）+ 解析后的 `serde_json::Value`（结构化开销通常是
//! 原始大小的 2–3 倍）。也就是说单个满额请求的峰值约 `3 × INBOX_BODY_LIMIT`。
//!
//! docker-compose 给 backend 的限制是 **2 GiB**。当前取值下，若干并发满额投递
//! 仍在预算内；继续调高必须同步抬高容器内存限制，否则表现为 OOM 被杀而不是
//! 干净的 413。
//!
//! 缓解措施：`inbox::verify_preparse_gate` 在解析 JSON **之前**先校验签名头、
//! Date 新鲜度与 Digest（对原始字节逐字节比对），攻击者要让我们开始解析就得先
//! 算出正确的 SHA-256，不能靠一坨随机字节撑爆内存。

/// 单条消息载荷上限（channel 与 room 共用）。
///
/// 按 `payload.to_string().len()` 度量，覆盖小文件内联 base64 与 Tapp 安装包
/// 分享；更大的附件走 [`file_transfer`](crate::federation::file_transfer) 分块。
pub const MESSAGE_PAYLOAD_LIMIT: usize = 64 * 1024 * 1024;

/// inbox 请求体上限（`/inbox` 与 `/users/{u}/inbox`）。
///
/// 必须容纳 [`MESSAGE_PAYLOAD_LIMIT`] 加上活动信封、JSON 转义膨胀与
/// base64 分块的 4/3 放大。留 1.5 倍余量。
pub const INBOX_BODY_LIMIT: usize = MESSAGE_PAYLOAD_LIMIT * 3 / 2;

/// 已认证用户提交内容的路由上限（发布、房间/频道消息、媒体上传）。
///
/// 比 inbox 略宽：这些请求来自已登录的本地用户，不是任意远端。
pub const AUTHENTICATED_BODY_LIMIT: usize = INBOX_BODY_LIMIT + 16 * 1024 * 1024;

/// 小型联邦控制端点的请求体上限（信任策略、房间创建、邀请等）。
///
/// 这些端点的 body 只有几百字节到几 KiB。放宽到 1 MiB 足够容纳异常大的
/// 名称/描述，同时避免一个只需要 JSON 小对象的端点可以缓冲整个路由上限。
pub const SMALL_CONTROL_BODY_LIMIT: usize = 1024 * 1024;

/// 文件分块传输的单块大小。
pub const TRANSFER_CHUNK_SIZE: i64 = 4 * 1024 * 1024;

/// 分块上传端点的请求体上限。
///
/// 块内容以 base64 承载（4/3 膨胀），再加 JSON 键，故显著大于
/// [`TRANSFER_CHUNK_SIZE`]。
pub const TRANSFER_CHUNK_BODY_LIMIT: usize = 16 * 1024 * 1024;

/// 单个文件传输的总大小上限。
///
/// Product decision: large media is in-scope for self-hosted chat. Concurrent
/// amplification is bounded separately (MYR-008) rather than shrinking this.
pub const MAX_FILE_SIZE: i64 = 20 * 1024 * 1024 * 1024;

// ── MYR-008: transfer amplification admission (generous budgets) ───────────
//
// Prefer budgets + admission control over removing chunked transfer. Numbers
// are intentionally loose for normal multi-file use; they only stop absurd
// concurrent pile-ups (dozens of max-size files / unbounded in-flight chunks).

/// Max concurrent open transfers (`pending` + `in-progress`) process-wide.
pub const MAX_CONCURRENT_TRANSFERS: i64 = 64;

/// Max concurrent open transfers attributed to one local user (initiator/owner).
pub const MAX_CONCURRENT_TRANSFERS_PER_USER: i64 = 16;

/// Sum of declared `file_size` across open transfers must stay under this.
///
/// ≈ 3 × [`MAX_FILE_SIZE`]: a few full-size uploads at once, or many smaller.
pub const MAX_CONCURRENT_TRANSFER_BYTES: i64 = 64 * 1024 * 1024 * 1024; // 64 GiB

/// Max decoded chunk payload bytes held concurrently across upload/inbound handlers.
///
/// At [`TRANSFER_CHUNK_SIZE`] (4 MiB), this allows ~32 simultaneous chunk ops.
pub const MAX_IN_FLIGHT_CHUNK_BYTES: usize = 128 * 1024 * 1024; // 128 MiB

/// Note 内联图片附件的字节上限。
pub const NOTE_IMAGE_LIMIT: usize = 32 * 1024 * 1024;

/// Note 内联视频附件的字节上限。
pub const NOTE_VIDEO_LIMIT: usize = 256 * 1024 * 1024;

/// 单条 Note 的附件数量上限。
pub const NOTE_ATTACHMENT_COUNT_LIMIT: usize = 32;

/// 单条 Note 的正文字符数上限。
pub const NOTE_TEXT_CHAR_LIMIT: usize = 100_000;

// 编译期不变量
//
// 这些关系是常量之间的，没有理由等到跑测试才发现 —— 违反它们直接编译失败。
// 想调大某个上限时，这里会先把连带约束顶出来。

/// 本实例发得出的东西，本实例必须收得进。
///
/// 违反它的表现不是错误信息，而是消息在对端被 413 静默拒收。
const _: () = assert!(
    INBOX_BODY_LIMIT > MESSAGE_PAYLOAD_LIMIT,
    "INBOX_BODY_LIMIT must exceed MESSAGE_PAYLOAD_LIMIT to fit the activity envelope"
);

/// 信封与 JSON 转义膨胀至少要有 25% 余量。
const _: () = assert!(
    INBOX_BODY_LIMIT - MESSAGE_PAYLOAD_LIMIT >= MESSAGE_PAYLOAD_LIMIT / 4,
    "not enough envelope headroom between MESSAGE_PAYLOAD_LIMIT and INBOX_BODY_LIMIT"
);

/// 本地用户提交的内容最终要能作为活动投递出去。
const _: () = assert!(
    AUTHENTICATED_BODY_LIMIT >= INBOX_BODY_LIMIT,
    "AUTHENTICATED_BODY_LIMIT must not be narrower than INBOX_BODY_LIMIT"
);

/// 分块请求体要容得下 base64 的 4/3 膨胀。
const _: () = assert!(
    TRANSFER_CHUNK_BODY_LIMIT > (TRANSFER_CHUNK_SIZE as usize) * 4 / 3,
    "TRANSFER_CHUNK_BODY_LIMIT cannot hold a base64-encoded TRANSFER_CHUNK_SIZE"
);

/// 控制类端点不该能缓冲和消息端点一样多的数据。
const _: () = assert!(
    SMALL_CONTROL_BODY_LIMIT < MESSAGE_PAYLOAD_LIMIT / 8,
    "SMALL_CONTROL_BODY_LIMIT is too close to the bulk message limit"
);

/// Concurrent transfer budget must be at least one full max-size file.
const _: () = assert!(
    MAX_CONCURRENT_TRANSFER_BYTES >= MAX_FILE_SIZE,
    "MAX_CONCURRENT_TRANSFER_BYTES must admit at least one MAX_FILE_SIZE transfer"
);

/// Per-user concurrent transfer cap must not exceed the global cap.
const _: () = assert!(
    MAX_CONCURRENT_TRANSFERS_PER_USER <= MAX_CONCURRENT_TRANSFERS,
    "per-user transfer cap cannot exceed the global concurrent transfer cap"
);

/// In-flight chunk budget must hold at least one full chunk.
const _: () = assert!(
    MAX_IN_FLIGHT_CHUNK_BYTES >= TRANSFER_CHUNK_SIZE as usize,
    "MAX_IN_FLIGHT_CHUNK_BYTES must hold at least one TRANSFER_CHUNK_SIZE"
);

/// 容器内存预算哨兵。
///
/// 单个满额 inbox 请求峰值约 `3 × INBOX_BODY_LIMIT`（原始字节 + 解析后的 Value）。
/// compose 给 backend 2 GiB；要求单请求峰值不超过其 1/4，好让若干并发投递
/// 不至于把容器打爆。
///
/// **这条先失败就是提醒：调高上限前先抬高 docker-compose.yml 里的 `memory:`。**
const _: () = assert!(
    INBOX_BODY_LIMIT * 3 <= (2 * 1024 * 1024 * 1024_usize) / 4,
    "a max-size inbox request would peak past a quarter of the 2 GiB container limit; \
     raise `memory:` in docker-compose.yml before raising INBOX_BODY_LIMIT"
);

#[cfg(test)]
mod tests {

    /// 实证 axum 的 layer 覆盖语义。
    ///
    /// 联邦路由的 body 上限现在**大于**外层的全局上限，这依赖「离 handler 更近的
    /// `DefaultBodyLimit` 覆盖更外层的」。这条语义如果不成立，联邦上传会被外层
    /// 静默夹到 50 MB —— 表现为大文件传输莫名 413，且没有任何编译期信号。
    ///
    /// 用真实的 Router 走一遍请求来证明，而不是靠读文档假设。
    #[tokio::test]
    async fn inner_body_limit_overrides_the_outer_one() {
        use axum::http::{Request, StatusCode};
        use axum::{body::Body, extract::DefaultBodyLimit, routing::post, Router};
        use tower::ServiceExt;

        const OUTER: usize = 1024;
        const INNER: usize = 8 * 1024;

        async fn echo(body: axum::body::Bytes) -> String {
            body.len().to_string()
        }

        // 内层（联邦子路由）比外层（全局）更宽，与 main.rs 的结构一致
        let inner = Router::new()
            .route("/wide", post(echo))
            .layer(DefaultBodyLimit::max(INNER));
        let app = Router::new()
            .route("/narrow", post(echo))
            .merge(inner)
            .layer(DefaultBodyLimit::max(OUTER));

        let payload = vec![b'x'; 4 * 1024]; // 大于 OUTER，小于 INNER

        let wide = app
            .clone()
            .oneshot(
                Request::post("/wide")
                    .body(Body::from(payload.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            wide.status(),
            StatusCode::OK,
            "inner DefaultBodyLimit must widen past the outer one; if this fails, \
             federation uploads are silently clamped to the global limit"
        );

        // 同一个 body 打到只受外层约束的路由上，必须被拒 —— 证明外层确实生效，
        // 上面的成功不是因为两个 limit 都没起作用
        let narrow = app
            .oneshot(Request::post("/narrow").body(Body::from(payload)).unwrap())
            .await
            .unwrap();
        assert_eq!(narrow.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}

#[cfg(test)]
mod optional_json_tests {
    /// `Option<Json<T>>` 对「空 body」与「畸形 JSON」的实际行为。
    ///
    /// `federation_join_room` 的 body 是可选的，原实现是
    /// `from_slice(..).unwrap_or_default()` —— 空 body 和畸形 JSON 都回落到默认值。
    /// 换成 `Option<Json<T>>` 之后这两种情况是否仍然一致，必须实测，
    /// 不能靠读文档猜。
    #[tokio::test]
    async fn optional_json_distinguishes_absent_from_malformed() {
        use axum::http::{Request, StatusCode};
        use axum::{body::Body, routing::post, Json, Router};
        use tower::ServiceExt;

        #[derive(Default, serde::Deserialize)]
        struct Payload {
            home_server: Option<String>,
        }

        async fn handler(body: Option<Json<Payload>>) -> String {
            let p = body.map(|Json(v)| v).unwrap_or_default();
            p.home_server.unwrap_or_else(|| "DEFAULT".into())
        }

        let app = Router::new().route("/j", post(handler));

        // 1) 完全没有 body → 走默认值（join_room 的正常用法）
        let empty = app
            .clone()
            .oneshot(Request::post("/j").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            empty.status(),
            StatusCode::OK,
            "empty body must be accepted"
        );

        // 2) 合法 body → 正常解析
        let ok = app
            .clone()
            .oneshot(
                Request::post("/j")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"home_server":"h"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);

        // 3) 畸形 JSON → 这里与原实现**有差异**：原来 unwrap_or_default() 静默
        // 回落，现在 axum 会拒绝。对一个"加入房间"的请求，显式报错比静默
        // 忽略用户传错的 home_server 更好，所以接受这个变化并在此记录。
        let malformed = app
            .oneshot(
                Request::post("/j")
                    .header("content-type", "application/json")
                    .body(Body::from("{not json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            malformed.status(),
            StatusCode::BAD_REQUEST,
            "malformed JSON is now rejected instead of silently defaulting"
        );
    }
}

#[cfg(test)]
mod rejection_tests {
    /// 超限 body 必须仍然拿到带分块传输指引的 413。
    ///
    /// `send_room_message` 原本手工 `Bytes::from_request` 才能返回这句提示；
    /// 换成 `Json<T>` 提取器后，如果不接管 `JsonRejection`，用户会收到 axum 的
    /// 纯文本 413，看不到"更大的文件走分块传输"这个下一步动作。
    #[tokio::test]
    async fn oversized_body_keeps_the_chunked_transfer_hint() {
        use axum::extract::rejection::JsonRejection;
        use axum::http::{Request, StatusCode};
        use axum::{body::Body, extract::DefaultBodyLimit, routing::post, Json, Router};
        use tower::ServiceExt;

        #[derive(serde::Deserialize)]
        struct P {
            _x: Option<String>,
        }

        async fn handler(payload: Result<Json<P>, JsonRejection>) -> axum::response::Response {
            match payload {
                Ok(_) => "ok".into_response(),
                Err(e) => crate::api::federation::json_rejection_response(e, Some("use chunked transfer")),
            }
        }
        use axum::response::IntoResponse;

        let app = Router::new()
            .route("/m", post(handler))
            .layer(DefaultBodyLimit::max(64));

        let resp = app
            .oneshot(
                Request::post("/m")
                    .header("content-type", "application/json")
                    .body(Body::from(vec![b'x'; 4096]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("use chunked transfer"),
            "the operator-facing hint must survive the extractor switch; got: {text}"
        );
    }
}
