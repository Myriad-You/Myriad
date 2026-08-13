# 拆分后细权限定级：读 Basic、写 Elevated

Status: accepted

ADR 0013 把六个旧粗权限拆成十五个细权限，但没定每个细权限的 `level()`。六个旧权限原本全是 Basic——读写一体，等于「给了读就连写一起给」，正是痛点一说的「一授全开」。拆分若只拆名字不拆风险，主线白走。

决定：按「会不会改平台或他人可见的持久状态」定级——读的保持 Basic，写的升 Elevated。

## 映射表

| 新权限 | 动作 | level |
|-|-|-|
| `storage:read` | 读 | Basic |
| `storage:write` | 写 | Elevated |
| `ui:theme:read` | 读 | Basic |
| `ui:theme:subscribe` | 订阅 | Basic |
| `media:playback` | 播放/暂停/seek | Basic |
| `media:volume` | 音量 | Basic |
| `media:queue` | 歌单 | Basic |
| `brew:readStatus` | 读 | Basic |
| `brew:favorite` | 收藏 | Basic |
| `brew:commentWrite` | 写评论 | Elevated |
| `federation:post` | 发帖 | Elevated |
| `federation:interact` | 互动 | Basic |
| `federation:channel` | 频道操作 | Elevated |
| `federation:room` | 房间操作 | Elevated |
| `federation:ring` | 响铃 | Basic |

`brew:comment` 的读半边并入已有的 `brew:read`（Basic），不再单列。

所有细权限名遵循既有的 `域:动作` 前缀惯例——`federation:write` 拆出的四个动作均带 `federation:` 前缀，不允许出现 `interact`、`channel`、`room`、`ring` 这类裸名。裸名会让 grep 一个 `channel` 捞到几十处语义无关的命中（发布通道、RSS 标签），且未来任何域要引入「频道」概念都会撞名。

**`brew:write` 的边界已核实**：`docs/development/tapp/fixtures/host_route_permissions.json` 中 `brew:write` 绑定的五条路由全是 `read` / `unread` / `star` / `unstar` / `mark-all-read`，只改当前用户自己的阅读状态与收藏，**不含任何编辑 Brew 内容的写路由**。因此拆为 `brew:readStatus` + `brew:favorite` 是完整的，且两者定 Basic 符合本 ADR 的判据。代码里 `TappPermission::BrewWrite` 的显示文案「编辑 Brew 内容」（`permission_service.rs:257`）是过时描述，随本次移除一并消失，不产生新的 `brew:contentWrite` 权限。

**`ui:theme:*` 与 `component:theme` 是两条线**：`ui:theme:read` / `ui:theme:subscribe` 是「站点主题的读取与订阅」（Basic，本文拆自 `ui:theme`）；`component:theme` 是「注册主题组件」（Elevated，受 ADR 0008 约束为安装 owner 专属），它是独立的既有权限，不在本次拆分之列，也不是 `ui:theme:read` 的降级版。

## Consequences

写类能力从「默认放行」变成「默认关、需站长授权」，与「直接不适配」的破坏性叠加——老应用升级后不仅要改写权限名，写类能力还要重新找站长点头。方向是对的：写操作有破坏面，本来就不该白送。

这个定级改动与 D3 的默认开放范围不冲突——D3 动的 `component:theme` / `shortcut:register` 是 Elevated 里额外默认开放的两项，本表不涉及这两项。
