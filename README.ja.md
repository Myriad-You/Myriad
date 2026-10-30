<div align="center">

<img src="frontend/public/logo.webp" alt="Myriad" width="120" />

# Myriad

### あなたという物語を、ひとつに

<em>A myriad of lights, in one place.</em>

<p>
<a href="README.md">English</a>
&nbsp;·&nbsp;
<a href="README.zh-CN.md">中文</a>
&nbsp;·&nbsp;
<strong>日本語</strong>
</p>

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/myriad-you/Myriad)](https://github.com/myriad-you/Myriad/releases)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://github.com/rust-lang/rust)
[![Vite](https://img.shields.io/badge/Vite-8-blueviolet.svg)](https://github.com/vitejs/vite)
[![React 19](https://img.shields.io/badge/React-19-61dafb.svg)](https://github.com/facebook/react)

[クイックスタート](docs/QUICKSTART.md) · [ドキュメント](docs/INDEX.md) · [フィードバック](https://github.com/myriad-you/Myriad/issues)

[![Discord](https://img.shields.io/badge/Discord-参加する-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://discord.gg/Jr5HccgxyD)
[![X](https://img.shields.io/badge/X-フォロー-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/myriadyou)
[![Telegram](https://img.shields.io/badge/Telegram-参加する-26A5E4?style=for-the-badge&logo=telegram&logoColor=white)](https://t.me/myriadyou)
![QQ グループ](https://img.shields.io/badge/QQ%20グループ-784229925-12B7F5?style=for-the-badge&logo=tencentqq&logoColor=white)

</div>

<p align="center">
<picture>
  <source media="(prefers-reduced-motion: reduce) and (prefers-color-scheme: dark)" srcset="docs/assets/readme/home-ja-dark-still.webp">
  <source media="(prefers-reduced-motion: reduce)" srcset="docs/assets/readme/home-ja-light-still.webp">
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme/home-ja-dark.webp">
  <img src="docs/assets/readme/home-ja-light.webp" width="880" alt="Myriad のホームページ：大きなタイトルの下にオーナーのカードとウィジェットの列——あいさつ、SNS アイコン、Steam と Bangumi のレポートカード、彼女の立ち絵、相互リンク、天気、NetEase と GitHub のレポートカード、音楽、最近のアクティビティ。レポートカードの中のカバーが切り替わっていく。">
</picture>
<br><sub>オフスクリーン描画：このリポジトリのフロントエンド（本番ビルド）をローカルのインスタンスにつなぎ、1 フレームずつ描いたもの。オーナー、作品、数字は合成したサンプルで、キャラクターはプロジェクト同梱のものです。[録画方法](docs/development/README_MEDIA.md)（中国語）。</sub>
</p>

## これは何か

**Myriad はセルフホストの個人ホームページです。** いろいろなプラットフォームに残したものをひとつのサイトに集め、サイトにひとりのキャラクターを置きます。彼女は自分の 2.5D の姿を持ち、訪れた人と話し、あなたの代わりに仕事もします。小さなアプリを入れたり、ブログを書いたりもできます。

シングルテナント。公開または非公開。PostgreSQL。UI：`zh-CN` · `zh-TW` · `en-US` · `ja-JP` · `ko-KR` · `fr-FR` · `de-DE`。

- **ホームページ**：標準 16×4 か自由 16×8 のグリッドに、ウィジェットを好きに並べます。自由レイアウトにはステッカーも貼れます。レポートカード、音楽、天気、相互リンク、最近のアクティビティ、彼女のウィジェットがここに並び、入れた Tapp もウィジェットを置けます。
- **プラットフォーム**：GitHub、Steam、Bilibili、NetEase Music、Bangumi など 11 のプラットフォームのデータがひとつのデータベースに入り、レポートカード、ライブラリ、プラットフォームレポートは同じデータを読みます（[プラットフォーム](#プラットフォーム)）。

## 彼女と話す

<p align="center">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme/chat-ja-dark.webp">
  <img src="docs/assets/readme/chat-ja-light.webp" width="720" alt="ホームページの空いたところを長押しして Agent パネルを開き、「会話」に切り替えて一言入力する。返事が少しずつ現れ、パネルの上の彼女の顔が返事に合わせて口を動かす。">
</picture>
<br><sub>オフスクリーン描画。彼女の返事はあらかじめ書いた代役で、その場でモデルが生成したものではありません。</sub>
</p>

- **サイトにひとつのペルソナ**：名前、性格、立ち絵はオーナーが決め、誰と話しても同じひとりです。ペルソナがオンで名前が未設定なら、彼女の名前は Arael です。
- **会話と仕事**：ページの空いたところを長押しするとパネルが開き、Tab で「会話」と「仕事」を切り替えます。会話ではペルソナとして話し、仕事では付与された権限の範囲で計画し、確認して、実行します（[Agent](#agent)）。
- **口の形**：音声がオフのときは、返事の文字から口の形を割り出します。中国語、日本語、英語はそれぞれの発音で読みます。

## 2.5D ビジュアル

<p align="center">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme/face-ja-dark.webp">
  <img src="docs/assets/readme/face-ja-light.webp" width="680" alt="キャラクター設定の「モーション」タブ：左に彼女の上半身、右に表情のボタン。照れ、わくわく、ときめき過ぎ、泣く、ぎゅっと目、くらくら、鼻歌、目を閉じて笑顔を順に押すと顔が変わり、表情によっては記号が描かれる。">
</picture>
<br><sub>オフスクリーン描画。キャラクター設定 › モーション。</sub>
</p>

- **ブラウザの中で動く**：レイヤー分けした上半身の立ち絵をブラウザがその場で再生します（[Anime2.5DRig](https://github.com/852wa/Anime2.5DRig)）。サーバーで GPU 推論はしません。
- **立ち絵から動くまで**：メインの立ち絵は生成もアップロードもできます。レイヤーは PSD から、またはレイヤー分割サービスで自動で分けられます。まばたき、口の形、頭と体の揺れはレイヤーの上で動きます。
- **表情と動作**：ワークベンチで試せて、会話中は彼女が自分で選びます。表情によっては涙、音符、ぐるぐるのような記号が描かれます。

- **振り向きは「描いて」作る**：取り込み時に画像編集モデルで同じ立ち絵を、頭が左右上下にそれぞれ約 30° 向いた姿に描き直させ、レイヤー分解したうえで部位ごとにキーフォームを測ります。再生は決定的なメッシュ変形で、推論はしません。手法と計測は技術報告 [*Drawn, Not Computed*](https://zenodo.org/records/23598773) にまとめています。

<details>
<summary>引用</summary>

```bibtex
@misc{wang_2026_23598773,
  author    = {Wang, Mingen},
  title     = {Drawn, Not Computed: Measuring Live2D-Style Turn Keyforms from Generated Views of a Single Anime Illustration},
  month     = oct,
  year      = 2026,
  publisher = {Zenodo},
  doi       = {10.5281/zenodo.23598773},
  url       = {https://doi.org/10.5281/zenodo.23598773},
}
```

</details>

## プラットフォームレポート

<p align="center">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme/report-ja-dark.webp">
  <img src="docs/assets/readme/report-ja-light.webp" width="800" alt="ホームページの Steam レポートカードで再生を押すと、ポップアップで「横顔」「好み」「ハイライト」の三幕が一行ずつ流れ、下に関連するゲームのカバーが並ぶ。">
</picture>
<br><sub>オフスクリーン描画。レポートの文章は手書きのサンプルです。実際のレポートはプラットフォームのデータからモデルが書きます。</sub>
</p>

- プラットフォームごとに一枚の横顔を、横顔・好み・ハイライトの三幕で描きます。専用ページはなく、ホームページのレポートカードを押すとポップアップで一幕ずつ再生されます。
- オーナーは同じ場所で再生成、有効期限、レポートのスタイルを設定します。

## ライブラリ

<p align="center">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme/library-ja-dark.webp">
  <img src="docs/assets/readme/library-ja-light.webp" width="720" alt="ライブラリの無限キャンバス：キャンバスをドラッグして縮小し、左のカテゴリでゲーム、動画、すべてに切り替える。">
</picture>
<br><sub>オフスクリーン描画。</sub>
</p>

- ゲーム、アニメ、映像、本、音楽を、つないだプラットフォームから同期し、種類で絞り込みます。種類ごとの取得元も設定できます。
- リストか無限キャンバス：ドラッグ、矢印キーで移動、ホイールで拡大縮小。性能の低い端末ではリストに戻ります。

## Tapp

<p align="center">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme/tapp-ja-dark.webp">
  <img src="docs/assets/readme/tapp-ja-light.webp" width="800" alt="Tapp プレイグラウンド：「ポモドーロタイマーを作って」と入力すると生成の手順がひとつずつ表示され、終わると左のプレビューにタイマー、右にコードが出る。スタートを押すと 25:00 からカウントダウンが進む。">
</picture>
<br><sub>オフスクリーン描画。生成は代役で、手順とコードはあらかじめ書いたものです。その場でモデルが生成したものではありません。</sub>
</p>

- **サイトの小さなアプリ**：ページ、ホームのウィジェット、またはその両方。サンドボックスで動き、インストール時に権限を承認します。ホストの秘密情報とアプリの認証情報はサンドボックスに入らず、エラーにも出ません。
- **プレイグラウンド**（デスクトップ、オーナー）：一文でプロジェクトを生成し、本番と同じサンドボックスでプレビューして、気に入ればインストールするか `.tapp` に書き出します。
- **ストアと CLI**：[tapp-store](https://github.com/Myriad-You/tapp-store) と [`tapp-cli`](tools/tapp-cli/README.md)（`myriad-tapp init / check / pack`）。開発は [Tapp 開発](docs/development/TAPP_DEVELOPMENT.md)。

## ブログ

<p align="center">
<picture>
  <img src="docs/assets/readme/blog-ja-light.webp" width="800" alt="ブログでスタジオのエディタを開き、タイトルと一段落を書いて公開を押す。ブログのトップに戻ると新しい記事がいちばん上にあり、押すと記事ページが開く。">
</picture>
<br><sub>オフスクリーン描画。公開のあと、ローカルのインスタンスが実際に Hugo で静的ブログを作り直しています。</sub>
</p>

- スタジオでノートを書き、公開すると Hugo テーマで静的ブログになり、`/blog` に置かれます。ページではスクリプトを動かさず、コメントと相互リンク申請は同一オリジンの埋め込みページです。
- 購読（RSS、Notion、RSSHub）は内蔵の「購読」アプリで読みます（[Phantasi](#phantasi)）。

## ほかにも

- **グループと DM**：彼女は QQ、Telegram、Discord のグループにもいて、メンションされると返事をし、誰にも呼ばれないときもときどき自分から口をはさみます。ペアリングした DM ボットは「仕事」と同じ流れを通ります。
- **連合**：ActivityPub と MFP。ほかのサイトをフォローできます（[連合](#連合)）。
- **版の切り替えとロールバック**：`/config` → 情報 → 更新管理でひと手間です（[運用](#運用)）。

---

## 目次

**製品** — [彼女と話す](#彼女と話す) · [2.5D ビジュアル](#25d-ビジュアル) · [プラットフォームレポート](#プラットフォームレポート) · [ライブラリ](#ライブラリ) · [Tapp](#tapp) · [ブログ](#ブログ) · [プラットフォーム](#プラットフォーム) · [Agent](#agent) · [Phantasi](#phantasi) · [連合](#連合) · [アカウント](#アカウントと公開範囲)

**実行** — [要件](#要件) · [デプロイ](#デプロイ) · [開発](#ローカル開発) · [運用](#運用)

**参照** — [アーキテクチャ](#アーキテクチャ) · [リポジトリ](#リポジトリ) · [技術構成](#技術構成) · [ドキュメント](#ドキュメント) · [License](#license)

---

## プラットフォーム

<table>
<tr>
<td width="50%" valign="top">

**GitHub** — リポジトリ、Star、コントリビューション  
**Steam** — ライブラリ、ウィッシュリスト、プレイ統計  
**YouTube** — 公開チャンネル、最近の投稿  
**Discord** — プロフィール、サーバー、連携アカウント  
**MyAnimeList** — アニメ / マンガのリストと点数  
**PlayStation** — トロフィー、レベル、最近のゲーム

</td>
<td width="50%" valign="top">

**Bilibili** — お気に入り、アニメ、視聴履歴  
**網易雲** — 好きな曲と傾向  
**Bangumi** — コレクション、評価、視聴状態  
**X** — プロフィールと投稿  
**Xbox** — 実績、Gamerscore、最近のゲーム

</td>
</tr>
</table>

PostgreSQL に保存。Report Card、ライブラリ、レポートは同一データを読む。

---

## Agent

<table>
<tr>
<td width="50%" valign="top">

**Chat**

ペルソナオン時はそのペルソナとしてのみ話す。検索、予約、生成、計画なし。着替えと、現在のプレーヤーの再生 / 停止 / スキップは可。

</td>
<td width="50%" valign="top">

**Work**

計画、確認、実行。記憶、スキル、定期実行、MCP。範囲は付与された権限。

</td>
</tr>
</table>

気づいた事項を Work に渡す提案ができる。提案の受理は自律許可ではない。任意の TTS、聞き取り。音声 / リアルタイム会話を設定すれば長押しで連続会話。

ペルソナ、Bot ペアリング、ハートビートは `/agent/settings`。`/config` ではない。MCP サーバー一覧はまだ `/config` → 詳細設定。保存で `mcp_servers.json` を書き、子プロセスをホットリロードする。プライベートチャット Bot（QQ、Telegram、Discord、飛書）は同じ Work パイプラインに入る。ペアリングは新しいログイン方式ではない。[チャンネル](docs/development/AGENT_CHANNELS.md)。

---

## Phantasi

購読・ノート・コメントのバックエンド。RSS、Notion、RSSHub、任意の AI 強化。公開済みノートはインストール済みの任意の Hugo テーマで静的ブログになり、`/blog`（記事は `/blog/posts/{slug}/`）に置かれる。ページはスクリプトを実行せず、コメント、相互リンク申請、本文ウィジェットは同一オリジンの埋め込みページ。管理者は `/studio` で書く。購読は内蔵の「購読」リーダーアプリで読む。ノート RSS（`/api/phantasi/notes.xml`）は管理者が有効化するまで非公開。ノートは TeX 数式、共同著者、本文カラム、本文ウィジェットに対応。

---

## 連合

ActivityPub + **MFP**。発見は `BASE_URL` を使う。

フォロー：Actor URL（`https://your.domain/users/<name>`）または `@name@your.domain`。チャンネルとリングに対応。WebFinger、NodeInfo、inbox、連合メディアは **proxy → federation-worker**。SPA にも web プロセスにも入らない。

出口地理位置が制限地域なら **連合ゲート** が連合を止める（照会失敗は fail-open）。専用連合プロセスは終了する。web と persona は動き続ける。

注記：[連合](docs/development/FEDERATION.md)。連合ドメインの移転 ≠ 通常のドメイン変更。

---

## アカウントと公開範囲

- **ローカルアカウント：** オーナー作成後。登録のオン/オフ。OAuth 連携後、そのユーザーのパスワードログインを無効化できる。
- **OAuth：** 組み込み GitHub と任意の OIDC（Authentik、Keycloak、Google、Microsoft、GitLab、Discord、…）。身元は明示バインド。二つの issuer の同一メールはマージしない。
- **モジュール公開範囲：** ライブラリ、Phantasi、Tapp、Agent — 全員 / ログイン済み / 管理者。
- **検索と AI：** 非公開（noindex、空 sitemap、`/llms.txt` なし）、検索エンジンのみ、AI 引用、完全公開。
- **サイト識別：** 名前、紹介、アイコン、任意 PWA、壁紙とテーマ、第一者訪問統計、任意の Google Analytics / Umami。

---

## 運用

本番：**proxy + updater**。ホスト公開は **proxy** のみ。web、`federation-worker`、`persona-worker`、frontend、Postgres、updater、updater-gateway、docker-guard は内部ネット。proxy は persona / federation / 残りの API を振り分ける。[PORTS.md](docs/deployment/PORTS.md)。

版切り替えとロールバック：`/config` → 情報 → 更新管理。ブラウザは `UPDATE_TOKEN` を受け取らない。`:latest` を上書きしない。

同梱 Postgres：updater は `./pgdata` をスナップショット。ロールバックはデータディレクトリとイメージ tag を戻す。外部 Postgres：`MYRIAD_DB_MODE=external`。ロールバックはイメージ tag のみ。[外部 PostgreSQL](docs/deployment/EXTERNAL_POSTGRES.md)。

約 1 GiB ホスト向けメモリ節約。`/config` の診断は実検査（データベース、ストレージ、版、出口、連合ゲート）。報告に認証情報は含まない。

---

## 要件

**本番（Docker）**

- Docker + Compose
- ホストポート（`HTTP_PORT`、既定 80）
- linux/amd64 または linux/arm64

**ソースから**

- Rust 1.99+
- Node.js 24 LTS
- PostgreSQL 18（Compose 既定。下限は release `min_pg_version`）

---

## デプロイ

### Docker

```bash
git clone https://github.com/myriad-you/Myriad.git
cd Myriad

cp .env.production.example .env
# 必須: POSTGRES_PASSWORD / JWT_SECRET / CORS_ORIGINS
# BASE_URL / FRONTEND_URL = 公開オリジン（連合の発見は BASE_URL）
# UPDATE_TOKEN / UPDATER_GATEWAY_SECRET / PERSONA_DB_PASSWORD /
# FEDERATION_DB_PASSWORD 空欄 → deploy.sh が生成

bash scripts/extra/deploy.sh up
```

`http://localhost`（または `HTTP_PORT`）を開き、ウィザードでデータベース、オーナー、サイト名を完了する。

公式 Compose は `DATABASE_URL` を設定済みのため、起動には `MYRIAD_SETUP_SECRET` が必要（`deploy.sh up` が生成）。ウィザードでデータベースを手入力する場合、この暗号は使わない。[Setup インストール暗号](docs/deployment/SETUP_BOOTSTRAP.md)。

```text
host HTTP_PORT → proxy → frontend:1102
                      → backend:1103 → postgres:5432
                      → federation-worker:1103
                      → persona-worker:1103
                      → updater（内部; updater-gateway 経由）
```

```bash
bash scripts/extra/deploy.sh status
bash scripts/extra/deploy.sh logs
bash scripts/extra/deploy.sh restart
bash scripts/extra/deploy.sh down
```

更新：`/config` → 情報 → 更新管理。手動 tag：`.env` の `MYRIAD_TAG` / `PROXY_TAG` のあと `bash scripts/extra/deploy.sh upgrade`。

同梱 Postgres のバックアップ：

```bash
mkdir -p backups
docker compose exec -T postgres pg_dump -U myriad -d myriad > "backups/backup_$(date +%Y%m%d_%H%M%S).sql"
```

### 自分のパソコンで（Docker Desktop）

ドメインも TLS も不要。手順は上と同じで、`.env` は次のとおり。

- `BASE_URL` / `FRONTEND_URL`：`http://localhost` または空欄。テンプレートの `https://yourdomain.com` は残さない
- `CORS_ORIGINS`：`http://localhost`。LAN 内のほかの端末から開くなら、このパソコンの LAN アドレス（例 `http://192.168.1.20`）も加える。`HTTP_PORT` が 80 以外ならポートも付ける
- LAN 内で http のまま開いてもログインできる。連合と外部サービスでのログインには公開サイトのアドレスが必要。

macOS はリハーサル済み、Windows（WSL 2）は未検証。[ローカルデプロイ](docs/deployment/DOCKER_DEPLOYMENT.md#local-docker-desktop)

[クイックスタート](docs/QUICKSTART.md) · [Docker](docs/deployment/DOCKER_DEPLOYMENT.md) · [ポート](docs/deployment/PORTS.md) · [Docker なし](docs/deployment/NATIVE_DEPLOYMENT.md)

---

## ローカル開発

共有コアは git サブモジュール `core/`（[myriad-core](https://github.com/Myriad-You/myriad-core)）、彼女の 2.5D フィギュアは git サブモジュール `figure/`（[myriad-figure](https://github.com/Myriad-You/myriad-figure)）。クローン後と、プルでその参照が動いたとき：

```bash
git submodule update --init
```

```bash
./scripts/dev.sh                   # TUI：メニュー、プロセス、データベース、ログ
./scripts/dev.sh start             # 本機 PostgreSQL。ログはこの端末
./scripts/dev.sh start --docker    # Docker postgres + 新しい端末
./scripts/dev.sh doctor            # ツールチェーン、ポート、データベース
./scripts/dev.sh status            # スナップショット
.\scripts\dev.ps1 start            # Windows
```

バックエンド `:1103`、フロントエンド `:1102`。本機データベースがなければ `./scripts/dev.sh db-setup`。

フロントの dev server は `/api/*`、`/health`、`/ready`、連合の公開パスをバックエンドへプロキシする。

開発 UI の更新管理：`./scripts/dev.sh start all-updater`。イメージ差し替え、メンテナンスモード、`pgdata` スナップショットは本番スタック（`scripts/extra/deploy.sh`）。

---

## アーキテクチャ

```mermaid
flowchart LR
  Browser --> Proxy["proxy :HTTP_PORT"]
  Proxy --> FE["frontend :1102"]
  Proxy --> BE["web :1103"]
  Proxy --> FW["federation-worker :1103"]
  Proxy --> PW["persona-worker :1103"]
  BE --> PG[("postgres :5432")]
  FW --> PG
  PW --> PG
  BE --> GW[updater-gateway]
  GW --> UP[updater]
  UP --> Guard[docker-guard]
```

<details>
<summary>開発 / 本番トポロジ</summary>

**開発**

```text
browser → Vite dev (:1102)
            └─ /api/*, /health, /ready, federation public paths → backend (:1103) → postgres
```

**本番**

```text
host HTTP_PORT
  → proxy
       ├─► frontend (:1102)                 [myriad-net]
       ├─► backend (:1103) → postgres         MYRIAD_PROCESS_ROLE=web
       ├─► federation-worker (:1103)
       ├─► persona-worker (:1103)
       │
       └─► backend ─► updater-gateway → updater   [myriad-admin-net]
                                          └─► docker-guard → Docker sock
       (rescue) updater when PROXY_ALLOW_DIRECT_UPDATER=true
```

図は内蔵 PostgreSQL の構成で、両 worker も同じデータベースへ直接接続します。外部 DB コンテナを使う場合は、backend と両 worker を DB と同じ `MYRIAD_BACKEND_EXTRA_NETWORK`（既定 `myriad-backend-ext`）に接続します。Updater/Guard はこの 3 サービスに限り同ネットワークを許可します。旧版は固定の既定名しか認識しないため、両方を先にアップグレードしてからサイト内更新を利用してください。[外部 PostgreSQL](docs/deployment/EXTERNAL_POSTGRES.md) を参照。

</details>

クローラ / アプリ内シェア UA はホーム、ライブラリ、Phantasi、Tapp の SEO HTML シェルを受け取り、ブラウザは SPA を受け取る。[アーキテクチャ](docs/development/ARCHITECTURE.md)。

---

## リポジトリ

```
Myriad/
├── backend/          Rust API、SeaORM、migrations
├── frontend/         Vite + React UI、Tapp ランタイム、i18n（宿主 locale 7 種）
├── proxy/            本番リバースプロキシ（独立 Cargo ツリー）
├── updater/          自己更新デーモン（独立 Cargo ツリー）
├── core/             共有コア（git サブモジュール：myriad-core）
├── crates/           ワークスペースライブラリ
├── shared/           横断静的設定
├── docker/           backend / frontend Dockerfile
├── docs/             開発、デプロイ、機能（中国語）
├── scripts/          dev.sh / dev.ps1；extra/ デプロイ
├── release/          release.json 契約
├── tools/            tapp-cli、契約エクスポート
└── docker-compose*.yml
```

`proxy` と `updater` は独立 Cargo ツリー。

---

## 技術構成

<table>
<tr>
<td width="50%" valign="top">

**フロントエンド**  
[Vite](https://github.com/vitejs/vite) 8 · [React](https://github.com/facebook/react) 19 · [React Router](https://github.com/remix-run/react-router) 7<br>
[Tailwind](https://github.com/tailwindlabs/tailwindcss) 4 · [Vite](https://github.com/vitejs/vite) 8 · [TypeScript](https://github.com/microsoft/TypeScript) 6 · [Motion](https://github.com/motiondivision/motion) · [pnpm](https://github.com/pnpm/pnpm)  
`zh-CN` · `zh-TW` · `en-US` · `ja-JP` · `ko-KR` · `fr-FR` · `de-DE`

**ライブ顔**  
[Anime2.5DRig](https://github.com/852wa/Anime2.5DRig) · WebGL2

**音声**  
Agora RTC / RTM（任意）

</td>
<td width="50%" valign="top">

**バックエンド**  
[Rust](https://github.com/rust-lang/rust) 1.99 · [Axum](https://github.com/tokio-rs/axum) 0.8 · [Tokio](https://github.com/tokio-rs/tokio)  
[SeaORM](https://github.com/SeaQL/sea-orm) 2 / [SQLx](https://github.com/launchbadge/sqlx) · [reqwest](https://github.com/seanmonstar/reqwest) 0.13

**データ**  
[PostgreSQL](https://github.com/postgres/postgres) 18

**エッジ**  
proxy · updater

**拡張**  
Tapp サンドボックス · [MCP](https://github.com/modelcontextprotocol/modelcontextprotocol) · ActivityPub / MFP

**デプロイ**  
[Docker Compose](https://github.com/docker/compose) · linux/amd64 + arm64

</td>
</tr>
</table>

---

## ドキュメント

現在は中国語。[入口](docs/INDEX.md)。

<table>
<tr>
<td width="33%" valign="top">

**始める**  
[クイックスタート](docs/QUICKSTART.md)  
[アーキテクチャ](docs/development/ARCHITECTURE.md)  
[ビルド](docs/development/BUILD.md)  
[API](docs/API.md)

</td>
<td width="33%" valign="top">

**製品**  
[Tapp](docs/development/TAPP_DEVELOPMENT.md)  
[ライブラリ](docs/features/LIBRARY.md)  
[OAuth](docs/development/OAUTH.md)  
[連合](docs/development/FEDERATION.md)  
[Agent チャンネル](docs/development/AGENT_CHANNELS.md)

</td>
<td width="33%" valign="top">

**デプロイ**  
[Docker](docs/deployment/DOCKER_DEPLOYMENT.md)  
[ポート](docs/deployment/PORTS.md)  
[隔離](docs/deployment/RUNTIME_ISOLATION.md)  
[Worker DB](docs/deployment/WORKER_DATABASE.md)  
[バックアップ](docs/deployment/BACKUP.md)  
[Updater](docs/deployment/UPDATER_QUICKSTART.md)  
[外部 PG](docs/deployment/EXTERNAL_POSTGRES.md)  
[Docker なし](docs/deployment/NATIVE_DEPLOYMENT.md)  
[Setup インストール暗号](docs/deployment/SETUP_BOOTSTRAP.md)

</td>
</tr>
</table>

---

## スポンサー

Myriad はオープンに開発され、使ってくれる人たちに支えられています。役に立っていると感じたら、ぜひ応援をお願いします：

[![unifans](https://img.shields.io/badge/unifans-%E5%BF%9C%E6%8F%B4-FF5E8A?style=for-the-badge)](https://app.unifans.io/c/somekawahitomi)
[![Liberapay](https://img.shields.io/badge/Liberapay-%E5%AF%84%E4%BB%98-F6C915?style=for-the-badge&logo=liberapay&logoColor=black)](https://liberapay.com/furina/donate)
[![afdian](https://img.shields.io/badge/%E7%88%B1%E5%8F%91%E7%94%B5-Sponsor-946CE6?style=for-the-badge)](https://afdian.com/a/mamori)

支えてくださるすべての方に感謝します。

## 貢献

Issue と PR を歓迎。UI 文言：`zh-CN` / `zh-TW` / `en-US` / `ja-JP` / `ko-KR` / `fr-FR` / `de-DE`。

### コントリビューター

[![Contributors](https://contrib.rocks/image?repo=Myriad-You/Myriad)](https://github.com/Myriad-You/Myriad/graphs/contributors)

### AI エージェント

Myriad のコードの多くはコーディングエージェントと一緒に書かれています。それらのコミットには `Co-Authored-By` が付くため、GitHub のコントリビューター一覧にも表示されます。

[![Claude Code](https://img.shields.io/badge/Claude%20Code-D97757?style=for-the-badge&logo=claude&logoColor=white)](https://claude.com/claude-code)
[![Cursor](https://img.shields.io/badge/Cursor-000000?style=for-the-badge&logo=cursor&logoColor=white)](https://cursor.com)
[![GitHub Copilot](https://img.shields.io/badge/GitHub%20Copilot-000000?style=for-the-badge&logo=githubcopilot&logoColor=white)](https://github.com/features/copilot)

## License

ホスト（`backend` / `frontend` およびワークスペース crate）と `proxy` / `updater` は [AGPL-3.0](LICENSE) です。文書化された Bridge だけを通してホストと話す Tapp は独立した著作物であり、そのライセンスは作者が決めます（`LICENSE` の AGPL 第 7 条追加許可を参照）。

Tapp の契約とツール（`crates/tapp-contract`、`tools/tapp-cli`、`tools/tapp-contract-export`）は [Apache-2.0](LICENSES/Apache-2.0.txt) です。

2.5D 再生：[Anime2.5DRig](https://github.com/852wa/Anime2.5DRig)（MIT）。

<br/>

<div align="center">
<sub><i>あなたという物語を、ひとつに</i> · <i>A myriad of lights, in one place.</i></sub>
<br/>
<sub><a href="README.md">English</a> · <a href="README.zh-CN.md">中文</a> · <strong>日本語</strong></sub>
<br/>
<sub>Maintained by <a href="https://github.com/myriad-you">@myriad-you</a></sub>
</div>
