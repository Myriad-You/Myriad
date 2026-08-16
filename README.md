<div align="center">

<img src="frontend/public/logo.webp" alt="Myriad" width="120" />

# Myriad

### 让每一个你，被看见

*A myriad of lights, in one place.*

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/myriad-you/Myriad)](https://github.com/myriad-you/Myriad/releases)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](backend/Cargo.toml)
[![Astro](https://img.shields.io/badge/Astro-7-blueviolet.svg)](frontend/package.json)
[![React 19](https://img.shields.io/badge/React-19-61dafb.svg)](frontend/package.json)

[快速开始](docs/QUICKSTART.md) · [文档门户](docs/quick-reference/INDEX.md) · [反馈](https://github.com/myriad-you/Myriad/issues)

</div>

---

## 关于 Myriad

我们在 GitHub 写代码、在 Bilibili 看视频、在 Steam 游玩、在网易云听歌——
每一处都留下了一点自己，但它们彼此不相往来。

**Myriad 是个人在互联网的数字生活展示入口。**
它把散落在各处的你，重新聚合到一个地方：让你自己看见，也让别人看见。

> 既可以作为公开的个人门户对外展示，也可以作为私有的数字仓库自己使用。

---

## 你可以用它做什么

<table>
<tr>
<td width="50%" valign="top">

### 把多平台汇成一处
连接 GitHub、Bilibili、Steam、网易云、YouTube、Bangumi、Discord、X、MAL、Xbox、PlayStation 等平台；
数据落入自有 PostgreSQL，由你完全拥有。

</td>
<td width="50%" valign="top">

### 首页小组件与资料库
欢迎语、音乐、天气、访客、游戏 Presence、社交网络、Report Card 等可编排小组件；
资料库整理游戏、番剧、音乐与活动轨迹。

</td>
</tr>
<tr>
<td width="50%" valign="top">

### Brew 订阅与 Arael 智能体
**Brew** 聚合 RSS / Notion / RSSHub 等阅读源。
**Arael** 多层 Agent（计划、执行、记忆、MCP）持续阅读你的数据，整理报告与洞察。

</td>
<td width="50%" valign="top">

### Tapp 扩展与联邦
**Tapp** 沙箱让你像写小程序一样挂载 Page / Widget。
内置 **ActivityPub / MFP** 联邦，可被其他实例关注、建频道或加入环网。

</td>
</tr>
</table>

生产环境通过 **proxy + updater** 自更新：只有 proxy 对外暴露端口，版本切换与回滚走管理员「更新管理」，无需手动覆盖 `:latest`。

---

## 快速开始

### Docker（推荐）

```bash
git clone https://github.com/myriad-you/Myriad.git
cd Myriad

cp .env.production.example .env
# 至少设置：POSTGRES_PASSWORD / JWT_SECRET / CORS_ORIGINS
# BASE_URL / FRONTEND_URL 填你的公网域名（联邦发现依赖 BASE_URL）
# UPDATE_TOKEN / UPDATER_GATEWAY_SECRET 留空时由 deploy 脚本生成

bash scripts/docker/deploy.sh up          # Windows: .\scripts\docker\deploy.ps1 up
```

打开 `http://localhost`（或 `.env` 中的 `HTTP_PORT`），按引导完成初始化。

```text
host HTTP_PORT → proxy → frontend:1102
                      → backend:1103 → postgres:5432
                      → updater（仅内网；经 updater-gateway）
```

更完整的部署、备份与排错见 [快速开始](docs/QUICKSTART.md)。

### 本地开发

```bash
./scripts/dev/dev.sh start             # 本机 PostgreSQL，日志打在当前终端
./scripts/dev/dev.sh start --docker    # Docker postgres + 新开终端
.\scripts\dev\dev.ps1 start            # Windows
```

后端 `:1103`，前端 `:1102`。没有本机库时先 `./scripts/dev/dev.sh db-setup`。
需要在开发 UI 里测「更新管理」时：`./scripts/dev/dev.sh start all-updater`。
更多细节见 [快速开始](docs/QUICKSTART.md)。


---

## 技术构成

| | |
| --- | --- |
| **前端** | Astro 7 · React 19 · Tailwind 4 · TypeScript |
| **后端** | Rust · Axum 0.8 · SeaORM · Tokio |
| **边缘** | proxy（反向代理 / 维护页）· updater（自更新 / 快照） |
| **数据** | PostgreSQL 18（Compose 默认镜像） |
| **扩展** | Arael Agent · Tapp 沙箱 · MCP · ActivityPub / MFP |
| **部署** | Docker Compose · linux/amd64 + arm64 镜像 |

---

## 仓库结构

```
Myriad/
├── backend/          Rust API、SeaORM、migrations
├── frontend/         Astro + React UI、Tapp 运行时、i18n（中/英/日）
├── proxy/            生产入口反向代理（独立 Cargo 树）
├── updater/          自更新守护进程（独立 Cargo 树）
├── crates/           工作区共享库（error、outbound、image-proxy、tapp-…）
├── shared/           跨组件静态配置（如图片代理域名名单）
├── docker/           backend / frontend Dockerfile
├── docs/             开发、部署与功能文档
├── scripts/          开发与部署脚本
├── release/          release.json 契约与覆盖
├── tools/            tapp-cli 等工具
└── docker-compose*.yml
```

---

## 文档

| | |
| --- | --- |
| [文档门户](docs/quick-reference/INDEX.md) | 总索引 |
| [快速开始](docs/QUICKSTART.md) | 部署与本地开发 |
| [架构总览](docs/development/ARCHITECTURE.md) | 系统结构 |
| [构建说明](docs/development/BUILD.md) | 从源码构建 |
| [API](docs/API.md) | HTTP API |
| [Tapp 开发](docs/development/TAPP_DEVELOPMENT.md) | 扩展应用 |
| [联邦](docs/development/FEDERATION.md) | ActivityPub / MFP |
| [OAuth / 登录](docs/development/OAUTH.md) | 本地账号与 OIDC |
| [资料库](docs/features/LIBRARY.md) | Library 功能 |
| [Docker 部署](docs/deployment/DOCKER_DEPLOYMENT.md) | 生产编排细节 |
| [端口清单](docs/deployment/PORTS.md) | 端口与暴露面 |
| [Updater 运维](docs/UPDATER_QUICKSTART.md) | 自更新通道 |
| [外部 PostgreSQL](docs/deployment/EXTERNAL_POSTGRES.md) | 自带库以外的 PG |
| [无 Docker 部署](docs/deployment/NATIVE_DEPLOYMENT.md) | 本机 PostgreSQL + 二进制 |
| [Setup 引导令牌](docs/deployment/SETUP_BOOTSTRAP.md) | CONFIG_MODE 破窗令牌；编排安装时的所有者暗号 |

---

## 贡献

Issue 与 PR 都欢迎。UI 文案改动请同步更新中 / 英 / 日三语 i18n（`zh-CN` / `en-US` / `ja-JP`）。

## License

根仓库与主应用为 [GPL-3.0](LICENSE)。`proxy` / `updater` 组件另行声明为 AGPL-3.0。

<br/>

<div align="center">
<sub><i>让每一个你，被看见</i> · <i>A myriad of lights, in one place.</i></sub>
<br/>
<sub>Maintained by <a href="https://github.com/myriad-you">@myriad-you</a></sub>
</div>
