# 升级说明

## TAPP 旧权限清理与重新授权

本次升级会从**已安装** TAPP 的批准权限与授予权限两列中移除三项已退役的粗权限：`storage`、`brew:comment`、`federation:write`。这些权限名已从权限枚举中删除，继续保留会让已装应用读回即失败。仍然有效的 `ui:theme`、`media:control`、`brew:write` 会原样保留，不会触发重新授权标记。

**这不是自动权限转换**：迁移不做任何旧权限到新权限的映射，也不会自动授予替代权限。被移除权限的安装会被标记为「需重新授权」，在站长重新授权前：

- 运行时不签发 runtime grant（fail-closed），TAPP 无法启动；
- 目录列表/详情中该应用呈现为需重新授权状态，可发起更新/重新授权。

**数据清理不可逆**：被移除的旧权限串无法恢复，且**回滚该迁移不会删除 `needs_reauthorization` 列，也不会清除已落库的重新授权标记**——持久信号必须在回滚后继续存在，否则受影响安装会在权限已清空的情况下被静默放行。

受影响安装的**审批权限与授予权限会各自独立清理**，与旧权限无关的权限及其顺序保持不变。请站长在升级后**逐台检查被标记的应用**，为其重新授权当前有效的权限名。新安装与正常更新不受影响。

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

## TAPP 联邦写权限拆分（破坏性变更）

声明旧权限 `federation:write` 的 TAPP Manifest 从本版本起会显式失败，不会自动映射或静默降权。请按实际操作改用 `federation:post`、`federation:interact`、`federation:channel`、`federation:room` 和/或 `federation:ring`，然后更新或重装应用。

其中 `federation:post`、`federation:channel`、`federation:room` 为 Elevated 权限，普通用户默认不下放；`federation:interact`、`federation:ring` 为 Basic 权限。所有联邦写操作仍要求持久登录主体，游客无法获得这些能力。
