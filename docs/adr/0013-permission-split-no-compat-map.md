# 权限拆分不做兼容映射，旧粗权限直接移除

Status: accepted
Supersedes: ADR-0010

ADR 0010 决定为旧粗权限保留兼容映射（expand 阶段），让老 manifest 继续可用。这个决定被推翻。

仓库内引用旧粗权限的主要是两份 host/action fixture 和生成的 `contract.json`，没有仓库外 manifest 依赖它们。为一个没有实际存量的兼容面维护一张映射表，代价高于收益：映射表要在授予层展开、要进一致性测试、要在将来删除时再做一次迁移，而它保护的是不存在的用户。

另有一处非 fixture 的存量需在删除时一并处理：agent capability 定义里有一处过时引用——`backend/src/services/agent/capability/definitions/brew.rs:247` 的「添加订阅源」声明 `required_permissions: ["brew:write"]`，而该 capability 对应的真实 host 路由 `/api/brew/sources`（POST）绑的是 `brew:manage`（Privileged）。这是一处**过时的权限串**，不是需要保护的真实依赖：删除 `brew:write` 时应把它改成 `brew:manage`，语义与现状一致（agent_footer 亦注明「加/改/删源仅管理员」）。它不构成保留兼容映射的理由，但构成 D1 完成标准里必须列出的一处改动点。

决定：按动作域拆细权限的同时，**直接从枚举中移除旧粗权限**，不保留任何映射规则。被移除的是 `storage`、`ui:theme`、`media:control`、`brew:write`、`brew:comment`、`federation:write`。声明这些权限的 manifest 在安装校验时被明确拒绝，错误信息指出应改用的新权限名。

## Considered Options

**保留兼容映射（ADR 0010 的方案）。** 老应用不破，但要维护一张跨三层的映射表，并在 contract 阶段再迁移一次。存量为零时，这张表保护的是假想的用户。

**保留旧枚举但不授予任何能力。** 名字还在、装得进去、但什么都做不了。这是三种结果里最坏的一种：应用以为拿到了权限，实际调用全部失败，且失败原因不在声明处。

## Consequences

拆分后只有一套权限名，没有「新旧并存」的中间态；契约、前端映射表、CLI 导出的 `contract.json` 都只描述一种事实。

代价是这是一次破坏性变更：声明旧粗权限的 manifest 必须改写才能安装。因为破坏面已核实为仓库内的 fixture，这个代价一次性付清。

**未知权限名必须显式失败，不得静默丢弃。** 安装校验已有拒绝路径，但运行时按角色过滤权限的路径使用 `filter_map`，会把不认识的名字直接丢掉。删除旧枚举前必须先让这条路径对未知名字显式报错，否则已装的老应用升级后会静默降权——这正是 ADR 0001 与 ADR 0004 要消灭的那类静默失败。失败在各调用路径上的具体形态（签发 fail-closed、展示标记需重新授权）由 ADR 0021 定义。
