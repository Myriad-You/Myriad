# component:theme 注册锚定安装 owner，guest 不开放

Status: accepted

主题组件是站点级外观资源。曾有一版设计让访客的 session 主题直接汇入宿主主题选择器，造成两项后果：访客的主题混进管理员的编辑面板，且同 id 主题会遮蔽管理员已固化的持久主题。这是上一版被 review 打回的做法，本次要避免。

决定：`component:theme` 的注册保持安装 owner 专属——只有 subject 等于安装 owner 才能注册；guest 不开放该权限（默认关闭）。普通用户即使获得 `component:theme`，也只在「自己是安装 owner」时才能注册。

## 明确不做

- 不做按用户分域的个性化主题（subject namespace）。
- 不做 session 主题合并。
- 不为主题 id 引入命名空间前缀。

这三项是上一版泄漏与遮蔽问题的来源。当前 owner 专属边界下，访客与普通用户根本写不进站点的主题 namespace，泄漏与遮蔽在结构上不存在，无需额外命名空间来兜。
