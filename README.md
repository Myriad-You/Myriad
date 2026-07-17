<div align="center">

<img src="frontend/public/logo.webp" alt="Myriad" width="120" />

# Myriad

### 让每一个你，被看见

*A myriad of lights, in one place.*

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](backend/Cargo.toml)
[![Astro](https://img.shields.io/badge/Astro-7-blueviolet.svg)](frontend/package.json)
[![React 19](https://img.shields.io/badge/React-19-61dafb.svg)](frontend/package.json)

[在线体验](#) · [快速开始](#-快速开始) · [文档](docs/) · [反馈](https://github.com/myriad-you/Myriad/issues)

</div>

---

## ✨ 关于 Myriad

我们在 GitHub 写代码、在 Bilibili 看视频、在 Steam 游玩、在 Notion 记录、在网易云听歌——
每一处都留下了一点自己，但它们彼此不相往来。

**Myriad 是个人在互联网的数字生活展示入口。**
它把散落在各处的你，重新聚合到一个地方：让你自己看见，也让别人看见。

> 既可以作为公开的个人门户对外展示，也可以作为私有的数字仓库自己使用。

---

## 🌟 你可以用它做什么

<table>
<tr>
<td width="50%" valign="top">

### 📡 把你的多平台汇成一处
连接 GitHub、Bilibili、Steam、网易云、Notion、RSSHub 等多个平台，
所有数据流入同一个本地数据库，你完全拥有它们。

</td>
<td width="50%" valign="top">

### 🧠 让 AI 替你阅读和组织
内置多层智能体——计划、执行、记忆、技能演化——
持续阅读你的数据，整理出报告、推荐与洞察。

</td>
</tr>
<tr>
<td width="50%" valign="top">

### 🧩 像写小程序一样扩展它
**Tapp 沙箱**让任何人写一个"应用"挂载到 Myriad 上：
声明式 API、组件、快捷键、事件总线，应有尽有。

</td>
<td width="50%" valign="top">

### 🌐 公开展示 / 私人收藏，皆可
作为门户开放给世界，作为知识库留给自己。
权限粒度、内容可见性、对外接口都由你来定。

</td>
</tr>
</table>

---

## 🚀 快速开始

### Docker（推荐）

```bash
git clone https://github.com/myriad-you/Myriad.git
cd Myriad

cp .env.production.example .env
# 编辑 .env：POSTGRES_PASSWORD / JWT_SECRET / CORS_ORIGINS
# UPDATE_TOKEN / UPDATER_GATEWAY_SECRET 留空时会由 deploy 脚本生成

bash scripts/docker/deploy.sh up
```

打开 `http://localhost`（或 `.env` 中的 `HTTP_PORT`），按引导完成初始化（数据库 + 管理员）。

生产拓扑只有 `proxy` 暴露宿主端口；`backend`、`frontend` 和 `updater` 都在内部网络中。
更新通过管理员界面的 updater 通道完成，不再手动覆盖 `:latest` 镜像。

### 本地开发

```bash
# 后端（需要 Rust 1.88+，推荐 1.90；PostgreSQL 16+）
cd backend && cp .env.example .env && cargo run

# 前端（需要 Node 25、pnpm）
cd frontend && pnpm install && pnpm dev
```

或一键脚本：

```bash
./scripts/dev/dev.sh        # macOS / Linux
.\scripts\dev\dev.ps1       # Windows
```

---

## 🏛 技术构成

<table>
<tr>
<td><b>前端</b></td><td>Astro 7 · React 19 · Tailwind 4 · TypeScript</td>
</tr>
<tr>
<td><b>后端</b></td><td>Rust · Axum 0.8 · SeaORM · Tokio</td>
</tr>
<tr>
<td><b>数据</b></td><td>PostgreSQL 16+</td>
</tr>
<tr>
<td><b>智能</b></td><td>多层 Agent 编排 · Tapp 可编程沙箱 · MCP</td>
</tr>
<tr>
<td><b>部署</b></td><td>Docker / Docker Compose · 多架构镜像</td>
</tr>
</table>

---

## 📂 仓库结构

```
Myriad/
├── backend/        Rust + Axum
│   └── src/
│       ├── api/            HTTP 路由
│       ├── services/       业务服务（agent / ai / fetcher / ...）
│       │   └── agent/      智能体子系统
│       └── models/         SeaORM 模型
├── frontend/       Astro + React
│   └── src/
│       ├── views/          页面
│       ├── tapp/           Tapp 运行时与示例
│       └── i18n/           中英日三语
├── docker/         构建与编排
├── docs/           开发与部署文档
└── scripts/        开发与部署脚本
```

---

## 📖 文档

- [架构总览](docs/development/ARCHITECTURE.md)
- [Tapp 开发](docs/development/TAPP_DEVELOPMENT.md)
- [构建说明](docs/development/BUILD.md)
- [API](docs/API.md)
- [生产部署](docs/deployment/DOCKER_DEPLOYMENT.md)
- [端口清单](docs/deployment/PORTS.md)
- [Updater 运维](docs/UPDATER_QUICKSTART.md)

---

## 🤝 贡献

Issue 与 PR 都欢迎。UI 改动请同步更新中 / 英 / 日三语 i18n。

## 📝 License

[GPL-3.0](LICENSE)

<br/>

<div align="center">
<sub><i>让每一个你，被看见</i> · <i>A myriad of lights, in one place.</i></sub>
<br/>
<sub>Maintained by <a href="https://github.com/myriad-you">@myriad-you</a></sub>
</div>
