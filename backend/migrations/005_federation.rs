use sea_orm_migration::prelude::*;

/// Myriad Federation Protocol (MFP) 数据库结构
///
/// 支持 ActivityPub 兼容 + MFP 扩展：
/// - Layer 1: 发现（WebFinger, NodeInfo）
/// - Layer 2: 实例核心（Actor, Inbox/Outbox, 投递队列）
/// - Layer 3: Channel(1↔1) / Room(N↔N) / Ring(去中心化)
/// - Layer 4: 内容发布 + 联邦 Timeline
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ==================== 1. FEDERATION_KEYS 表 ====================
        // 联邦密钥对（RSA-SHA256 / Ed25519）
        manager
            .create_table(
                Table::create()
                    .table(FederationKeys::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationKeys::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationKeys::UserId)
                            .integer()
                            .not_null()
                            .unique_key(),
                    )
                    // PEM 格式公钥
                    .col(
                        ColumnDef::new(FederationKeys::PublicKeyPem)
                            .text()
                            .not_null(),
                    )
                    // AES-256-GCM 加密的私钥（用 JWT_SECRET 派生密钥加密）
                    .col(
                        ColumnDef::new(FederationKeys::PrivateKeyEncrypted)
                            .text()
                            .not_null(),
                    )
                    // Key ID URL: https://domain/users/username#main-key
                    .col(
                        ColumnDef::new(FederationKeys::KeyId)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    // 签名算法
                    .col(
                        ColumnDef::new(FederationKeys::Algorithm)
                            .string_len(20)
                            .not_null()
                            .default("RSA-SHA256"),
                    )
                    .col(
                        ColumnDef::new(FederationKeys::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .col(ColumnDef::new(FederationKeys::RotatedAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await?;

        // ==================== 2. FEDERATION_REMOTE_ACTORS 表 ====================
        // 远程 Actor 缓存
        manager
            .create_table(
                Table::create()
                    .table(FederationRemoteActors::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationRemoteActors::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // ActivityPub Actor URL (唯一标识)
                    .col(
                        ColumnDef::new(FederationRemoteActors::ActorUrl)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(FederationRemoteActors::Username).text())
                    .col(
                        ColumnDef::new(FederationRemoteActors::Domain)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(FederationRemoteActors::DisplayName).text())
                    .col(ColumnDef::new(FederationRemoteActors::AvatarUrl).text())
                    .col(ColumnDef::new(FederationRemoteActors::Summary).text())
                    .col(
                        ColumnDef::new(FederationRemoteActors::InboxUrl)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(FederationRemoteActors::OutboxUrl).text())
                    .col(ColumnDef::new(FederationRemoteActors::SharedInboxUrl).text())
                    .col(ColumnDef::new(FederationRemoteActors::PublicKeyPem).text())
                    .col(ColumnDef::new(FederationRemoteActors::PublicKeyId).text())
                    // 远程实例软件类型
                    .col(ColumnDef::new(FederationRemoteActors::Software).string_len(50))
                    // MFP 协议版本（NULL = 纯 AP 实例）
                    .col(ColumnDef::new(FederationRemoteActors::MfpVersion).string_len(20))
                    // 远程实例 Tapp 能力列表
                    .col(ColumnDef::new(FederationRemoteActors::TappCapabilities).json())
                    .col(
                        ColumnDef::new(FederationRemoteActors::LastFetchedAt)
                            .timestamp_with_time_zone(),
                    )
                    .col(
                        ColumnDef::new(FederationRemoteActors::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .col(
                        ColumnDef::new(FederationRemoteActors::UpdatedAt)
                            .timestamp_with_time_zone(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_remote_actors_domain")
                    .table(FederationRemoteActors::Table)
                    .col(FederationRemoteActors::Domain)
                    .to_owned(),
            )
            .await?;

        // ==================== 3. FEDERATION_INSTANCES 表 ====================
        // 远程实例信息
        manager
            .create_table(
                Table::create()
                    .table(FederationInstances::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationInstances::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationInstances::Domain)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(FederationInstances::Software).string_len(50))
                    .col(ColumnDef::new(FederationInstances::SoftwareVersion).string_len(50))
                    .col(ColumnDef::new(FederationInstances::MfpVersion).string_len(20))
                    .col(ColumnDef::new(FederationInstances::NodeinfoUrl).text())
                    .col(ColumnDef::new(FederationInstances::SharedInboxUrl).text())
                    // 信任层级：0=unknown, 1=discovered, 2=followed, 3=trusted, 4=federated
                    .col(
                        ColumnDef::new(FederationInstances::TrustLevel)
                            .small_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(FederationInstances::IsBlocked)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(FederationInstances::BlockReason).text())
                    .col(ColumnDef::new(FederationInstances::TotalUsers).integer())
                    .col(ColumnDef::new(FederationInstances::ActiveUsersMonthly).integer())
                    .col(ColumnDef::new(FederationInstances::OpenRegistrations).boolean())
                    .col(ColumnDef::new(FederationInstances::TappCapabilities).json())
                    .col(ColumnDef::new(FederationInstances::LastSeenAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(FederationInstances::LastSuccessAt)
                            .timestamp_with_time_zone(),
                    )
                    .col(
                        ColumnDef::new(FederationInstances::FailureCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 当前连续投递失败的起点（原 015）。已存在的表由 schema_check 通用 ADD 补列。
                    .col(
                        ColumnDef::new(FederationInstances::FailingSince)
                            .timestamp_with_time_zone(),
                    )
                    .col(
                        ColumnDef::new(FederationInstances::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .col(ColumnDef::new(FederationInstances::UpdatedAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await?;

        // ==================== 4. FEDERATION_FOLLOWS 表 ====================
        // 关注关系（双向）
        manager
            .create_table(
                Table::create()
                    .table(FederationFollows::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationFollows::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationFollows::UserId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationFollows::RemoteActorId)
                            .integer()
                            .not_null(),
                    )
                    // outgoing = 我关注ta, incoming = ta关注我
                    .col(
                        ColumnDef::new(FederationFollows::Direction)
                            .string_len(10)
                            .not_null(),
                    )
                    // pending, accepted, rejected
                    .col(
                        ColumnDef::new(FederationFollows::Status)
                            .string_len(20)
                            .not_null()
                            .default("pending"),
                    )
                    .col(ColumnDef::new(FederationFollows::ActivityId).text())
                    .col(
                        ColumnDef::new(FederationFollows::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .col(ColumnDef::new(FederationFollows::AcceptedAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_follows_user")
                    .table(FederationFollows::Table)
                    .col(FederationFollows::UserId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_follows_direction_status")
                    .table(FederationFollows::Table)
                    .col(FederationFollows::Direction)
                    .col(FederationFollows::Status)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_follows_unique")
                    .table(FederationFollows::Table)
                    .col(FederationFollows::UserId)
                    .col(FederationFollows::RemoteActorId)
                    .col(FederationFollows::Direction)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // ==================== 5. FEDERATION_ACTIVITIES 表 ====================
        // Activity 日志（Outbox + 收到的）
        manager
            .create_table(
                Table::create()
                    .table(FederationActivities::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationActivities::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationActivities::ActivityId)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    // 本地用户（发出时非 NULL）
                    .col(ColumnDef::new(FederationActivities::UserId).integer())
                    // 远程 Actor（收到时非 NULL）
                    .col(ColumnDef::new(FederationActivities::RemoteActorId).integer())
                    // Create, Announce, Follow, Accept, Like, myriad:ChannelOpen...
                    .col(
                        ColumnDef::new(FederationActivities::ActivityType)
                            .string_len(50)
                            .not_null(),
                    )
                    .col(ColumnDef::new(FederationActivities::ObjectType).string_len(50))
                    .col(
                        ColumnDef::new(FederationActivities::ObjectJson)
                            .json()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationActivities::IsLocal)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(FederationActivities::PublishedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .col(
                        ColumnDef::new(FederationActivities::ReceivedAt).timestamp_with_time_zone(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_activities_user")
                    .table(FederationActivities::Table)
                    .col(FederationActivities::UserId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_activities_type")
                    .table(FederationActivities::Table)
                    .col(FederationActivities::ActivityType)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_activities_published")
                    .table(FederationActivities::Table)
                    .col(FederationActivities::PublishedAt)
                    .to_owned(),
            )
            .await?;

        // ==================== 6. FEDERATION_DELIVERY_QUEUE 表 ====================
        // 持久化投递队列（指数退避重试）
        manager
            .create_table(
                Table::create()
                    .table(FederationDeliveryQueue::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationDeliveryQueue::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationDeliveryQueue::ActivityId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationDeliveryQueue::TargetInbox)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationDeliveryQueue::TargetDomain)
                            .text()
                            .not_null(),
                    )
                    // pending, delivering, delivered, failed, dead
                    .col(
                        ColumnDef::new(FederationDeliveryQueue::Status)
                            .string_len(20)
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(FederationDeliveryQueue::Attempts)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(FederationDeliveryQueue::MaxAttempts)
                            .integer()
                            .not_null()
                            .default(12),
                    )
                    .col(
                        ColumnDef::new(FederationDeliveryQueue::LastAttemptAt)
                            .timestamp_with_time_zone(),
                    )
                    .col(ColumnDef::new(FederationDeliveryQueue::LeaseToken).uuid())
                    .col(
                        ColumnDef::new(FederationDeliveryQueue::LeaseExpiresAt)
                            .timestamp_with_time_zone(),
                    )
                    .col(
                        ColumnDef::new(FederationDeliveryQueue::NextRetryAt)
                            .timestamp_with_time_zone()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .col(ColumnDef::new(FederationDeliveryQueue::ErrorMessage).text())
                    .col(
                        ColumnDef::new(FederationDeliveryQueue::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_delivery_pending")
                    .table(FederationDeliveryQueue::Table)
                    .col(FederationDeliveryQueue::Status)
                    .col(FederationDeliveryQueue::NextRetryAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_delivery_lease_expiry")
                    .table(FederationDeliveryQueue::Table)
                    .col(FederationDeliveryQueue::Status)
                    .col(FederationDeliveryQueue::LeaseExpiresAt)
                    .to_owned(),
            )
            .await?;

        // 同一条活动对同一个 inbox 只应排队一次。25 个入队点都是裸 INSERT，
        // 没有这个约束就无法阻止重复投递（远端会收到两次同一条活动）。
        // 入队处配合 ON CONFLICT (activity_id, target_inbox) DO NOTHING。
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .unique()
                    .name("idx_delivery_queue_activity_target")
                    .table(FederationDeliveryQueue::Table)
                    .col(FederationDeliveryQueue::ActivityId)
                    .col(FederationDeliveryQueue::TargetInbox)
                    .to_owned(),
            )
            .await?;

        // 按目标域名扫队列（原 015）。LOWER() 表达式索引 SeaORM Iden 建不了。
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE INDEX IF NOT EXISTS idx_delivery_queue_target_domain
    ON federation_delivery_queue (LOWER(target_domain), status);
"#,
            )
            .await?;

        // ==================== 7. FEDERATION_CHANNELS 表 ====================
        // 1:1 双向通道
        manager
            .create_table(
                Table::create()
                    .table(FederationChannels::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationChannels::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationChannels::ChannelId)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(FederationChannels::UserId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationChannels::RemoteActorId)
                            .integer()
                            .not_null(),
                    )
                    // text, file-transfer, rpc, data-exchange, stream
                    .col(
                        ColumnDef::new(FederationChannels::ChannelType)
                            .string_len(30)
                            .not_null(),
                    )
                    // 关联的 Tapp ID
                    .col(ColumnDef::new(FederationChannels::TappId).string_len(255))
                    // pending, accepted, active, closed, rejected
                    .col(
                        ColumnDef::new(FederationChannels::Status)
                            .string_len(20)
                            .not_null()
                            .default("pending"),
                    )
                    // http, websocket
                    .col(
                        ColumnDef::new(FederationChannels::Transport)
                            .string_len(10)
                            .not_null()
                            .default("http"),
                    )
                    // 通道属性 JSON
                    .col(ColumnDef::new(FederationChannels::Properties).json())
                    // local 或 remote
                    .col(
                        ColumnDef::new(FederationChannels::InitiatedBy)
                            .string_len(10)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationChannels::LastActivityAt)
                            .timestamp_with_time_zone(),
                    )
                    .col(
                        ColumnDef::new(FederationChannels::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .col(ColumnDef::new(FederationChannels::ClosedAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_channels_user")
                    .table(FederationChannels::Table)
                    .col(FederationChannels::UserId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_channels_status")
                    .table(FederationChannels::Table)
                    .col(FederationChannels::Status)
                    .to_owned(),
            )
            .await?;

        // ==================== 8. FEDERATION_CHANNEL_MESSAGES 表 ====================
        manager
            .create_table(
                Table::create()
                    .table(FederationChannelMessages::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationChannelMessages::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationChannelMessages::ChannelId)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationChannelMessages::MessageId)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(FederationChannelMessages::SenderActor)
                            .text()
                            .not_null(),
                    )
                    // text, file-meta, rpc-request, rpc-response, system
                    .col(
                        ColumnDef::new(FederationChannelMessages::MessageType)
                            .string_len(30)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationChannelMessages::Payload)
                            .json()
                            .not_null(),
                    )
                    .col(ColumnDef::new(FederationChannelMessages::ReplyTo).text())
                    .col(
                        ColumnDef::new(FederationChannelMessages::IsEncrypted)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(FederationChannelMessages::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_channel_msgs_channel")
                    .table(FederationChannelMessages::Table)
                    .col(FederationChannelMessages::ChannelId)
                    .col(FederationChannelMessages::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // ==================== 9. FEDERATION_ROOMS 表 ====================
        // N:N 持久房间
        manager
            .create_table(
                Table::create()
                    .table(FederationRooms::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationRooms::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationRooms::RoomId)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(FederationRooms::Name)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(ColumnDef::new(FederationRooms::Description).text())
                    .col(ColumnDef::new(FederationRooms::AvatarUrl).text())
                    // 房主 Actor URL
                    .col(
                        ColumnDef::new(FederationRooms::OwnerActor)
                            .text()
                            .not_null(),
                    )
                    // 主服务器域名
                    .col(
                        ColumnDef::new(FederationRooms::HomeServer)
                            .text()
                            .not_null(),
                    )
                    // owner, democratic, open
                    .col(
                        ColumnDef::new(FederationRooms::GovernanceType)
                            .string_len(20)
                            .not_null()
                            .default("owner"),
                    )
                    .col(ColumnDef::new(FederationRooms::GovernanceConfig).json())
                    // 房间内启用的 Tapp 列表
                    .col(ColumnDef::new(FederationRooms::EnabledTapps).json())
                    // 共享数据范围
                    .col(ColumnDef::new(FederationRooms::SharedDataConfig).json())
                    // fan-out, mesh
                    .col(
                        ColumnDef::new(FederationRooms::DistributionStrategy)
                            .string_len(20)
                            .not_null()
                            .default("fan-out"),
                    )
                    .col(
                        ColumnDef::new(FederationRooms::MaxMembers)
                            .integer()
                            .not_null()
                            .default(50),
                    )
                    .col(
                        ColumnDef::new(FederationRooms::IsPublic)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // admin-only, member-invite, open
                    .col(
                        ColumnDef::new(FederationRooms::InvitePolicy)
                            .string_len(20)
                            .not_null()
                            .default("admin-only"),
                    )
                    .col(
                        ColumnDef::new(FederationRooms::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .col(ColumnDef::new(FederationRooms::UpdatedAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await?;

        // ==================== 10. FEDERATION_ROOM_MEMBERS 表 ====================
        manager
            .create_table(
                Table::create()
                    .table(FederationRoomMembers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationRoomMembers::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationRoomMembers::RoomId)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationRoomMembers::ActorUrl)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationRoomMembers::IsLocal)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(FederationRoomMembers::LocalUserId).integer())
                    // owner, admin, member, observer
                    .col(
                        ColumnDef::new(FederationRoomMembers::Role)
                            .string_len(20)
                            .not_null()
                            .default("member"),
                    )
                    .col(ColumnDef::new(FederationRoomMembers::CustomPermissions).json())
                    .col(
                        ColumnDef::new(FederationRoomMembers::JoinedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .col(ColumnDef::new(FederationRoomMembers::InvitedBy).text())
                    // 邀请生命周期：pending（已邀未接受）/ active
                    .col(
                        ColumnDef::new(FederationRoomMembers::MembershipStatus)
                            .string()
                            .not_null()
                            .default("active"),
                    )
                    // 群侧栏未读徽标的已读光标
                    .col(
                        ColumnDef::new(FederationRoomMembers::LastReadAt)
                            .timestamp_with_time_zone(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_room_members_room")
                    .table(FederationRoomMembers::Table)
                    .col(FederationRoomMembers::RoomId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_room_members_unique")
                    .table(FederationRoomMembers::Table)
                    .col(FederationRoomMembers::RoomId)
                    .col(FederationRoomMembers::ActorUrl)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // ==================== 11. FEDERATION_ROOM_MESSAGES 表 ====================
        manager
            .create_table(
                Table::create()
                    .table(FederationRoomMessages::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationRoomMessages::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationRoomMessages::RoomId)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationRoomMessages::MessageId)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(FederationRoomMessages::SenderActor)
                            .text()
                            .not_null(),
                    )
                    // text, file, tapp-event, system, vote
                    .col(
                        ColumnDef::new(FederationRoomMessages::MessageType)
                            .string_len(30)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationRoomMessages::Payload)
                            .json()
                            .not_null(),
                    )
                    .col(ColumnDef::new(FederationRoomMessages::ThreadId).text())
                    .col(ColumnDef::new(FederationRoomMessages::ReplyTo).text())
                    .col(
                        ColumnDef::new(FederationRoomMessages::Reactions)
                            .json()
                            .not_null()
                            .default("{}"),
                    )
                    .col(
                        ColumnDef::new(FederationRoomMessages::IsPinned)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(FederationRoomMessages::IsEncrypted)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(FederationRoomMessages::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_room_msgs_room")
                    .table(FederationRoomMessages::Table)
                    .col(FederationRoomMessages::RoomId)
                    .col(FederationRoomMessages::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_room_msgs_thread")
                    .table(FederationRoomMessages::Table)
                    .col(FederationRoomMessages::ThreadId)
                    .to_owned(),
            )
            .await?;

        // ==================== 12. FEDERATION_RING_MEMBERSHIPS 表 ====================
        // 去中心化环
        manager
            .create_table(
                Table::create()
                    .table(FederationRingMemberships::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationRingMemberships::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationRingMemberships::RingId)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(FederationRingMemberships::RingName).string_len(255))
                    // tapp-store, brew-recommend, library-exchange, instance-directory
                    .col(
                        ColumnDef::new(FederationRingMemberships::RingType)
                            .string_len(30)
                            .not_null(),
                    )
                    .col(ColumnDef::new(FederationRingMemberships::GossipConfig).json())
                    .col(ColumnDef::new(FederationRingMemberships::KnownPeers).json())
                    .col(
                        ColumnDef::new(FederationRingMemberships::LastSyncAt)
                            .timestamp_with_time_zone(),
                    )
                    .col(
                        ColumnDef::new(FederationRingMemberships::JoinedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .to_owned(),
            )
            .await?;

        // ==================== 13. FEDERATION_PUBLISHED_CONTENT 表 ====================
        // 本地内容 → 已发布 Activity 映射
        manager
            .create_table(
                Table::create()
                    .table(FederationPublishedContent::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationPublishedContent::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationPublishedContent::UserId)
                            .integer()
                            .not_null(),
                    )
                    // report, brew-article, library, activity, tapp, dashboard
                    .col(
                        ColumnDef::new(FederationPublishedContent::ContentType)
                            .string_len(30)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationPublishedContent::ContentId)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationPublishedContent::ActivityId)
                            .text()
                            .not_null(),
                    )
                    // public, followers, mentioned, direct
                    .col(
                        ColumnDef::new(FederationPublishedContent::Visibility)
                            .string_len(20)
                            .not_null()
                            .default("public"),
                    )
                    .col(
                        ColumnDef::new(FederationPublishedContent::PublishedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .col(
                        ColumnDef::new(FederationPublishedContent::UpdatedAt)
                            .timestamp_with_time_zone(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_published_user_type")
                    .table(FederationPublishedContent::Table)
                    .col(FederationPublishedContent::UserId)
                    .col(FederationPublishedContent::ContentType)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_published_content_unique")
                    .table(FederationPublishedContent::Table)
                    .col(FederationPublishedContent::ContentType)
                    .col(FederationPublishedContent::ContentId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // ==================== 14. FEDERATION_TIMELINE 表 ====================
        // 聚合远程内容
        manager
            .create_table(
                Table::create()
                    .table(FederationTimeline::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationTimeline::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationTimeline::UserId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationTimeline::ActivityId)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(FederationTimeline::RemoteActorId).integer())
                    .col(ColumnDef::new(FederationTimeline::ActivityType).string_len(50))
                    .col(ColumnDef::new(FederationTimeline::ObjectType).string_len(50))
                    .col(ColumnDef::new(FederationTimeline::ContentPreview).text())
                    .col(ColumnDef::new(FederationTimeline::ContentJson).json())
                    .col(
                        ColumnDef::new(FederationTimeline::IsRead)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(FederationTimeline::IsBookmarked)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(FederationTimeline::ReceivedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .to_owned(),
            )
            .await?;

        // 同一用户的同一条活动只应出现一次。6 个写入点原先各自用
        // `WHERE NOT EXISTS` 去重，那是先查后插，并发下会双双插入。
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .unique()
                    .name("idx_timeline_user_activity")
                    .table(FederationTimeline::Table)
                    .col(FederationTimeline::UserId)
                    .col(FederationTimeline::ActivityId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_timeline_user_received")
                    .table(FederationTimeline::Table)
                    .col(FederationTimeline::UserId)
                    .col(FederationTimeline::ReceivedAt)
                    .to_owned(),
            )
            .await?;

        // ==================== 15. FEDERATION_FILE_TRANSFERS 表 ====================
        manager
            .create_table(
                Table::create()
                    .table(FederationFileTransfers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FederationFileTransfers::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FederationFileTransfers::ChannelId)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationFileTransfers::TransferId)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(FederationFileTransfers::Filename)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FederationFileTransfers::FileSize)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(FederationFileTransfers::MimeType).string_len(255))
                    .col(ColumnDef::new(FederationFileTransfers::ChecksumSha256).text())
                    // send, receive
                    .col(
                        ColumnDef::new(FederationFileTransfers::Direction)
                            .string_len(10)
                            .not_null(),
                    )
                    // pending, transferring, completed, failed, cancelled
                    .col(
                        ColumnDef::new(FederationFileTransfers::Status)
                            .string_len(20)
                            .not_null()
                            .default("pending"),
                    )
                    .col(ColumnDef::new(FederationFileTransfers::ChunksTotal).integer())
                    .col(
                        ColumnDef::new(FederationFileTransfers::ChunksCompleted)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(FederationFileTransfers::LocalPath).text())
                    .col(
                        ColumnDef::new(FederationFileTransfers::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT NOW()".to_owned()),
                    )
                    .col(
                        ColumnDef::new(FederationFileTransfers::CompletedAt)
                            .timestamp_with_time_zone(),
                    )
                    // 群聊分块传输：传输归属的房间与发起者
                    .col(ColumnDef::new(FederationFileTransfers::RoomId).text())
                    .col(ColumnDef::new(FederationFileTransfers::OwnerUserId).integer())
                    .to_owned(),
            )
            .await?;

        // 群文件列表：按 room_id 查 transfer，按时间倒序
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_file_transfers_room")
                    .table(FederationFileTransfers::Table)
                    .col(FederationFileTransfers::RoomId)
                    .col(FederationFileTransfers::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // ==================== 扩展表（与 schema_check ensure_* 同结构）====================
        // 内容过滤 / 策略单例 / domain Move 别名 / 对象互动
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
CREATE TABLE IF NOT EXISTS federation_content_filters (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    filter_type VARCHAR NOT NULL,
    value TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS federation_policy_settings (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    min_trust_level SMALLINT NOT NULL DEFAULT 0,
    allowed_domains JSONB NOT NULL DEFAULT '[]'::jsonb,
    auto_discover BOOLEAN NOT NULL DEFAULT true,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    rate_max_requests BIGINT NOT NULL DEFAULT 100,
    rate_window_seconds BIGINT NOT NULL DEFAULT 60,
    rate_trusted_multiplier BIGINT NOT NULL DEFAULT 5
);
INSERT INTO federation_policy_settings (id) VALUES (1)
ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS federation_domain_aliases (
    id SERIAL PRIMARY KEY,
    old_base_url TEXT NOT NULL UNIQUE,
    new_base_url TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_federation_domain_aliases_new
    ON federation_domain_aliases (new_base_url);

CREATE TABLE IF NOT EXISTS federation_object_interactions (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    object_id TEXT NOT NULL,
    kind VARCHAR(20) NOT NULL,
    activity_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT federation_object_interactions_kind_check
        CHECK (kind IN ('like', 'bookmark', 'announce')),
    CONSTRAINT federation_object_interactions_unique
        UNIQUE (user_id, object_id, kind)
);
CREATE INDEX IF NOT EXISTS idx_fed_interactions_object_kind
    ON federation_object_interactions (object_id, kind);
CREATE INDEX IF NOT EXISTS idx_fed_interactions_user_kind_created
    ON federation_object_interactions (user_id, kind, created_at DESC);

-- 联邦 inbox 幂等回执（原 012/013；已跑过旧 005 的库由 schema_check 建表/修旧形）
CREATE TABLE IF NOT EXISTS federation_inbox_receipts (
    signer TEXT NOT NULL,
    activity_id TEXT NOT NULL,
    inbox_scope TEXT NOT NULL,
    body_digest CHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'processing',
    outcome_status SMALLINT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    CONSTRAINT federation_inbox_receipts_status_check
        CHECK (status IN ('processing', 'accepted', 'rejected')),
    CONSTRAINT federation_inbox_receipts_digest_check
        CHECK (body_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT federation_inbox_receipts_pkey
        PRIMARY KEY (signer, activity_id, inbox_scope)
);
"#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 按依赖顺序反向删除
        manager
            .get_connection()
            .execute_unprepared(
                r#"
DROP TABLE IF EXISTS federation_inbox_receipts;
DROP TABLE IF EXISTS federation_object_interactions;
DROP TABLE IF EXISTS federation_domain_aliases;
DROP TABLE IF EXISTS federation_policy_settings;
DROP TABLE IF EXISTS federation_content_filters;
"#,
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(FederationFileTransfers::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(FederationTimeline::Table).to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(FederationPublishedContent::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(FederationRingMemberships::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(FederationRoomMessages::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(FederationRoomMembers::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(FederationRooms::Table).to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(FederationChannelMessages::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(FederationChannels::Table).to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(FederationDeliveryQueue::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(FederationActivities::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(FederationFollows::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(FederationInstances::Table).to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(FederationRemoteActors::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(FederationKeys::Table).to_owned())
            .await?;
        Ok(())
    }
}

// ==================== 表标识定义 ====================

#[derive(Iden)]
pub enum FederationKeys {
    Table,
    Id,
    UserId,
    PublicKeyPem,
    PrivateKeyEncrypted,
    KeyId,
    Algorithm,
    CreatedAt,
    RotatedAt,
}

#[derive(Iden)]
pub enum FederationRemoteActors {
    Table,
    Id,
    ActorUrl,
    Username,
    Domain,
    DisplayName,
    AvatarUrl,
    Summary,
    InboxUrl,
    OutboxUrl,
    SharedInboxUrl,
    PublicKeyPem,
    PublicKeyId,
    Software,
    MfpVersion,
    TappCapabilities,
    LastFetchedAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
pub enum FederationInstances {
    Table,
    Id,
    Domain,
    Software,
    SoftwareVersion,
    MfpVersion,
    NodeinfoUrl,
    SharedInboxUrl,
    TrustLevel,
    IsBlocked,
    BlockReason,
    TotalUsers,
    ActiveUsersMonthly,
    OpenRegistrations,
    TappCapabilities,
    LastSeenAt,
    LastSuccessAt,
    FailureCount,
    FailingSince,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
pub enum FederationFollows {
    Table,
    Id,
    UserId,
    RemoteActorId,
    Direction,
    Status,
    ActivityId,
    CreatedAt,
    AcceptedAt,
}

#[derive(Iden)]
pub enum FederationActivities {
    Table,
    Id,
    ActivityId,
    UserId,
    RemoteActorId,
    ActivityType,
    ObjectType,
    ObjectJson,
    IsLocal,
    PublishedAt,
    ReceivedAt,
}

#[derive(Iden)]
pub enum FederationDeliveryQueue {
    Table,
    Id,
    ActivityId,
    TargetInbox,
    TargetDomain,
    Status,
    Attempts,
    MaxAttempts,
    LastAttemptAt,
    LeaseToken,
    LeaseExpiresAt,
    NextRetryAt,
    ErrorMessage,
    CreatedAt,
}

#[derive(Iden)]
pub enum FederationChannels {
    Table,
    Id,
    ChannelId,
    UserId,
    RemoteActorId,
    ChannelType,
    TappId,
    Status,
    Transport,
    Properties,
    InitiatedBy,
    LastActivityAt,
    CreatedAt,
    ClosedAt,
}

#[derive(Iden)]
pub enum FederationChannelMessages {
    Table,
    Id,
    ChannelId,
    MessageId,
    SenderActor,
    MessageType,
    Payload,
    ReplyTo,
    IsEncrypted,
    CreatedAt,
}

#[derive(Iden)]
pub enum FederationRooms {
    Table,
    Id,
    RoomId,
    Name,
    Description,
    AvatarUrl,
    OwnerActor,
    HomeServer,
    GovernanceType,
    GovernanceConfig,
    EnabledTapps,
    SharedDataConfig,
    DistributionStrategy,
    MaxMembers,
    IsPublic,
    InvitePolicy,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
pub enum FederationRoomMembers {
    Table,
    Id,
    RoomId,
    ActorUrl,
    IsLocal,
    LocalUserId,
    Role,
    CustomPermissions,
    JoinedAt,
    InvitedBy,
    MembershipStatus,
    LastReadAt,
}

#[derive(Iden)]
pub enum FederationRoomMessages {
    Table,
    Id,
    RoomId,
    MessageId,
    SenderActor,
    MessageType,
    Payload,
    ThreadId,
    ReplyTo,
    Reactions,
    IsPinned,
    IsEncrypted,
    CreatedAt,
}

#[derive(Iden)]
pub enum FederationRingMemberships {
    Table,
    Id,
    RingId,
    RingName,
    RingType,
    GossipConfig,
    KnownPeers,
    LastSyncAt,
    JoinedAt,
}

#[derive(Iden)]
pub enum FederationPublishedContent {
    Table,
    Id,
    UserId,
    ContentType,
    ContentId,
    ActivityId,
    Visibility,
    PublishedAt,
    UpdatedAt,
}

#[derive(Iden)]
pub enum FederationTimeline {
    Table,
    Id,
    UserId,
    ActivityId,
    RemoteActorId,
    ActivityType,
    ObjectType,
    ContentPreview,
    ContentJson,
    IsRead,
    IsBookmarked,
    ReceivedAt,
}

#[derive(Iden)]
pub enum FederationFileTransfers {
    Table,
    Id,
    ChannelId,
    TransferId,
    Filename,
    FileSize,
    MimeType,
    ChecksumSha256,
    Direction,
    Status,
    ChunksTotal,
    ChunksCompleted,
    LocalPath,
    CreatedAt,
    CompletedAt,
    RoomId,
    OwnerUserId,
}
