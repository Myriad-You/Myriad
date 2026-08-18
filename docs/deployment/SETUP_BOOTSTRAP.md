# Setup 安装暗号

安装控制面只有一把给用户用的钥匙：**安装暗号** `MYRIAD_SETUP_SECRET`。

| 凭证 | 何时要 | 放哪 |
| --- | --- | --- |
| **安装暗号** `MYRIAD_SETUP_SECRET` | 编排已经写好数据库时，所有安装写操作 | `.env` / compose 环境 |

不是给第三方调用的 API key，也不进高级设置。

相关代码：`backend/src/api/setup_bootstrap.rs`。

---

## 安装暗号

只在**编排里已经写好数据库**时才要。这时栈一启动就能建所有者，谁先点谁就是站长，所以写操作必须对上暗号。

向导自己填数据库的路径不预置这枚值，也不显示这个框。

- 进程环境里有非空 `MYRIAD_SETUP_SECRET` → 连库 / 建表 / 改 `.env` / 创建所有者都必须对上。官方向导走请求头 `X-Setup-Secret`；手写客户端也可以只放 JSON `setup_secret`。中间件只拦向导窗口，暗号在读完 body 之后校验，两种送法等效。
- 没配这枚值 → 放行
- **编排生成器**（`com.myriad.config-generator`）和 `deploy.sh` 会写入并注入 backend
- 官方 `docker-compose.yml` 已经写好 `DATABASE_URL`，未设置 `MYRIAD_SETUP_SECRET` 时 compose 会拒绝启动
- 向导的 `init-env` / `database-config` **不会**再自动生成
- HTTP 响应和启动日志都不回传正文

```bash
# Docker / 编排产物
grep '^MYRIAD_SETUP_SECRET=' .env
```

配了但填错是 **401**。没配则不会出现这个框。

编排生成器完成页会给出 `https://你的域名/#setup_secret=…`。打开后向导自动填入暗号，并立刻从地址栏清掉。优先用 **hash**，不要用 `?setup_secret=`（查询串会进 Nginx 访问日志）。链接等同于能当站长，勿发到公开频道。

---

## 要解决什么

`CONFIG_MODE` 会在下面两种情况打开：

- 进程环境里没有 `DATABASE_URL`
- 有 `DATABASE_URL`，但连不上 PostgreSQL（重试失败后自动降级）

配置模式下，setup 路由可以改写 `.env`。认领之后磁盘上的 `.bootstrap-claimed` 阻止重开向导。库挂了进程仍应能起来（`/health`、静态页），但向导窗口保持关闭，setup 写操作返回「向导已关闭」。先修库，不要用 setup 改宿主配置。

| 情形 | 判定 | 浏览器向导 |
| --- | --- | --- |
| 首次安装（向导窗口开着） | 还没有站长，磁盘也没有认领标记 | 浏览器可走。编排预置了安装暗号则所有写操作必须对上。 |
| 已认领之后库挂了 | 磁盘有 `.bootstrap-claimed` | 进程可启动。`/api/setup/config` 的 `setup_window_open` 为 false，向导只提示修库，不画写步骤。 |

---

## 不要做什么

- **不要**把安装暗号做到高级设置、诊断 JSON、`/health` 或任何已登录 API 里。能进后台 ≠ 能上机器。
- **不要**当长期自动化凭证。实例正常时改配置走管理员登录。
- **不要**贴进 issue、聊天、CI 日志。泄露后改 `.env` 里的 `MYRIAD_SETUP_SECRET` 并重启。
- **不要**和 `UPDATE_TOKEN` / `UPDATER_GATEWAY_SECRET` / `JWT_SECRET` 混用。它们各管各的面。
- **不要**再设 `MYRIAD_ALLOW_REMOTE_BOOTSTRAP`。旧 `.env` 里的该键会被忽略。

---

## 排错

| 现象 | 原因 / 处理 |
| --- | --- |
| 向导创建所有者返回 401 | 编排预置了 `MYRIAD_SETUP_SECRET` 但对不上。从宿主 `.env` 复制。向导自己填库时不该出现这个框。 |
| 向导保存数据库 / 建表返回 401 | 同上。编排安装的写操作都要暗号，不只是创建所有者。 |
| 填了暗号仍 401 且提示向导已关闭 | 实例已经认领。库挂了也一样：进程还在，向导不会重开。先修 postgres。 |

---

## 和其它密钥的关系

| 凭证 | 用途 | 出现位置 |
| --- | --- | --- |
| `MYRIAD_SETUP_SECRET` | 编排预置库时，安装写操作对暗号 | `.env` / compose，**值不进前端** |
| `JWT_SECRET` | 登录会话签名 | `.env` |
| `UPDATE_TOKEN` | updater / gateway / docker-guard | 不进 backend |
| `UPDATER_GATEWAY_SECRET` | backend → updater-gateway | backend + gateway |

日常更新仍走 `/config → 关于 → 更新管理`。
