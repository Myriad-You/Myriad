# 升级说明

## Runtime 配置 seed

启动时会为数据库中缺失的运行时配置写入当前默认值，且不会覆盖站长已有配置。

本次升级有两项显式默认开放的**授予权限**：普通用户默认获得 `component:theme`（主题组件注册）和 `shortcut:register`（快捷键注册）。站长已经保存的关闭值会保留。

此前已保存但因解析缺失未生效的 AI 配额配置，升级后会开始生效；暂存池和常驻配额也会写入默认值：隐藏容量 `8`、隐藏空闲淘汰 `300` 秒、单应用常驻 `1`、全站常驻 `3`。

## TAPP 存储权限拆分（破坏性变更）

声明旧权限 `storage` 的 TAPP Manifest 从本版本起在**安装校验与运行授权签发时一律失败关闭**（fail-closed）：该权限名已从平台权限枚举中移除，不再被识别。

- **不会自动映射。** `storage` 不会被解码成任何新权限，能力也不会被静默丢弃——安装或签发直接报错，而不是缺斤少两地放行。
- **站长必须更新 Manifest。** 把声明的 `storage` 改为 `storage:read`（读取私有存储）和/或 `storage:write`（写入私有存储），然后更新或重装该应用。
- **改变平台下放配置无法修复。** `storage` 是声明层不存在的权限名，调整平台侧的权限下放开关对未知的声明权限没有任何作用。

已安装应用的 `approved_permissions` 里如果还留着无法识别的旧名（例如 `storage`），列表和详情会标 `needs_reauthorization`，授予权限为空，直到更新 Manifest 并更新或重装。不会自动把 `storage` 改写成 `storage:read` / `storage:write`。

## TAPP Brew 权限拆分（破坏性变更）

`brew:write` 继续覆盖已读/未读/全部已读和收藏等当前用户状态；评论与回复的读取并入 `brew:read`，创建、更新与删除改用 `brew:commentWrite`。
- `brew:commentWrite` 是 Elevated 权限，普通用户需由站长显式下放，游客不会获得该授予权限。

已安装应用的旧权限串清理与「需重新授权」标记不随本版本提供，由后续升级迁移单独处理。

## TAPP 联邦写权限拆分（破坏性变更）

声明旧权限 `federation:write` 的 TAPP Manifest 从本版本起会显式失败，不会自动映射或静默降权。请按实际操作改用 `federation:post`、`federation:interact`、`federation:channel`、`federation:room` 和/或 `federation:ring`，然后更新或重装应用。

其中 `federation:post`、`federation:channel`、`federation:room` 为 Elevated 权限，普通用户默认不下放；`federation:interact`、`federation:ring` 为 Basic 权限。所有联邦写操作仍要求持久登录主体，游客无法获得这些能力。
