# Myriad 审查修复任务清单

基线：`Myriad_Audit_2026_Evidence` 与对应审查会话；实施分支：`repair-runtime-trust-and-reliability`。

原则：先恢复启动、事务、身份与资源所有权不变量，再处理协议投影和工程治理。CSP、请求体、队列和并发限额必须有明确威胁模型与容量依据，不以任意低阈值换取表面安全。

## 结论与任务

- [x] **T00 — 三棵 Rust 构建根缺少一致治理：真实存在。**
  - 主工作区、proxy、updater 固定 Rust 1.94；CI 对各自锁文件执行 check/test/Clippy，并补齐 proxy/updater 独立门禁与依赖审计。
  - Rust 1.94 新增 lint 已做等价兼容修复。

- [x] **T01 — migration/schema repair 失败后仍可能开放服务：真实存在。**
  - 迁移、修复、锁等待和最终 drift 复核全部 fail-closed；只有 schema 被证明就绪后 health/业务路由才可 ready。
  - 保留已发布迁移历史，以 no-op retired migration 表达废弃版本，不再删除或重写迁移记录。
  - 负向测试覆盖缺表、缺 owner 列、失败 repair、绕过开关和未就绪健康状态。

- [ ] **T02 — 重建首次设置链路：按用户要求排除。**
  - 本分支不改变 bootstrap capability、首次 owner 创建入口或设置 UI。

- [x] **T03 — Release manifest 缺失时必须禁止 Docker Hub tag 回退：当前产品阶段不成立。**
  - [Issue #265](https://github.com/Myriad-You/Myriad/issues/265) 明确要求 preview/内测阶段保留 GitHub Release 受限时的 Docker Hub tag 回退。
  - 已撤销本轮对此回退路径的修改；签名失败、manifest 非法或已取得 manifest 后的校验失败仍保持 fail-closed。公开 GA 前再单独评估收紧。

- [x] **T04 — proxy 对 hop-by-hop/forwarded headers 的信任边界不完整：真实存在。**
  - HTTP 与 WebSocket 使用同一 HeaderPolicy；移除固定及 `Connection` 动态提名头。
  - forwarded IP/proto/host 只接受显式 allowlist 的直接上游；空 allowlist 不再隐式信任本机/私网。
  - 负向测试覆盖伪造 forwarding 头、大小写混合/重复 Connection token、WS 和响应头清理。

- [x] **T05 — CSRF 状态绑定单进程：真实存在。**
  - 改为有版本、限长、带 TTL、绑定 JWT 签名摘要与 token version 的 HMAC 无状态令牌。
  - 负向测试覆盖篡改、跨会话、跨 token epoch、过期、畸形与未知版本；正向覆盖跨进程重启验证。

- [x] **T06 — 每次认证请求重复查库，角色路径再次查库：真实存在。**
  - 使用 5 秒、4096 项有界正/负缓存，合并 token-version/role 查询，并以单用户 single-flight 抑制击穿。
  - 密码、角色、删除和 token-version 变更同时本地失效并发布 PostgreSQL NOTIFY；管理员高危校验保留实时查库。
  - 负向测试覆盖负缓存、过期、失效和并发加载边界。

- [x] **T07 — Federation Inbox 缺少跨进程幂等收据与原子提交边界：真实存在。**
  - 新增持久化 receipt claim/finish 状态机；业务 DB 写入、Accept/outbox 与 receipt 结果同一事务提交。
  - 摘要冲突、重复投递、失败重试均有明确语义；仍依赖远程 HTTP/文件系统副作用的 Move/FileChunk 暂时返回 503，避免伪装成功。
  - 恢复条件：Move 将远程文档校验拆为有界 preflight、事务内只保留 DB 效果；FileChunk 使用内容寻址暂存 + 事务化 finalize outbox（或 DB 内同事务存储），并覆盖崩溃恢复。
  - 负向测试覆盖 identity/digest 冲突、多本地 inbox 作用域、重试回滚和永久拒绝。

- [x] **T08 — ActivityPub collection/object 链接广告了不可解引用资源：真实存在。**
  - followers/following 不再发布虚假的 `first`；outbox、activity 与对象解引用共享公开投影。
  - Notes、Reports、Brew articles、Tapps、Library 对象仅对 public/published 且 canonical/type 匹配的资源可达。
  - 负向测试覆盖私有、未发布、跨类型、非 canonical 与不存在对象。

- [x] **T09 — AI task event 通道无界且持久化逐事件放大：真实存在。**
  - 改为有界 mailbox、进程级 semaphore、delta/progress 合并、64 KiB 文本上限、批量持久化与有界 shutdown drain。
  - terminal 状态继续走独立控制路径，不能被数据队列背压丢弃。
  - 负向测试覆盖 queue full、慢消费者、delta 风暴、取消、关闭顺序、交替压力与长结果。

## 验收顺序

1. 聚焦负向测试。
2. 主 Rust workspace `check`、全量 `test`、Clippy。
3. proxy 独立 fmt/check/test/Clippy。
4. frontend 单测、类型检查、构建；现存 lint/style 基线单独列明。
5. updater 在 Ubuntu CI 验证（其 Unix API 与 `ring` 交叉编译器使 Windows 本机不能提供有效证据）。
6. 合并最新 `preview` 后重复受影响门禁，并以 PR CI 作为 PostgreSQL schema drift 与 Linux updater 的最终证据。
