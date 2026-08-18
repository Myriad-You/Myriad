# Architecture Decision Records

ADR 记录的是**已生效的结论**，不是提议。编号是写作顺序，不是逻辑顺序——按主题线读，不要按编号读。不要重新论证已记录的决策，除非手上有能推翻它的新事实；那种情况下写一条新 ADR 标注取代关系（见 `Supersedes:` / `Superseded by`），不改旧的。

## 主题线

### 平台配置与升级语义

- [0001 升级时 seed 平台配置，消除「库里未设置」态](0001-seed-permission-config-on-upgrade.md)
- [0004 平台配置的读写一致性由测试保证](0004-config-parse-consistency-test.md)
- [0015 暂存池阈值作为平台配置项，附默认值](0015-stash-thresholds-as-config.md)
- [0016 seed 走 runtime schema_check，默认开放两项用显式迁移并告知](0016-seed-timing-and-default-open-migration.md)

### 权限拆分与定级

- [0010 权限按动作域拆分，老应用自动映射](0010-permission-split-compat-map.md) — **被 0013 取代**
- [0013 权限拆分不做兼容映射，旧粗权限直接移除](0013-permission-split-no-compat-map.md) — 取代 0010；Brew 部分被 0022 取代
- [0018 删除旧权限枚举后，已装应用的历史权限清空并标记需重新授权](0018-legacy-grant-clear-and-requeue.md)
- [0020 拆分后细权限定级：读 Basic、写 Elevated](0020-split-permission-levels.md) — Brew 部分被 0022 取代
- [0021 运行时权限过滤遇到未知权限名：签发路径失败、展示路径标记](0021-unknown-permission-fail-semantics.md)
- [0022 Brew 保留 write 粗权限，只拆评论写入](0022-brew-write-remains-coarse.md) — 取代 0013 / 0020 的 Brew 部分

### 宿主密钥

- [0002 宿主密钥使用独立的引用命名空间](0002-host-secret-separate-namespace.md)
- [0003 宿主密钥由管理员逐应用授权，默认拒绝](0003-host-secret-admin-authorization.md)
- [0011 宿主密钥首期只支持静态请求头注入](0011-host-secret-static-header-only.md)
- [0012 宿主密钥缺失时一律 fail-fast，首期不做降级模式](0012-host-secret-fail-fast-on-missing.md)

### TAPP 生命周期与暂存池

- [0005 暂存池统一管理离场实例，运行与否是池内属性](0005-stash-pool-owns-lifecycle.md) — 首段事实被 0014 更正，结论不变
- [0006 暂存池对停止实例用淘汰控制、对运行实例用准入控制](0006-stash-eviction-vs-resident-admission.md)
- [0007 暂存实例恢复前重新确认授权前提](0007-stash-resume-reconfirms-authorization.md)
- [0014 TAPP 生命周期：实例层三状态，卸载从安装层向下级联](0014-instance-lifecycle-and-uninstall-cascade.md)
- [0019 常驻三档映射到 backgroundRequirements 六类](0019-resident-tier-mapping.md)

### 能力面（组件、端点、出站）

- [0008 component:theme 注册锚定安装 owner，guest 不开放](0008-component-theme-owner-scoped.md)
- [0009 出站 http 端点域名固定为 allowlist](0009-outbound-http-fixed-origin.md)
- [0017 builtin 端点权限判定收敛为声明式端点目录](0017-builtin-endpoint-catalog.md)

## 术语

授权链三层（声明/批准/授予）、隐藏/销毁/卸载等概念定义见根目录 [`CONTEXT.md`](../../CONTEXT.md)。
