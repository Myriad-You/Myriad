# Brew 保留 write 粗权限，只拆评论写入

Status: accepted
Supersedes: ADR-0013（仅 Brew）、ADR-0020（仅 Brew）

ADR 0013 / 0020 原计划删除 `brew:write`，拆成 `brew:readStatus` 与
`brew:favorite`。复核实际动作后，这两项都只修改当前用户自己的阅读状态：五条宿主路由
分别是已读、未读、全部已读、收藏和取消收藏
（`docs/development/tapp/fixtures/host_route_permissions.json:87`）。继续拆分会增加声明、批准和
授予权限的组合数量，但不会形成不同的下放边界。

决定：保留 Basic 级别的 `brew:write`，继续覆盖上述五个当前用户状态动作；不引入
`brew:readStatus` 或 `brew:favorite`。`brew:write` 仍要求持久登录主体，不能形成游客授予权限。

评论能力仍按动作性质拆分：读取评论和回复并入 `brew:read`；创建、编辑、删除评论与创建
回复使用 Elevated 的 `brew:commentWrite`。订阅源和分类管理继续使用 Privileged 的
`brew:manage`。三者在前端动作目录中的绑定见
`frontend/src/tapp/runtime/permissionConfig.ts:148`、`:156` 与 `:163`。

## Consequences

声明过 `brew:write` 的应用无需因阅读状态与收藏动作重新授权；运行时仍按当前用户隔离数据。
声明旧 `brew:comment` 的应用必须改为 `brew:read` 和/或 `brew:commentWrite`，未知名处理继续遵循
ADR 0021 的 fail-closed / 展示标记语义。

ADR 0013 对其它旧粗权限、ADR 0020 对其它拆分权限的结论不变。
