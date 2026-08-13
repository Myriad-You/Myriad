# 权限按动作域拆分，老应用自动映射

Status: superseded by ADR-0013

现状是粗权限一授全开：`media:control` 一个权限就盖住播放、音量、歌单全部操作，`storage` 盖住读与写，`federation:write` 盖住发帖、关注、频道、房间、Ring 五个动作域。

决定：按动作域拆细权限（`media:control` → `media:playback` / `media:volume` / `media:queue` 等，见飞书方案拆法）。老应用 manifest 里声明的粗权限自动映射到对应细权限集合，行为不变——这是 expand 阶段，不逼老应用立刻升级。

## Consequences

拆分后新应用可以声明最小够用的一块，而不是被迫全要。老应用的粗权限作为兼容映射保留，等自然迁移完成后另行评估移除时机。这个「映射」和「细权限」并存的结构，就是宽重构的 expand 半边。
