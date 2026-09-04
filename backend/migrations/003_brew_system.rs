use sea_orm_migration::prelude::*;

/// Brew 阅读系统数据库结构
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
        // ==================== 1. BREW_SOURCES 表 ====================
        // 存储 RSS/Atom 订阅源
        manager
            .create_table(
                Table::create()
                    .table(BrewSources::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BrewSources::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 所属用户
                    .col(ColumnDef::new(BrewSources::UserId).integer().not_null())
                    // 订阅源名称
                    .col(ColumnDef::new(BrewSources::Name).string_len(255).not_null())
                    // 订阅源 URL
                    .col(ColumnDef::new(BrewSources::Url).text().not_null())
                    // 订阅源类型: rss, atom, json_feed
                    .col(
                        ColumnDef::new(BrewSources::FeedType)
                            .string_len(20)
                            .not_null()
                            .default("rss"),
                    )
                    // 来源类型: link(纯链接), rss(RSS订阅), brewlia(AI增强订阅)
                    .col(
                        ColumnDef::new(BrewSources::SourceType)
                            .string_len(20)
                            .not_null()
                            .default("rss"),
                    )
                    // 分类标签
                    .col(ColumnDef::new(BrewSources::Category).string_len(100))
                    // 订阅源图标
                    .col(ColumnDef::new(BrewSources::Icon).text())
                    // 订阅源描述
                    .col(ColumnDef::new(BrewSources::Description).text())
                    // 订阅源网站链接
                    .col(ColumnDef::new(BrewSources::SiteUrl).text())
                    // 更新间隔（分钟），默认 30
                    .col(
                        ColumnDef::new(BrewSources::UpdateInterval)
                            .integer()
                            .not_null()
                            .default(30),
                    )
                    // 最后抓取时间
                    .col(ColumnDef::new(BrewSources::LastFetchedAt).timestamp_with_time_zone())
                    // 最后成功抓取时间
                    .col(ColumnDef::new(BrewSources::LastSuccessAt).timestamp_with_time_zone())
                    // 最后错误信息
                    .col(ColumnDef::new(BrewSources::LastError).text())
                    // 连续错误次数
                    .col(
                        ColumnDef::new(BrewSources::ErrorCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 是否启用
                    .col(
                        ColumnDef::new(BrewSources::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    // 文章总数缓存
                    .col(
                        ColumnDef::new(BrewSources::ItemCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 未读数缓存
                    .col(
                        ColumnDef::new(BrewSources::UnreadCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 卡片显示尺寸: full, mini
                    .col(ColumnDef::new(BrewSources::CardSize).string_len(20))
                    // 主题颜色
                    .col(ColumnDef::new(BrewSources::ThemeColor).string_len(20))
                    // 自定义排序顺序
                    .col(ColumnDef::new(BrewSources::SortOrder).integer())
                    // AI 风格标签（JSON 数组）
                    .col(ColumnDef::new(BrewSources::AiStyleTags).json())
                    // 额外配置（如 Notion token 等）
                    .col(ColumnDef::new(BrewSources::ExtraConfig).json())
                    // RSSHub 路由路径（仅当 feed_type = rsshub 时使用）
                    .col(ColumnDef::new(BrewSources::RsshubRoute).text())
                    // 仅管理员可见
                    .col(
                        ColumnDef::new(BrewSources::AdminOnly)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 创建时间
                    .col(
                        ColumnDef::new(BrewSources::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // 更新时间
                    .col(
                        ColumnDef::new(BrewSources::UpdatedAt)
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
                    .name("idx_brew_sources_user_id")
                    .table(BrewSources::Table)
                    .col(BrewSources::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 唯一索引：同一用户不能订阅同一 URL 两次
        manager
            .create_index(
                Index::create()
                    .name("idx_brew_sources_user_url")
                    .table(BrewSources::Table)
                    .col(BrewSources::UserId)
                    .col(BrewSources::Url)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：按分类查询
        manager
            .create_index(
                Index::create()
                    .name("idx_brew_sources_category")
                    .table(BrewSources::Table)
                    .col(BrewSources::UserId)
                    .col(BrewSources::Category)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：调度器查询需要更新的订阅源
        manager
            .create_index(
                Index::create()
                    .name("idx_brew_sources_schedule")
                    .table(BrewSources::Table)
                    .col(BrewSources::Enabled)
                    .col(BrewSources::LastFetchedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 2. BREW_ITEMS 表 ====================
        // 存储订阅源的文章/内容
        manager
            .create_table(
                Table::create()
                    .table(BrewItems::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BrewItems::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 关联订阅源
                    .col(ColumnDef::new(BrewItems::SourceId).integer().not_null())
                    // RSS guid / Atom id（唯一标识）
                    .col(ColumnDef::new(BrewItems::Guid).string_len(512).not_null())
                    // 文章标题
                    .col(ColumnDef::new(BrewItems::Title).text().not_null())
                    // 原文链接
                    .col(ColumnDef::new(BrewItems::Link).text().not_null())
                    // 文章摘要
                    .col(ColumnDef::new(BrewItems::Summary).text())
                    // 全文内容（用于离线阅读）
                    .col(ColumnDef::new(BrewItems::Content).text())
                    // 作者
                    .col(ColumnDef::new(BrewItems::Author).string_len(255))
                    // 封面图
                    .col(ColumnDef::new(BrewItems::Image).text())
                    // 音频链接（播客支持）
                    .col(ColumnDef::new(BrewItems::AudioUrl).text())
                    // 视频链接
                    .col(ColumnDef::new(BrewItems::VideoUrl).text())
                    // 附件信息 (JSON)
                    .col(ColumnDef::new(BrewItems::Enclosures).json())
                    // 文章分类/标签 (JSON Array)
                    .col(ColumnDef::new(BrewItems::Categories).json())
                    // 发布时间
                    .col(
                        ColumnDef::new(BrewItems::PublishedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // 抓取时间
                    .col(
                        ColumnDef::new(BrewItems::FetchedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // 字数统计
                    .col(ColumnDef::new(BrewItems::WordCount).integer())
                    // 预估阅读时间（分钟）
                    .col(ColumnDef::new(BrewItems::ReadingTime).integer())
                    // 是否已抓取全文
                    .col(
                        ColumnDef::new(BrewItems::FulltextFetched)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 预定义主题 key（关键词或 AI 离线写入）。NULL = 未分类，
                    // 聚类侧靠 NULL 把文章留在源磁贴里，不建「其他」桶。
                    .col(ColumnDef::new(BrewItems::Topic).text())
                    // 手记原文（Markdown）。只有 source_type = note 的源下的
                    // 条目有值；抓来的文章恒为 NULL。`content` 存的是渲染后的
                    // HTML，全站只认它 —— 阅读器、RSS、联邦、SEO 都读 content。
                    .col(ColumnDef::new(BrewItems::ContentMd).text())
                    .to_owned(),
            )
            .await?;

        // 唯一索引：同一订阅源内 guid 唯一
        manager
            .create_index(
                Index::create()
                    .name("idx_brew_items_source_guid")
                    .table(BrewItems::Table)
                    .col(BrewItems::SourceId)
                    .col(BrewItems::Guid)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：按发布时间排序
        manager
            .create_index(
                Index::create()
                    .name("idx_brew_items_published")
                    .table(BrewItems::Table)
                    .col(BrewItems::SourceId)
                    .col(BrewItems::PublishedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：全局按时间排序（用于时间线视图）
        manager
            .create_index(
                Index::create()
                    .name("idx_brew_items_timeline")
                    .table(BrewItems::Table)
                    .col(BrewItems::PublishedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：主题过滤。绝大多数行的 topic 是 NULL，做成部分索引。
        // 与 `schema_check::ensure_brew_item_topic_index` 的 DDL 必须一字不差 ——
        // 通用索引路径不支持 WHERE 子句，两边形状不一致就会一直报漂移。
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX IF NOT EXISTS idx_brew_items_topic                  ON brew_items (topic) WHERE topic IS NOT NULL",
            )
            .await?;

        // 外键约束
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_brew_items_source")
                    .from(BrewItems::Table, BrewItems::SourceId)
                    .to(BrewSources::Table, BrewSources::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // ==================== 3. BREW_USER_STATES 表 ====================
        // 存储用户阅读状态（已读、收藏等）
        manager
            .create_table(
                Table::create()
                    .table(BrewUserStates::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BrewUserStates::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 用户 ID
                    .col(ColumnDef::new(BrewUserStates::UserId).integer().not_null())
                    // 文章 ID
                    .col(ColumnDef::new(BrewUserStates::ItemId).integer().not_null())
                    // 是否已读
                    .col(
                        ColumnDef::new(BrewUserStates::IsRead)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 是否收藏
                    .col(
                        ColumnDef::new(BrewUserStates::IsStarred)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 阅读时间
                    .col(ColumnDef::new(BrewUserStates::ReadAt).timestamp_with_time_zone())
                    // 阅读进度 (0.0 - 1.0)
                    .col(ColumnDef::new(BrewUserStates::ReadProgress).float())
                    // 收藏时间
                    .col(ColumnDef::new(BrewUserStates::StarredAt).timestamp_with_time_zone())
                    // 用户笔记
                    .col(ColumnDef::new(BrewUserStates::Notes).text())
                    // 更新时间
                    .col(
                        ColumnDef::new(BrewUserStates::UpdatedAt)
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
                    .name("idx_brew_user_states_unique")
                    .table(BrewUserStates::Table)
                    .col(BrewUserStates::UserId)
                    .col(BrewUserStates::ItemId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：查询用户未读文章
        manager
            .create_index(
                Index::create()
                    .name("idx_brew_user_states_unread")
                    .table(BrewUserStates::Table)
                    .col(BrewUserStates::UserId)
                    .col(BrewUserStates::IsRead)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：查询用户收藏文章
        manager
            .create_index(
                Index::create()
                    .name("idx_brew_user_states_starred")
                    .table(BrewUserStates::Table)
                    .col(BrewUserStates::UserId)
                    .col(BrewUserStates::IsStarred)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 外键约束
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_brew_user_states_item")
                    .from(BrewUserStates::Table, BrewUserStates::ItemId)
                    .to(BrewItems::Table, BrewItems::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // ==================== 4. BREW_CATEGORIES 表 ====================
        // 存储用户自定义分类
        manager
            .create_table(
                Table::create()
                    .table(BrewCategories::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BrewCategories::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 用户 ID
                    .col(ColumnDef::new(BrewCategories::UserId).integer().not_null())
                    // 分类名称
                    .col(
                        ColumnDef::new(BrewCategories::Name)
                            .string_len(100)
                            .not_null(),
                    )
                    // 分类图标
                    .col(ColumnDef::new(BrewCategories::Icon).string_len(50))
                    // 分类颜色
                    .col(ColumnDef::new(BrewCategories::Color).string_len(20))
                    // 排序顺序
                    .col(
                        ColumnDef::new(BrewCategories::SortOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 创建时间
                    .col(
                        ColumnDef::new(BrewCategories::CreatedAt)
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
                    .name("idx_brew_categories_unique")
                    .table(BrewCategories::Table)
                    .col(BrewCategories::UserId)
                    .col(BrewCategories::Name)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 5. BREW_ANNOTATIONS 表 ====================
        // 存储 Brewlia AI 阅读辅助注释
        manager
            .create_table(
                Table::create()
                    .table(BrewAnnotations::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BrewAnnotations::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 关联文章 ID
                    .col(ColumnDef::new(BrewAnnotations::ItemId).integer().not_null())
                    // 注释类型: term, reference, implicit, context, abbreviation
                    .col(
                        ColumnDef::new(BrewAnnotations::AnnotationType)
                            .string_len(20)
                            .not_null()
                            .default("term"),
                    )
                    // 原词/短语
                    .col(ColumnDef::new(BrewAnnotations::Term).text().not_null())
                    // 注释说明
                    .col(
                        ColumnDef::new(BrewAnnotations::Explanation)
                            .text()
                            .not_null(),
                    )
                    // 位置（字符偏移，可选）
                    .col(ColumnDef::new(BrewAnnotations::Position).integer())
                    // 上下文提示（用于指代类注释）
                    .col(ColumnDef::new(BrewAnnotations::ContextHint).text())
                    // 创建时间
                    .col(
                        ColumnDef::new(BrewAnnotations::CreatedAt)
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
                    .name("idx_brew_annotations_item")
                    .table(BrewAnnotations::Table)
                    .col(BrewAnnotations::ItemId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 外键约束：删除文章时级联删除注释
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_brew_annotations_item")
                    .from(BrewAnnotations::Table, BrewAnnotations::ItemId)
                    .to(BrewItems::Table, BrewItems::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // ==================== 6. BREW_PODCASTS 表 ====================
        // 存储 Brewlia AI 播客脚本
        manager
            .create_table(
                Table::create()
                    .table(BrewPodcasts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BrewPodcasts::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 关联文章 ID（唯一，每篇文章只有一个播客）
                    .col(ColumnDef::new(BrewPodcasts::ItemId).integer().not_null())
                    // 播客标题
                    .col(ColumnDef::new(BrewPodcasts::Title).text().not_null())
                    // 检测到的语言
                    .col(ColumnDef::new(BrewPodcasts::Language).string_len(20))
                    // 对话列表 (JSON)
                    .col(
                        ColumnDef::new(BrewPodcasts::Dialogues)
                            .json_binary()
                            .not_null(),
                    )
                    // 预计时长（秒）
                    .col(ColumnDef::new(BrewPodcasts::EstimatedDuration).integer())
                    // 创建时间
                    .col(
                        ColumnDef::new(BrewPodcasts::CreatedAt)
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
                    .name("idx_brew_podcasts_item")
                    .table(BrewPodcasts::Table)
                    .col(BrewPodcasts::ItemId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 外键约束：删除文章时级联删除播客
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_brew_podcasts_item")
                    .from(BrewPodcasts::Table, BrewPodcasts::ItemId)
                    .to(BrewItems::Table, BrewItems::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // ==================== 7. BREW_COMMENTS 表 ====================
        // 存储用户对文章的选中文本评论（类似批注）
        manager
            .create_table(
                Table::create()
                    .table(BrewComments::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BrewComments::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 关联文章 ID
                    .col(ColumnDef::new(BrewComments::ItemId).integer().not_null())
                    // 评论用户 ID
                    .col(ColumnDef::new(BrewComments::UserId).integer().not_null())
                    // 选中的原文文本
                    .col(ColumnDef::new(BrewComments::SelectedText).text().not_null())
                    // 评论内容
                    .col(ColumnDef::new(BrewComments::Comment).text().not_null())
                    // 选中文本在原文中的起始位置（字符偏移）
                    .col(ColumnDef::new(BrewComments::StartOffset).integer())
                    // 选中文本在原文中的结束位置（字符偏移）
                    .col(ColumnDef::new(BrewComments::EndOffset).integer())
                    // 上下文文本（用于定位）
                    .col(ColumnDef::new(BrewComments::ContextBefore).text())
                    .col(ColumnDef::new(BrewComments::ContextAfter).text())
                    // 评论颜色标记
                    .col(ColumnDef::new(BrewComments::Color).string_len(20))
                    // 是否公开（预留）
                    .col(
                        ColumnDef::new(BrewComments::IsPublic)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 父评论 ID（用于嵌套回复，NULL 表示顶级评论）
                    .col(ColumnDef::new(BrewComments::ParentId).integer())
                    // 创建时间
                    .col(
                        ColumnDef::new(BrewComments::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // 更新时间
                    .col(
                        ColumnDef::new(BrewComments::UpdatedAt)
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
                    .name("idx_brew_comments_item")
                    .table(BrewComments::Table)
                    .col(BrewComments::ItemId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：按用户查询评论
        manager
            .create_index(
                Index::create()
                    .name("idx_brew_comments_user")
                    .table(BrewComments::Table)
                    .col(BrewComments::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 复合索引：用户在特定文章的评论
        manager
            .create_index(
                Index::create()
                    .name("idx_brew_comments_item_user")
                    .table(BrewComments::Table)
                    .col(BrewComments::ItemId)
                    .col(BrewComments::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 外键约束：删除文章时级联删除评论
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_brew_comments_item")
                    .from(BrewComments::Table, BrewComments::ItemId)
                    .to(BrewItems::Table, BrewItems::Id)
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
                    // 健康状态: healthy, degraded, unhealthy, unknown
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

        // 索引：按优先级和健康状态查询可用实例
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

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 删除 rsshub_instances 表
        manager
            .drop_table(Table::drop().table(RsshubInstances::Table).to_owned())
            .await?;

        // 删除外键
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(BrewComments::Table)
                    .name("fk_brew_comments_item")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(BrewPodcasts::Table)
                    .name("fk_brew_podcasts_item")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(BrewAnnotations::Table)
                    .name("fk_brew_annotations_item")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(BrewUserStates::Table)
                    .name("fk_brew_user_states_item")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(BrewItems::Table)
                    .name("fk_brew_items_source")
                    .to_owned(),
            )
            .await?;

        // 删除表
        manager
            .drop_table(Table::drop().table(BrewComments::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(BrewPodcasts::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(BrewAnnotations::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(BrewCategories::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(BrewUserStates::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(BrewItems::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(BrewSources::Table).to_owned())
            .await?;

        Ok(())
    }
}

// ==================== 表定义枚举 ====================

#[derive(DeriveIden)]
enum BrewSources {
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
    UnreadCount,
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
enum BrewItems {
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
}

#[derive(DeriveIden)]
enum BrewUserStates {
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
    UpdatedAt,
}

#[derive(DeriveIden)]
enum BrewCategories {
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
enum BrewAnnotations {
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
enum BrewPodcasts {
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
enum BrewComments {
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
