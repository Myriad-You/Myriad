use sea_orm_migration::prelude::*;

/// Phantasi 阅读系统数据库结构
///
/// RSS/Atom 订阅阅读器，支持：
/// - 订阅源管理
/// - 文章内容缓存（离线阅读）
/// - 用户阅读状态同步
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ==================== 1. PHANTASI_SOURCES 表 ====================
        // 存储 RSS/Atom 订阅源
        manager
            .create_table(
                Table::create()
                    .table(PhantasiSources::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PhantasiSources::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 所属用户
                    .col(ColumnDef::new(PhantasiSources::UserId).integer().not_null())
                    // 订阅源名称
                    .col(
                        ColumnDef::new(PhantasiSources::Name)
                            .string_len(255)
                            .not_null(),
                    )
                    // 订阅源 URL
                    .col(ColumnDef::new(PhantasiSources::Url).text().not_null())
                    // varchar(20) NOT NULL default rss；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(PhantasiSources::FeedType)
                            .string_len(20)
                            .not_null()
                            .default("rss"),
                    )
                    // varchar(20) NOT NULL default rss；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(PhantasiSources::SourceType)
                            .string_len(20)
                            .not_null()
                            .default("rss"),
                    )
                    // 分类标签
                    .col(ColumnDef::new(PhantasiSources::Category).string_len(100))
                    // 订阅源图标
                    .col(ColumnDef::new(PhantasiSources::Icon).text())
                    // 订阅源描述
                    .col(ColumnDef::new(PhantasiSources::Description).text())
                    // 订阅源网站链接
                    .col(ColumnDef::new(PhantasiSources::SiteUrl).text())
                    // 更新间隔（分钟），默认 30
                    .col(
                        ColumnDef::new(PhantasiSources::UpdateInterval)
                            .integer()
                            .not_null()
                            .default(30),
                    )
                    // 最后抓取时间
                    .col(ColumnDef::new(PhantasiSources::LastFetchedAt).timestamp_with_time_zone())
                    // 最后成功抓取时间
                    .col(ColumnDef::new(PhantasiSources::LastSuccessAt).timestamp_with_time_zone())
                    // 最后错误信息
                    .col(ColumnDef::new(PhantasiSources::LastError).text())
                    // 连续错误次数
                    .col(
                        ColumnDef::new(PhantasiSources::ErrorCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 是否启用
                    .col(
                        ColumnDef::new(PhantasiSources::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    // 文章总数缓存
                    .col(
                        ColumnDef::new(PhantasiSources::ItemCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // optional varchar(20)；本 migration 无 CHECK
                    .col(ColumnDef::new(PhantasiSources::CardSize).string_len(20))
                    // 主题颜色
                    .col(ColumnDef::new(PhantasiSources::ThemeColor).string_len(20))
                    // 自定义排序顺序
                    .col(ColumnDef::new(PhantasiSources::SortOrder).integer())
                    // AI 风格标签（JSON 数组）
                    .col(ColumnDef::new(PhantasiSources::AiStyleTags).json())
                    // 额外配置（如 Notion token 等）
                    .col(ColumnDef::new(PhantasiSources::ExtraConfig).json())
                    // RSSHub 路由路径（仅当 feed_type = rsshub 时使用）
                    .col(ColumnDef::new(PhantasiSources::RsshubRoute).text())
                    // 仅管理员可见
                    .col(
                        ColumnDef::new(PhantasiSources::AdminOnly)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 创建时间
                    .col(
                        ColumnDef::new(PhantasiSources::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // 更新时间
                    .col(
                        ColumnDef::new(PhantasiSources::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 索引：用户订阅源列表
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_sources_user_id")
                    .table(PhantasiSources::Table)
                    .col(PhantasiSources::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 唯一索引：同一用户不能订阅同一 URL 两次
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_sources_user_url")
                    .table(PhantasiSources::Table)
                    .col(PhantasiSources::UserId)
                    .col(PhantasiSources::Url)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 共享笔记目录全局只有一个。部分唯一索引不进 get_expected_indexes。
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_phantasi_sources_note_type \
                 ON phantasi_sources ((true)) WHERE source_type = 'note'",
            )
            .await?;

        // 索引：按分类查询
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_sources_category")
                    .table(PhantasiSources::Table)
                    .col(PhantasiSources::UserId)
                    .col(PhantasiSources::Category)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：调度器查询需要更新的订阅源
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_sources_schedule")
                    .table(PhantasiSources::Table)
                    .col(PhantasiSources::Enabled)
                    .col(PhantasiSources::LastFetchedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 2. PHANTASI_ITEMS 表 ====================
        // 存储订阅源的文章/内容
        manager
            .create_table(
                Table::create()
                    .table(PhantasiItems::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PhantasiItems::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 关联订阅源
                    .col(ColumnDef::new(PhantasiItems::SourceId).integer().not_null())
                    // varchar(512) NOT NULL
                    .col(
                        ColumnDef::new(PhantasiItems::Guid)
                            .string_len(512)
                            .not_null(),
                    )
                    // 文章标题
                    .col(ColumnDef::new(PhantasiItems::Title).text().not_null())
                    // 原文链接
                    .col(ColumnDef::new(PhantasiItems::Link).text().not_null())
                    // 文章摘要
                    .col(ColumnDef::new(PhantasiItems::Summary).text())
                    // 全文内容（用于离线阅读）
                    .col(ColumnDef::new(PhantasiItems::Content).text())
                    // 作者
                    .col(ColumnDef::new(PhantasiItems::Author).string_len(255))
                    // 封面图
                    .col(ColumnDef::new(PhantasiItems::Image).text())
                    // 音频链接（播客支持）
                    .col(ColumnDef::new(PhantasiItems::AudioUrl).text())
                    // 视频链接
                    .col(ColumnDef::new(PhantasiItems::VideoUrl).text())
                    // 附件信息 (JSON)
                    .col(ColumnDef::new(PhantasiItems::Enclosures).json())
                    // 文章分类/标签 (JSON Array)
                    .col(ColumnDef::new(PhantasiItems::Categories).json())
                    // 发布时间
                    .col(
                        ColumnDef::new(PhantasiItems::PublishedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // 抓取时间
                    .col(
                        ColumnDef::new(PhantasiItems::FetchedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // 字数统计
                    .col(ColumnDef::new(PhantasiItems::WordCount).integer())
                    // 预估阅读时间（分钟）
                    .col(ColumnDef::new(PhantasiItems::ReadingTime).integer())
                    // 是否已抓取全文
                    .col(
                        ColumnDef::new(PhantasiItems::FulltextFetched)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // nullable text；本 migration 无 CHECK
                    .col(ColumnDef::new(PhantasiItems::Topic).text())
                    // 笔记原文（Markdown）。只有 source_type = note 的源下的
                    // 条目有值；抓来的文章恒为 NULL。`content` 存的是渲染后的
                    // HTML，全站只认它 —— 阅读器、RSS、联邦、SEO 都读 content。
                    .col(ColumnDef::new(PhantasiItems::ContentMd).text())
                    // 正文版本。已有库由 schema_check 补列和触发器。
                    .col(
                        ColumnDef::new(PhantasiItems::ContentRevision)
                            .big_integer()
                            .not_null()
                            .default(1),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一索引：同一订阅源内 guid 唯一
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_items_source_guid")
                    .table(PhantasiItems::Table)
                    .col(PhantasiItems::SourceId)
                    .col(PhantasiItems::Guid)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(crate::SOURCE_RECENT_INDEX_SQL)
            .await?;

        // 索引：全局按时间排序（用于时间线视图）
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_items_timeline")
                    .table(PhantasiItems::Table)
                    .col(PhantasiItems::PublishedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：主题过滤。绝大多数行的 topic 是 NULL，做成部分索引。
        // 与 `ensure_phantasi_item_topic_index` 的 DDL 必须一字不差；不进
        // `get_expected_indexes`（通用路径没有 WHERE）。
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX IF NOT EXISTS idx_phantasi_items_topic ON phantasi_items (topic) WHERE topic IS NOT NULL",
            )
            .await?;

        // 外键约束
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_phantasi_items_source")
                    .from(PhantasiItems::Table, PhantasiItems::SourceId)
                    .to(PhantasiSources::Table, PhantasiSources::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // ==================== 3. PHANTASI_USER_STATES 表 ====================
        // 存储用户阅读状态（已读、收藏等）
        manager
            .create_table(
                Table::create()
                    .table(PhantasiUserStates::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PhantasiUserStates::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 用户 ID
                    .col(
                        ColumnDef::new(PhantasiUserStates::UserId)
                            .integer()
                            .not_null(),
                    )
                    // 文章 ID
                    .col(
                        ColumnDef::new(PhantasiUserStates::ItemId)
                            .integer()
                            .not_null(),
                    )
                    // 是否已读
                    .col(
                        ColumnDef::new(PhantasiUserStates::IsRead)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 是否收藏
                    .col(
                        ColumnDef::new(PhantasiUserStates::IsStarred)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 阅读时间
                    .col(ColumnDef::new(PhantasiUserStates::ReadAt).timestamp_with_time_zone())
                    // 阅读进度 float；本 migration 无 CHECK
                    .col(ColumnDef::new(PhantasiUserStates::ReadProgress).float())
                    // 收藏时间
                    .col(ColumnDef::new(PhantasiUserStates::StarredAt).timestamp_with_time_zone())
                    // 用户笔记
                    .col(ColumnDef::new(PhantasiUserStates::Notes).text())
                    // 阅读状态版本。已有库由 schema_check 补列和触发器。
                    .col(
                        ColumnDef::new(PhantasiUserStates::Revision)
                            .big_integer()
                            .not_null()
                            .default(1),
                    )
                    // 更新时间
                    .col(
                        ColumnDef::new(PhantasiUserStates::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一索引：每个用户对每篇文章只有一条状态记录
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_user_states_unique")
                    .table(PhantasiUserStates::Table)
                    .col(PhantasiUserStates::UserId)
                    .col(PhantasiUserStates::ItemId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：查询用户未读文章
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_user_states_unread")
                    .table(PhantasiUserStates::Table)
                    .col(PhantasiUserStates::UserId)
                    .col(PhantasiUserStates::IsRead)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：查询用户收藏文章
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_user_states_starred")
                    .table(PhantasiUserStates::Table)
                    .col(PhantasiUserStates::UserId)
                    .col(PhantasiUserStates::IsStarred)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 外键约束
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_phantasi_user_states_item")
                    .from(PhantasiUserStates::Table, PhantasiUserStates::ItemId)
                    .to(PhantasiItems::Table, PhantasiItems::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // ==================== 4. PHANTASI_CATEGORIES 表 ====================
        // 存储用户自定义分类
        manager
            .create_table(
                Table::create()
                    .table(PhantasiCategories::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PhantasiCategories::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 用户 ID
                    .col(
                        ColumnDef::new(PhantasiCategories::UserId)
                            .integer()
                            .not_null(),
                    )
                    // 分类名称
                    .col(
                        ColumnDef::new(PhantasiCategories::Name)
                            .string_len(100)
                            .not_null(),
                    )
                    // 分类图标
                    .col(ColumnDef::new(PhantasiCategories::Icon).string_len(50))
                    // 分类颜色
                    .col(ColumnDef::new(PhantasiCategories::Color).string_len(20))
                    // 排序顺序
                    .col(
                        ColumnDef::new(PhantasiCategories::SortOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 创建时间
                    .col(
                        ColumnDef::new(PhantasiCategories::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一索引：同一用户分类名唯一
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_categories_unique")
                    .table(PhantasiCategories::Table)
                    .col(PhantasiCategories::UserId)
                    .col(PhantasiCategories::Name)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 5. PHANTASI_ANNOTATIONS 表 ====================
        // 存储 Phantasiai AI 阅读辅助注释
        manager
            .create_table(
                Table::create()
                    .table(PhantasiAnnotations::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PhantasiAnnotations::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 关联文章 ID
                    .col(
                        ColumnDef::new(PhantasiAnnotations::ItemId)
                            .integer()
                            .not_null(),
                    )
                    // varchar(20) NOT NULL default term；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(PhantasiAnnotations::AnnotationType)
                            .string_len(20)
                            .not_null()
                            .default("term"),
                    )
                    // 原词/短语
                    .col(ColumnDef::new(PhantasiAnnotations::Term).text().not_null())
                    // 注释说明
                    .col(
                        ColumnDef::new(PhantasiAnnotations::Explanation)
                            .text()
                            .not_null(),
                    )
                    // 位置（字符偏移，可选）
                    .col(ColumnDef::new(PhantasiAnnotations::Position).integer())
                    // 上下文提示（用于指代类注释）
                    .col(ColumnDef::new(PhantasiAnnotations::ContextHint).text())
                    // 创建时间
                    .col(
                        ColumnDef::new(PhantasiAnnotations::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 索引：按文章 ID 查询注释
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_annotations_item")
                    .table(PhantasiAnnotations::Table)
                    .col(PhantasiAnnotations::ItemId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 外键约束：删除文章时级联删除注释
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_phantasi_annotations_item")
                    .from(PhantasiAnnotations::Table, PhantasiAnnotations::ItemId)
                    .to(PhantasiItems::Table, PhantasiItems::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // ==================== 6. PHANTASI_PODCASTS 表 ====================
        // 存储 Phantasiai AI 播客脚本
        manager
            .create_table(
                Table::create()
                    .table(PhantasiPodcasts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PhantasiPodcasts::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 关联文章 ID（唯一，每篇文章只有一个播客）
                    .col(
                        ColumnDef::new(PhantasiPodcasts::ItemId)
                            .integer()
                            .not_null(),
                    )
                    // 播客标题
                    .col(ColumnDef::new(PhantasiPodcasts::Title).text().not_null())
                    // 检测到的语言
                    .col(ColumnDef::new(PhantasiPodcasts::Language).string_len(20))
                    // 对话列表 (JSON)
                    .col(
                        ColumnDef::new(PhantasiPodcasts::Dialogues)
                            .json_binary()
                            .not_null(),
                    )
                    // 预计时长（秒）
                    .col(ColumnDef::new(PhantasiPodcasts::EstimatedDuration).integer())
                    // 创建时间
                    .col(
                        ColumnDef::new(PhantasiPodcasts::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一索引：每篇文章只有一个播客
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_podcasts_item")
                    .table(PhantasiPodcasts::Table)
                    .col(PhantasiPodcasts::ItemId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 外键约束：删除文章时级联删除播客
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_phantasi_podcasts_item")
                    .from(PhantasiPodcasts::Table, PhantasiPodcasts::ItemId)
                    .to(PhantasiItems::Table, PhantasiItems::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // ==================== 7. PHANTASI_COMMENTS 表 ====================
        // 存储用户对文章的选中文本评论（类似批注）
        manager
            .create_table(
                Table::create()
                    .table(PhantasiComments::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PhantasiComments::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 关联文章 ID
                    .col(
                        ColumnDef::new(PhantasiComments::ItemId)
                            .integer()
                            .not_null(),
                    )
                    // 评论用户 ID
                    .col(
                        ColumnDef::new(PhantasiComments::UserId)
                            .integer()
                            .not_null(),
                    )
                    // 选中的原文文本
                    .col(
                        ColumnDef::new(PhantasiComments::SelectedText)
                            .text()
                            .not_null(),
                    )
                    // 评论内容
                    .col(ColumnDef::new(PhantasiComments::Comment).text().not_null())
                    // 选中文本在原文中的起始位置（字符偏移）
                    .col(ColumnDef::new(PhantasiComments::StartOffset).integer())
                    // 选中文本在原文中的结束位置（字符偏移）
                    .col(ColumnDef::new(PhantasiComments::EndOffset).integer())
                    // 上下文文本（用于定位）
                    .col(ColumnDef::new(PhantasiComments::ContextBefore).text())
                    .col(ColumnDef::new(PhantasiComments::ContextAfter).text())
                    // 评论颜色标记
                    .col(ColumnDef::new(PhantasiComments::Color).string_len(20))
                    // 是否公开，默认 false
                    .col(
                        ColumnDef::new(PhantasiComments::IsPublic)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 父评论 ID（用于嵌套回复，NULL 表示顶级评论）
                    .col(ColumnDef::new(PhantasiComments::ParentId).integer())
                    // 创建时的正文版本（可空）
                    .col(ColumnDef::new(PhantasiComments::ContentRevision).big_integer())
                    // 创建时间
                    .col(
                        ColumnDef::new(PhantasiComments::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // 更新时间
                    .col(
                        ColumnDef::new(PhantasiComments::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 索引：按文章查询评论
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_comments_item")
                    .table(PhantasiComments::Table)
                    .col(PhantasiComments::ItemId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：按用户查询评论
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_comments_user")
                    .table(PhantasiComments::Table)
                    .col(PhantasiComments::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 复合索引：用户在特定文章的评论
        manager
            .create_index(
                Index::create()
                    .name("idx_phantasi_comments_item_user")
                    .table(PhantasiComments::Table)
                    .col(PhantasiComments::ItemId)
                    .col(PhantasiComments::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 外键约束：删除文章时级联删除评论
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_phantasi_comments_item")
                    .from(PhantasiComments::Table, PhantasiComments::ItemId)
                    .to(PhantasiItems::Table, PhantasiItems::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // ==================== 8. RSSHUB_INSTANCES 表 ====================
        // 存储用户的 RSSHub 实例配置，支持健康检查和自动故障转移
        manager
            .create_table(
                Table::create()
                    .table(RsshubInstances::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(RsshubInstances::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 所属用户（NULL 表示全局默认实例）
                    .col(ColumnDef::new(RsshubInstances::UserId).integer())
                    // 实例名称
                    .col(
                        ColumnDef::new(RsshubInstances::Name)
                            .string_len(100)
                            .not_null(),
                    )
                    // 实例 URL（如 https://rsshub.app）
                    .col(ColumnDef::new(RsshubInstances::Url).text().not_null())
                    // 访问密钥（可选）
                    .col(ColumnDef::new(RsshubInstances::AccessKey).string_len(255))
                    // 优先级（数字越小优先级越高）
                    .col(
                        ColumnDef::new(RsshubInstances::Priority)
                            .integer()
                            .not_null()
                            .default(100),
                    )
                    // 是否启用
                    .col(
                        ColumnDef::new(RsshubInstances::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    // varchar(20) NOT NULL default unknown；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(RsshubInstances::HealthStatus)
                            .string_len(20)
                            .not_null()
                            .default("unknown"),
                    )
                    // 最后健康检查时间
                    .col(
                        ColumnDef::new(RsshubInstances::LastHealthCheck).timestamp_with_time_zone(),
                    )
                    // 最后响应时间（毫秒）
                    .col(ColumnDef::new(RsshubInstances::LastResponseTimeMs).integer())
                    // 连续失败次数
                    .col(
                        ColumnDef::new(RsshubInstances::ConsecutiveFailures)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 总请求次数
                    .col(
                        ColumnDef::new(RsshubInstances::TotalRequests)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 成功请求次数
                    .col(
                        ColumnDef::new(RsshubInstances::SuccessRequests)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 创建时间
                    .col(
                        ColumnDef::new(RsshubInstances::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // 更新时间
                    .col(
                        ColumnDef::new(RsshubInstances::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一索引：同一用户不能添加同一实例 URL 两次
        manager
            .create_index(
                Index::create()
                    .name("idx_rsshub_instances_user_url")
                    .table(RsshubInstances::Table)
                    .col(RsshubInstances::UserId)
                    .col(RsshubInstances::Url)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // NULL user_id 在普通 UNIQUE 里不互斥；全局实例按 URL 单独约束。
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_rsshub_instances_global_url \
                 ON rsshub_instances (url) WHERE user_id IS NULL",
            )
            .await?;

        // 索引：`(user_id, enabled, priority)`
        manager
            .create_index(
                Index::create()
                    .name("idx_rsshub_instances_priority")
                    .table(RsshubInstances::Table)
                    .col(RsshubInstances::UserId)
                    .col(RsshubInstances::Enabled)
                    .col(RsshubInstances::Priority)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 9. PHANTASI_NOTE_DOCS 表 ====================
        // 云端笔记文档。草稿 / 定时只活在这里，发布后才有 phantasi_items。
        // 与 `ensure_phantasi_note_docs_table` / TableDef 同一段 DDL。
        // 友联 / 订阅申请与 `ensure_phantasi_source_applications_table` 同一段。
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE TABLE IF NOT EXISTS phantasi_note_docs (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL,
    last_edited_by INTEGER,
    item_id INTEGER,
    title TEXT NOT NULL DEFAULT '',
    content_md TEXT NOT NULL DEFAULT '',
    topic TEXT,
    image TEXT,
    status VARCHAR NOT NULL DEFAULT 'draft',
    scheduled_at TIMESTAMPTZ,
    published_at TIMESTAMPTZ,
    revision BIGINT NOT NULL DEFAULT 1,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_phantasi_note_docs_user
    ON phantasi_note_docs (user_id);
CREATE INDEX IF NOT EXISTS idx_phantasi_note_docs_item
    ON phantasi_note_docs (item_id);
CREATE INDEX IF NOT EXISTS idx_phantasi_note_docs_schedule
    ON phantasi_note_docs (status, scheduled_at);
CREATE TABLE IF NOT EXISTS phantasi_note_authors (
    doc_id INTEGER NOT NULL REFERENCES phantasi_note_docs(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL,
    role VARCHAR NOT NULL DEFAULT 'author',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (doc_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_phantasi_note_authors_user
    ON phantasi_note_authors (user_id);
INSERT INTO phantasi_note_authors (doc_id, user_id, role)
SELECT id, user_id, 'owner' FROM phantasi_note_docs
ON CONFLICT (doc_id, user_id) DO NOTHING;
CREATE TABLE IF NOT EXISTS phantasi_source_applications (
    id SERIAL PRIMARY KEY,
    kind VARCHAR NOT NULL DEFAULT 'friend',
    status VARCHAR NOT NULL DEFAULT 'pending',
    site_name TEXT NOT NULL,
    site_url TEXT NOT NULL,
    feed_url TEXT,
    description TEXT,
    message TEXT,
    applicant_name TEXT,
    applicant_email TEXT,
    applicant_user_id INTEGER,
    applicant_ip TEXT,
    result_source_id INTEGER,
    review_note TEXT,
    reviewed_by INTEGER,
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_phantasi_source_applications_status
    ON phantasi_source_applications (status, created_at);
CREATE INDEX IF NOT EXISTS idx_phantasi_source_applications_site_url
    ON phantasi_source_applications (site_url);
"#,
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE UNIQUE INDEX IF NOT EXISTS idx_phantasi_source_applications_pending_site
    ON phantasi_source_applications (regexp_replace(site_url, '/+$', ''))
    WHERE status = 'pending';
CREATE UNIQUE INDEX IF NOT EXISTS idx_phantasi_source_applications_pending_feed
    ON phantasi_source_applications (regexp_replace(feed_url, '/+$', ''))
    WHERE status = 'pending' AND feed_url IS NOT NULL AND btrim(feed_url) <> '';
"#,
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(include_str!("note_editor.sql"))
            .await?;

        manager
            .get_connection()
            .execute_unprepared(include_str!("media_asset_model.sql"))
            .await?;

        // 与 schema_check::ensure_phantasi_state_revision /
        // ensure_phantasi_content_revision 同一段 DDL。
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE OR REPLACE FUNCTION phantasi_advance_state_revision() RETURNS trigger AS $$
BEGIN
    NEW.revision := OLD.revision + 1;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS phantasi_state_revision ON phantasi_user_states;
CREATE TRIGGER phantasi_state_revision BEFORE UPDATE ON phantasi_user_states FOR EACH ROW EXECUTE FUNCTION phantasi_advance_state_revision();

CREATE OR REPLACE FUNCTION phantasi_advance_content_revision() RETURNS trigger AS $$
BEGIN
    IF NEW.content IS DISTINCT FROM OLD.content
        OR NEW.content_md IS DISTINCT FROM OLD.content_md THEN
        NEW.content_revision := OLD.content_revision + 1;
    ELSE
        NEW.content_revision := OLD.content_revision;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS phantasi_content_revision ON phantasi_items;
CREATE TRIGGER phantasi_content_revision BEFORE UPDATE ON phantasi_items FOR EACH ROW EXECUTE FUNCTION phantasi_advance_content_revision();
"#,
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "DROP TRIGGER IF EXISTS phantasi_content_revision ON phantasi_items; \
             DROP TRIGGER IF EXISTS phantasi_state_revision ON phantasi_user_states; \
             DROP FUNCTION IF EXISTS phantasi_advance_content_revision(); \
             DROP FUNCTION IF EXISTS phantasi_advance_state_revision();",
            )
            .await?;
        manager.get_connection().execute_unprepared("DROP TABLE IF EXISTS phantasi_note_history; DROP FUNCTION IF EXISTS phantasi_capture_note_history() CASCADE;").await?;
        manager
            .get_connection()
            .execute_unprepared(
                "DROP TABLE IF EXISTS phantasi_source_applications; DROP TABLE IF EXISTS media_migration_jobs; DROP TABLE IF EXISTS media_url_aliases; DROP TABLE IF EXISTS media_references; DROP TABLE IF EXISTS media_assets; DROP TABLE IF EXISTS phantasi_note_authors; DROP TABLE IF EXISTS phantasi_note_docs",
            )
            .await?;

        // 删除 rsshub_instances 表
        manager
            .drop_table(Table::drop().table(RsshubInstances::Table).to_owned())
            .await?;

        // 删除外键
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(PhantasiComments::Table)
                    .name("fk_phantasi_comments_item")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(PhantasiPodcasts::Table)
                    .name("fk_phantasi_podcasts_item")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(PhantasiAnnotations::Table)
                    .name("fk_phantasi_annotations_item")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(PhantasiUserStates::Table)
                    .name("fk_phantasi_user_states_item")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(PhantasiItems::Table)
                    .name("fk_phantasi_items_source")
                    .to_owned(),
            )
            .await?;

        // 删除表
        manager
            .drop_table(Table::drop().table(PhantasiComments::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PhantasiPodcasts::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PhantasiAnnotations::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PhantasiCategories::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PhantasiUserStates::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PhantasiItems::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PhantasiSources::Table).to_owned())
            .await?;

        Ok(())
    }
}

// ==================== 表定义枚举 ====================

#[derive(DeriveIden)]
enum PhantasiSources {
    Table,
    Id,
    UserId,
    Name,
    Url,
    FeedType,
    SourceType,
    Category,
    Icon,
    Description,
    SiteUrl,
    UpdateInterval,
    LastFetchedAt,
    LastSuccessAt,
    LastError,
    ErrorCount,
    Enabled,
    ItemCount,
    CardSize,
    ThemeColor,
    SortOrder,
    AiStyleTags,
    ExtraConfig,
    RsshubRoute,
    AdminOnly,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum PhantasiItems {
    Table,
    Id,
    SourceId,
    Guid,
    Title,
    Link,
    Summary,
    Content,
    Author,
    Image,
    AudioUrl,
    VideoUrl,
    Enclosures,
    Categories,
    PublishedAt,
    FetchedAt,
    WordCount,
    ReadingTime,
    FulltextFetched,
    Topic,
    ContentMd,
    ContentRevision,
}

#[derive(DeriveIden)]
enum PhantasiUserStates {
    Table,
    Id,
    UserId,
    ItemId,
    IsRead,
    IsStarred,
    ReadAt,
    ReadProgress,
    StarredAt,
    Notes,
    Revision,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum PhantasiCategories {
    Table,
    Id,
    UserId,
    Name,
    Icon,
    Color,
    SortOrder,
    CreatedAt,
}

#[derive(DeriveIden)]
enum PhantasiAnnotations {
    Table,
    Id,
    ItemId,
    AnnotationType,
    Term,
    Explanation,
    Position,
    ContextHint,
    CreatedAt,
}

#[derive(DeriveIden)]
enum PhantasiPodcasts {
    Table,
    Id,
    ItemId,
    Title,
    Language,
    Dialogues,
    EstimatedDuration,
    CreatedAt,
}

#[derive(DeriveIden)]
enum PhantasiComments {
    Table,
    Id,
    ItemId,
    UserId,
    SelectedText,
    Comment,
    StartOffset,
    EndOffset,
    ContextBefore,
    ContextAfter,
    Color,
    IsPublic,
    ParentId,
    ContentRevision,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum RsshubInstances {
    Table,
    Id,
    UserId,
    Name,
    Url,
    AccessKey,
    Priority,
    Enabled,
    HealthStatus,
    LastHealthCheck,
    LastResponseTimeMs,
    ConsecutiveFailures,
    TotalRequests,
    SuccessRequests,
    CreatedAt,
    UpdatedAt,
}
