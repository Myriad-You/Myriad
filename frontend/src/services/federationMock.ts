/**
 * 联邦功能 Mock 数据 — 开发/测试环境使用
 *
 * 当后端不可用时，federationApi 自动 fallback 至此 mock 层，
 * 可在浏览器里直接看到完整的联邦页面 UI。
 */

import type {
  ChannelDetail,
  ChannelListResponse,
  ChannelSummary,
  FederationIdentity,
  FollowListResponse,
  MessageItem,
  MessageListResponse,
  PublishedItem,
  PublishedListResponse,
  RemoteActor,
  RingDetail,
  RingListResponse,
  RingPeer,
  RingPeersResponse,
  RingSummary,
  RoomDetail,
  RoomListResponse,
  RoomMember,
  RoomMembersResponse,
  RoomMessageItem,
  RoomMessageListResponse,
  RoomSummary,
  SendMessageResponse,
  SendRoomMessageResponse,
  TimelineItem,
  TimelineResponse,
} from '../types/federation'

// ==================== 远程用户 ====================

const MOCK_IDENTITY: FederationIdentity = {
  username: 'me',
  domain: 'myriad.local',
  handle: '@me@myriad.local',
  acct: 'me@myriad.local',
  webfinger_resource: 'acct:me@myriad.local',
  actor_url: 'https://myriad.local/users/me',
  inbox_url: 'https://myriad.local/users/me/inbox',
  outbox_url: 'https://myriad.local/users/me/outbox',
  followers_url: 'https://myriad.local/users/me/followers',
  following_url: 'https://myriad.local/users/me/following',
  profile_url: 'https://myriad.local/profile/me',
}

const MOCK_ACTORS: RemoteActor[] = [
  {
    actor_url: 'https://mastodon.social/users/alice',
    username: 'alice',
    domain: 'mastodon.social',
    display_name: 'Alice Chen',
    avatar_url: 'https://i.pravatar.cc/150?u=alice@mastodon.social',
    status: 'accepted',
  },
  {
    actor_url: 'https://misskey.io/users/bob',
    username: 'bob',
    domain: 'misskey.io',
    display_name: 'Bob 田中',
    avatar_url: 'https://i.pravatar.cc/150?u=bob@misskey.io',
    status: 'accepted',
  },
  {
    actor_url: 'https://pixelfed.social/users/carol',
    username: 'carol',
    domain: 'pixelfed.social',
    display_name: 'Carol Wang',
    avatar_url: 'https://i.pravatar.cc/150?u=carol@pixelfed.social',
    status: 'accepted',
  },
  {
    actor_url: 'https://lemmy.world/u/dave',
    username: 'dave',
    domain: 'lemmy.world',
    display_name: 'Dave López',
    avatar_url: 'https://i.pravatar.cc/150?u=dave@lemmy.world',
    status: 'pending',
  },
  {
    actor_url: 'https://pleroma.example.org/users/eve',
    username: 'eve',
    domain: 'pleroma.example.org',
    display_name: 'Eve 佐藤',
    avatar_url: 'https://i.pravatar.cc/150?u=eve@pleroma.example.org',
    status: 'accepted',
  },
]

// ==================== 时间线 ====================

const MOCK_TIMELINE: TimelineItem[] = [
  {
    activity_id: 'act-tl-001',
    activity_type: 'Create',
    object_type: 'Note',
    content_preview:
      '刚发现了一个超棒的开源项目 Myriad，集成了 RSS 阅读器和去中心化协议，太酷了！',
    is_read: false,
    created_at: '2026-03-14T08:00:00Z',
    received_at: '2026-03-14T08:00:00Z',
    actor: {
      actor_url: 'https://mastodon.social/users/alice',
      username: 'alice',
      domain: 'mastodon.social',
      display_name: 'Alice Chen',
      avatar_url: 'https://i.pravatar.cc/150?u=alice@mastodon.social',
    },
  },
  {
    activity_id: 'act-tl-002',
    activity_type: 'Announce',
    object_type: 'Article',
    content_preview:
      '转发了一篇关于去中心化社交网络未来发展的深度分析文章。ActivityPub 协议正在改变互联网的基础架构...',
    is_read: false,
    created_at: '2026-03-14T07:30:00Z',
    received_at: '2026-03-14T07:30:00Z',
    actor: {
      actor_url: 'https://misskey.io/users/bob',
      username: 'bob',
      domain: 'misskey.io',
      display_name: 'Bob 田中',
      avatar_url: 'https://i.pravatar.cc/150?u=bob@misskey.io',
    },
  },
  {
    activity_id: 'act-tl-003',
    activity_type: 'Create',
    object_type: 'Article',
    content_preview:
      '我的新摄影作品集更新了，这次去了冰岛拍摄北极光，已同步到所有节点。',
    is_read: true,
    created_at: '2026-03-13T18:00:00Z',
    received_at: '2026-03-13T18:00:00Z',
    actor: {
      actor_url: 'https://pixelfed.social/users/carol',
      username: 'carol',
      domain: 'pixelfed.social',
      display_name: 'Carol Wang',
      avatar_url: 'https://i.pravatar.cc/150?u=carol@pixelfed.social',
    },
  },
  {
    activity_id: 'act-tl-004',
    activity_type: 'Create',
    object_type: 'Note',
    content_preview:
      '有没有人试过用 Ring 协议做分布式 Tapp 商店？gossip 同步效率意外地高。',
    is_read: true,
    created_at: '2026-03-13T12:00:00Z',
    received_at: '2026-03-13T12:00:00Z',
    actor: {
      actor_url: 'https://lemmy.world/u/dave',
      username: 'dave',
      domain: 'lemmy.world',
      display_name: 'Dave López',
      avatar_url: 'https://i.pravatar.cc/150?u=dave@lemmy.world',
    },
  },
  {
    activity_id: 'act-tl-005',
    activity_type: 'Like',
    object_type: 'Note',
    content_preview: '赞了你的动态「Brew 阅读器新增了 RSSHub 集成」',
    is_read: true,
    created_at: '2026-03-12T22:00:00Z',
    received_at: '2026-03-12T22:00:00Z',
    actor: {
      actor_url: 'https://pleroma.example.org/users/eve',
      username: 'eve',
      domain: 'pleroma.example.org',
      display_name: 'Eve 佐藤',
      avatar_url: 'https://i.pravatar.cc/150?u=eve@pleroma.example.org',
    },
  },
  {
    activity_id: 'act-tl-006',
    activity_type: 'Create',
    object_type: 'Note',
    content_preview:
      '去中心化网络最近新增了不少有趣的实例，各种小众社区正在涌现。开放协议 + 自托管 = 真正的互联网自由。',
    is_read: false,
    created_at: '2026-03-12T09:15:00Z',
    received_at: '2026-03-12T09:15:00Z',
    actor: {
      actor_url: 'https://mastodon.social/users/alice',
      username: 'alice',
      domain: 'mastodon.social',
      display_name: 'Alice Chen',
      avatar_url: 'https://i.pravatar.cc/150?u=alice@mastodon.social',
    },
  },
]

// ==================== 已发布内容 ====================

const MOCK_PUBLISHED: PublishedItem[] = [
  {
    id: 1,
    content_type: 'report',
    content_id: 'rpt-2026-spring',
    activity_id: 'act-pub-001',
    visibility: 'public',
    published_at: '2026-03-10T08:30:00Z',
  },
  {
    id: 2,
    content_type: 'brew-article',
    content_id: 'brew-rss-guide',
    activity_id: 'act-pub-002',
    visibility: 'public',
    published_at: '2026-03-08T14:20:00Z',
  },
  {
    id: 3,
    content_type: 'library',
    content_id: 'lib-cyberpunk-edgerunners',
    activity_id: 'act-pub-003',
    visibility: 'followers',
    published_at: '2026-03-05T20:15:00Z',
  },
  {
    id: 4,
    content_type: 'report',
    content_id: 'rpt-winter-recap',
    activity_id: 'act-pub-004',
    visibility: 'public',
    published_at: '2026-02-28T10:00:00Z',
  },
]

// ==================== 通道 ====================

const MOCK_CHANNELS: ChannelSummary[] = [
  {
    channel_id: 'ch-001',
    remote_actor_url: 'https://mastodon.social/users/alice',
    remote_actor_name: 'Alice Chen',
    remote_actor_avatar: 'https://i.pravatar.cc/150?u=alice@mastodon.social',
    channel_type: 'text',
    status: 'active',
    transport: 'websocket',
    initiated_by: 'local',
    last_activity_at: '2026-03-14T06:30:00Z',
    created_at: '2026-03-01T10:00:00Z',
    unread_count: 3,
  },
  {
    channel_id: 'ch-002',
    remote_actor_url: 'https://misskey.io/users/bob',
    remote_actor_name: 'Bob 田中',
    remote_actor_avatar: 'https://i.pravatar.cc/150?u=bob@misskey.io',
    channel_type: 'data-exchange',
    status: 'active',
    transport: 'http',
    initiated_by: 'remote',
    last_activity_at: '2026-03-13T22:15:00Z',
    created_at: '2026-02-20T08:00:00Z',
    unread_count: 0,
  },
  {
    channel_id: 'ch-003',
    remote_actor_url: 'https://pleroma.example.org/users/eve',
    remote_actor_name: 'Eve 佐藤',
    remote_actor_avatar: 'https://i.pravatar.cc/150?u=eve@pleroma.example.org',
    channel_type: 'file-transfer',
    status: 'pending',
    transport: 'websocket',
    initiated_by: 'remote',
    last_activity_at: undefined,
    created_at: '2026-03-14T02:00:00Z',
    unread_count: 1,
  },
]

// ==================== 房间 ====================

const MOCK_ROOMS: RoomSummary[] = [
  {
    room_id: 'rm-001',
    name: '开发者交流',
    description: '讨论 ActivityPub、MFP 等去中心化协议实现',
    avatar_url: 'https://i.pravatar.cc/150?u=room-dev',
    owner_actor: 'https://myriad.local/users/me',
    governance_type: 'owner',
    invite_policy: 'member-invite',
    member_count: 12,
    max_members: 50,
    is_public: true,
    my_role: 'owner',
    last_message_at: '2026-03-14T07:45:00Z',
    created_at: '2026-01-15T10:00:00Z',
    unread_count: 5,
  },
  {
    room_id: 'rm-002',
    name: 'Brew 阅读推荐',
    description: '分享优质 RSS 源和阅读推荐',
    avatar_url: 'https://i.pravatar.cc/150?u=room-brew',
    owner_actor: 'https://mastodon.social/users/alice',
    governance_type: 'democratic',
    invite_policy: 'open',
    member_count: 28,
    max_members: 100,
    is_public: true,
    my_role: 'member',
    last_message_at: '2026-03-14T05:20:00Z',
    created_at: '2026-02-01T14:00:00Z',
    unread_count: 12,
  },
  {
    room_id: 'rm-003',
    name: 'Tapp 开发小组',
    description: '协作开发第三方 Tapp 应用插件',
    avatar_url: 'https://i.pravatar.cc/150?u=room-tapp',
    owner_actor: 'https://lemmy.world/u/dave',
    governance_type: 'open',
    invite_policy: 'open',
    member_count: 7,
    max_members: 20,
    is_public: false,
    my_role: 'admin',
    last_message_at: '2026-03-13T18:30:00Z',
    created_at: '2026-02-28T09:00:00Z',
    unread_count: 0,
  },
  {
    room_id: 'rm-004',
    name: '动画与游戏杂谈',
    description: '聊聊最近在看的番和在玩的游戏',
    avatar_url: 'https://i.pravatar.cc/150?u=room-anime',
    owner_actor: 'https://misskey.io/users/bob',
    governance_type: 'owner',
    invite_policy: 'admin-only',
    member_count: 42,
    max_members: 100,
    is_public: true,
    my_role: 'member',
    last_message_at: '2026-03-14T08:10:00Z',
    created_at: '2025-12-20T16:00:00Z',
    unread_count: 8,
  },
]

// ==================== 环网 ====================

const MOCK_RINGS: RingSummary[] = [
  {
    ring_id: 'ring-001',
    ring_name: 'Global Brew Recommend',
    ring_type: 'brew-recommend',
    peer_count: 15,
    last_sync_at: '2026-03-14T07:00:00Z',
    joined_at: '2026-01-10T10:00:00Z',
  },
  {
    ring_id: 'ring-002',
    ring_name: 'Community Tapp Store',
    ring_type: 'tapp-store',
    peer_count: 8,
    last_sync_at: '2026-03-14T06:45:00Z',
    joined_at: '2026-02-15T12:00:00Z',
  },
  {
    ring_id: 'ring-003',
    ring_name: '东亚 Library 交换网',
    ring_type: 'library-exchange',
    peer_count: 23,
    last_sync_at: '2026-03-13T23:30:00Z',
    joined_at: '2026-03-01T08:00:00Z',
  },
  {
    ring_id: 'ring-004',
    ring_name: 'Fediverse Instance Directory',
    ring_type: 'instance-directory',
    peer_count: 47,
    last_sync_at: '2026-03-14T08:00:00Z',
    joined_at: '2026-01-05T06:00:00Z',
  },
]

// ==================== Channel 详情 ====================

const MOCK_CHANNEL_DETAILS: Record<string, ChannelDetail> = {
  'ch-001': {
    channel_id: 'ch-001',
    remote_actor_url: 'https://mastodon.social/users/alice',
    remote_actor_name: 'Alice Chen',
    remote_actor_avatar: 'https://i.pravatar.cc/150?u=alice@mastodon.social',
    channel_type: 'text',
    status: 'active',
    transport: 'websocket',
    initiated_by: 'local',
    last_activity_at: '2026-03-14T06:30:00Z',
    created_at: '2026-03-01T10:00:00Z',
  },
  'ch-002': {
    channel_id: 'ch-002',
    remote_actor_url: 'https://misskey.io/users/bob',
    remote_actor_name: 'Bob 田中',
    remote_actor_avatar: 'https://i.pravatar.cc/150?u=bob@misskey.io',
    channel_type: 'data-exchange',
    status: 'active',
    transport: 'http',
    initiated_by: 'remote',
    last_activity_at: '2026-03-13T22:15:00Z',
    created_at: '2026-02-20T08:00:00Z',
  },
  'ch-003': {
    channel_id: 'ch-003',
    remote_actor_url: 'https://pleroma.example.org/users/eve',
    remote_actor_name: 'Eve 佐藤',
    remote_actor_avatar: 'https://i.pravatar.cc/150?u=eve@pleroma.example.org',
    channel_type: 'file-transfer',
    status: 'pending',
    transport: 'websocket',
    initiated_by: 'remote',
    created_at: '2026-03-14T02:00:00Z',
  },
}

const MOCK_CHANNEL_MESSAGES: Record<string, MessageItem[]> = {
  'ch-001': [
    {
      message_id: 'msg-ch1-001',
      sender_actor: 'https://mastodon.social/users/alice',
      message_type: 'text',
      payload: { text: '你好！最近 Myriad 的社交功能进展如何？' },
      is_encrypted: false,
      created_at: '2026-03-14T05:00:00Z',
    },
    {
      message_id: 'msg-ch1-002',
      sender_actor: 'https://myriad.local/users/me',
      message_type: 'text',
      payload: {
        text: '进展不错，Channel 和 Room 基本功能都实现了，正在测试中。',
      },
      is_encrypted: false,
      created_at: '2026-03-14T05:05:00Z',
    },
    {
      message_id: 'msg-ch1-003',
      sender_actor: 'https://mastodon.social/users/alice',
      message_type: 'text',
      payload: {
        text: '太好了！Ring 的 gossip 同步我也很期待，可以用来做跨实例的 RSS 推荐。',
      },
      is_encrypted: false,
      created_at: '2026-03-14T05:10:00Z',
    },
    {
      message_id: 'msg-ch1-004',
      sender_actor: 'https://myriad.local/users/me',
      message_type: 'tapp',
      payload: {
        title: 'Aro Messenger',
        description: '即时通讯应用，支持频道、群组和圈子',
        content_type: 'tapp',
        tapp_id: 'com.myriad.aro',
        tapp_version: '1.0.0',
        icon: 'tapp',
        text: '试试这个新做的通讯应用',
      },
      is_encrypted: false,
      created_at: '2026-03-14T06:00:00Z',
    },
    {
      message_id: 'msg-ch1-005',
      sender_actor: 'https://mastodon.social/users/alice',
      message_type: 'brew',
      payload: {
        title: 'Hacker News 精选',
        description: '每日精选 HN 热门文章，自动翻译中文摘要',
        content_type: 'brew',
        brew_id: 42,
        brew_link: 'https://news.ycombinator.com/rss',
        icon: 'brew',
        text: '这个 Brew 源不错，推荐给你',
      },
      is_encrypted: false,
      created_at: '2026-03-14T06:30:00Z',
    },
    {
      message_id: 'msg-ch1-006',
      sender_actor: 'https://myriad.local/users/me',
      message_type: 'text',
      payload: { text: '收到，我安装看看！' },
      is_encrypted: false,
      created_at: '2026-03-14T06:35:00Z',
    },
  ],
  'ch-002': [
    {
      message_id: 'msg-ch2-001',
      sender_actor: 'https://misskey.io/users/bob',
      message_type: 'text',
      payload: { text: 'データ交換テスト — Library の同期をチェックしよう' },
      is_encrypted: false,
      created_at: '2026-03-13T22:00:00Z',
    },
    {
      message_id: 'msg-ch2-002',
      sender_actor: 'https://myriad.local/users/me',
      message_type: 'text',
      payload: { text: '好的，我这边准备好了，发送同步请求。' },
      is_encrypted: false,
      created_at: '2026-03-13T22:15:00Z',
    },
  ],
  'ch-003': [],
}

// ==================== Room 详情 ====================

const MOCK_ROOM_DETAILS: Record<string, RoomDetail> = {
  'rm-001': {
    room_id: 'rm-001',
    name: '开发者交流',
    description: '讨论 ActivityPub、MFP 等去中心化协议实现',
    avatar_url: 'https://i.pravatar.cc/150?u=room-dev',
    owner_actor: 'https://myriad.local/users/me',
    home_server: 'myriad.local',
    governance_type: 'owner',
    invite_policy: 'member-invite',
    distribution_strategy: 'full-mesh',
    max_members: 50,
    is_public: true,
    my_role: 'owner',
    member_count: 12,
    created_at: '2026-01-15T10:00:00Z',
  },
  'rm-002': {
    room_id: 'rm-002',
    name: 'Brew 阅读推荐',
    description: '分享优质 RSS 源和阅读推荐',
    avatar_url: 'https://i.pravatar.cc/150?u=room-brew',
    owner_actor: 'https://mastodon.social/users/alice',
    home_server: 'mastodon.social',
    governance_type: 'democratic',
    invite_policy: 'open',
    distribution_strategy: 'hub-spoke',
    max_members: 100,
    is_public: true,
    my_role: 'member',
    member_count: 28,
    created_at: '2026-02-01T14:00:00Z',
  },
  'rm-003': {
    room_id: 'rm-003',
    name: 'Tapp 开发小组',
    description: '协作开发第三方 Tapp 应用插件',
    avatar_url: 'https://i.pravatar.cc/150?u=room-tapp',
    owner_actor: 'https://lemmy.world/u/dave',
    home_server: 'lemmy.world',
    governance_type: 'open',
    invite_policy: 'open',
    distribution_strategy: 'full-mesh',
    max_members: 20,
    is_public: false,
    my_role: 'admin',
    member_count: 7,
    created_at: '2026-02-28T09:00:00Z',
  },
  'rm-004': {
    room_id: 'rm-004',
    name: '动画与游戏杂谈',
    description: '聊聊最近在看的番和在玩的游戏',
    avatar_url: 'https://i.pravatar.cc/150?u=room-anime',
    owner_actor: 'https://misskey.io/users/bob',
    home_server: 'misskey.io',
    governance_type: 'owner',
    invite_policy: 'admin-only',
    distribution_strategy: 'hub-spoke',
    max_members: 100,
    is_public: true,
    my_role: 'member',
    member_count: 42,
    created_at: '2025-12-20T16:00:00Z',
  },
}

const MOCK_ROOM_MEMBERS: Record<string, RoomMember[]> = {
  'rm-001': [
    {
      actor_url: 'https://myriad.local/users/me',
      is_local: true,
      display_name: 'Me',
      avatar_url: 'https://i.pravatar.cc/150?u=me@myriad.local',
      role: 'owner',
      joined_at: '2026-01-15T10:00:00Z',
    },
    {
      actor_url: 'https://mastodon.social/users/alice',
      is_local: false,
      display_name: 'Alice Chen',
      avatar_url: 'https://i.pravatar.cc/150?u=alice@mastodon.social',
      role: 'admin',
      joined_at: '2026-01-16T08:00:00Z',
      invited_by: 'https://myriad.local/users/me',
    },
    {
      actor_url: 'https://misskey.io/users/bob',
      is_local: false,
      display_name: 'Bob 田中',
      avatar_url: 'https://i.pravatar.cc/150?u=bob@misskey.io',
      role: 'member',
      joined_at: '2026-01-20T12:00:00Z',
    },
    {
      actor_url: 'https://lemmy.world/u/dave',
      is_local: false,
      display_name: 'Dave López',
      avatar_url: 'https://i.pravatar.cc/150?u=dave@lemmy.world',
      role: 'member',
      joined_at: '2026-02-05T09:00:00Z',
    },
  ],
  'rm-002': [
    {
      actor_url: 'https://mastodon.social/users/alice',
      is_local: false,
      display_name: 'Alice Chen',
      avatar_url: 'https://i.pravatar.cc/150?u=alice@mastodon.social',
      role: 'owner',
      joined_at: '2026-02-01T14:00:00Z',
    },
    {
      actor_url: 'https://myriad.local/users/me',
      is_local: true,
      display_name: 'Me',
      avatar_url: 'https://i.pravatar.cc/150?u=me@myriad.local',
      role: 'member',
      joined_at: '2026-02-02T10:00:00Z',
    },
    {
      actor_url: 'https://pleroma.example.org/users/eve',
      is_local: false,
      display_name: 'Eve 佐藤',
      avatar_url: 'https://i.pravatar.cc/150?u=eve@pleroma.example.org',
      role: 'member',
      joined_at: '2026-02-10T16:00:00Z',
    },
  ],
  'rm-003': [
    {
      actor_url: 'https://lemmy.world/u/dave',
      is_local: false,
      display_name: 'Dave López',
      avatar_url: 'https://i.pravatar.cc/150?u=dave@lemmy.world',
      role: 'owner',
      joined_at: '2026-02-28T09:00:00Z',
    },
    {
      actor_url: 'https://myriad.local/users/me',
      is_local: true,
      display_name: 'Me',
      avatar_url: 'https://i.pravatar.cc/150?u=me@myriad.local',
      role: 'admin',
      joined_at: '2026-02-28T10:00:00Z',
    },
    {
      actor_url: 'https://pixelfed.social/users/carol',
      is_local: false,
      display_name: 'Carol Wang',
      avatar_url: 'https://i.pravatar.cc/150?u=carol@pixelfed.social',
      role: 'member',
      joined_at: '2026-03-01T12:00:00Z',
    },
  ],
  'rm-004': [
    {
      actor_url: 'https://misskey.io/users/bob',
      is_local: false,
      display_name: 'Bob 田中',
      role: 'owner',
      joined_at: '2025-12-20T16:00:00Z',
    },
    {
      actor_url: 'https://myriad.local/users/me',
      is_local: true,
      display_name: 'Me',
      role: 'member',
      joined_at: '2026-01-05T20:00:00Z',
    },
    {
      actor_url: 'https://mastodon.social/users/alice',
      is_local: false,
      display_name: 'Alice Chen',
      role: 'member',
      joined_at: '2026-01-10T14:00:00Z',
    },
    {
      actor_url: 'https://pleroma.example.org/users/eve',
      is_local: false,
      display_name: 'Eve 佐藤',
      role: 'member',
      joined_at: '2026-02-15T08:00:00Z',
    },
  ],
}

const MOCK_ROOM_MESSAGES: Record<string, RoomMessageItem[]> = {
  'rm-001': [
    {
      message_id: 'msg-rm1-001',
      sender_actor: 'https://mastodon.social/users/alice',
      message_type: 'text',
      payload: { text: 'MFP Layer 3 的 Channel 双向通信测试通过了' },
      reactions: {},
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-14T06:00:00Z',
    },
    {
      message_id: 'msg-rm1-002',
      sender_actor: 'https://myriad.local/users/me',
      message_type: 'text',
      payload: { text: '太好了！接下来要把 Room 的分发策略也跑一遍。' },
      reactions: {},
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-14T06:15:00Z',
    },
    {
      message_id: 'msg-rm1-003',
      sender_actor: 'https://lemmy.world/u/dave',
      message_type: 'tapp',
      payload: {
        title: 'MFP Inspector',
        description: '协议调试工具，可视化消息流和节点状态',
        content_type: 'tapp',
        tapp_id: 'com.myriad.mfp-inspector',
        tapp_version: '0.2.0',
        icon: 'search',
        text: '大家可以用这个工具来调试',
      },
      reactions: { like: 2 },
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-14T07:00:00Z',
    },
    {
      message_id: 'msg-rm1-004',
      sender_actor: 'https://misskey.io/users/bob',
      message_type: 'library',
      payload: {
        title: 'Steins;Gate (命运石之门)',
        description: 'MAL 评分 9.07 · Sci-Fi, Thriller · 24 集',
        content_type: 'library',
        platform_id: 'mal',
        item_id: '9253',
        icon: 'library',
        text: '经典神作推荐',
      },
      reactions: { heart: 4 },
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-14T07:30:00Z',
    },
    {
      message_id: 'msg-rm1-005',
      sender_actor: 'https://myriad.local/users/me',
      message_type: 'text',
      payload: { text: 'Perfect. 大家把各自负责的模块测一下，明天汇总。' },
      reactions: { like: 3 },
      is_pinned: true,
      is_encrypted: false,
      created_at: '2026-03-14T07:45:00Z',
    },
  ],
  'rm-002': [
    {
      message_id: 'msg-rm2-001',
      sender_actor: 'https://mastodon.social/users/alice',
      message_type: 'text',
      payload: { text: '推荐一个 Hacker News 的 RSS 源，配合 RSSHub 效果很好' },
      reactions: {},
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-14T04:00:00Z',
    },
    {
      message_id: 'msg-rm2-002',
      sender_actor: 'https://pleroma.example.org/users/eve',
      message_type: 'text',
      payload: { text: '我最近在用 Brew 看日本新闻，NHK 的 RSS 源质量不错' },
      reactions: {},
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-14T04:30:00Z',
    },
    {
      message_id: 'msg-rm2-003',
      sender_actor: 'https://myriad.local/users/me',
      message_type: 'text',
      payload: { text: '收藏了，顺便问下大家有没有好用的技术博客 RSS？' },
      reactions: {},
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-14T05:20:00Z',
    },
  ],
  'rm-003': [
    {
      message_id: 'msg-rm3-001',
      sender_actor: 'https://lemmy.world/u/dave',
      message_type: 'text',
      payload: {
        text: '新的 Tapp API 草案已经提交到 repo 了，大家看看有没有问题。',
      },
      reactions: {},
      is_pinned: true,
      is_encrypted: false,
      created_at: '2026-03-13T17:00:00Z',
    },
    {
      message_id: 'msg-rm3-002',
      sender_actor: 'https://pixelfed.social/users/carol',
      message_type: 'text',
      payload: {
        text: '我看了一下，sandbox 部分的设计很棒，权限模型也很清晰。',
      },
      reactions: {},
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-13T18:00:00Z',
    },
    {
      message_id: 'msg-rm3-003',
      sender_actor: 'https://myriad.local/users/me',
      message_type: 'text',
      payload: { text: '同意，周末我来完善文档部分。' },
      reactions: {},
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-13T18:30:00Z',
    },
  ],
  'rm-004': [
    {
      message_id: 'msg-rm4-001',
      sender_actor: 'https://misskey.io/users/bob',
      message_type: 'text',
      payload: { text: '今季度最期待的番剧是什么？我投《链锯人》第二季' },
      reactions: { hot: 5 },
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-14T07:00:00Z',
    },
    {
      message_id: 'msg-rm4-002',
      sender_actor: 'https://mastodon.social/users/alice',
      message_type: 'library',
      payload: {
        title: '葬送的芙莉莲 (Sousou no Frieren)',
        description: 'MAL 评分 9.38 · Adventure, Drama, Fantasy · 第二季确认',
        content_type: 'library',
        platform_id: 'mal',
        item_id: '52991',
        icon: 'library',
        text: '葬送的芙莉莲第二季已确定，超期待！',
      },
      reactions: { heart: 8 },
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-14T07:20:00Z',
    },
    {
      message_id: 'msg-rm4-003',
      sender_actor: 'https://myriad.local/users/me',
      message_type: 'report',
      payload: {
        title: '2026年冬季番剧观看报告',
        description: '追番 12 部，完成 8 部，平均评分 7.8',
        content_type: 'report',
        report_id: 'rpt-2026-winter',
        summary: '2026年冬季番剧观看报告',
        platform: 'mal',
        content_preview: '追番 12 部，完成 8 部，平均评分 7.8',
        icon: 'report',
        text: '分享一下我的冬季番剧报告',
      },
      reactions: {},
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-14T07:45:00Z',
    },
    {
      message_id: 'msg-rm4-004',
      sender_actor: 'https://pleroma.example.org/users/eve',
      message_type: 'text',
      payload: { text: '芙莉莲 +1！漫画也很好看。' },
      reactions: {},
      is_pinned: false,
      is_encrypted: false,
      created_at: '2026-03-14T08:10:00Z',
    },
  ],
}

// ==================== Ring 详情 ====================

const MOCK_RING_DETAILS: Record<string, RingDetail> = {
  'ring-001': {
    ring_id: 'ring-001',
    ring_name: 'Global Brew Recommend',
    ring_type: 'brew-recommend',
    gossip_config: { fanout: 3, ttl: 5, interval: 300 },
    known_peers: [
      'mastodon.social',
      'misskey.io',
      'pixelfed.social',
      'lemmy.world',
      'pleroma.example.org',
    ],
    last_sync_at: '2026-03-14T07:00:00Z',
    joined_at: '2026-01-10T10:00:00Z',
  },
  'ring-002': {
    ring_id: 'ring-002',
    ring_name: 'Community Tapp Store',
    ring_type: 'tapp-store',
    gossip_config: { fanout: 2, ttl: 3, interval: 600 },
    known_peers: ['lemmy.world', 'misskey.io', 'myriad.example.com'],
    last_sync_at: '2026-03-14T06:45:00Z',
    joined_at: '2026-02-15T12:00:00Z',
  },
  'ring-003': {
    ring_id: 'ring-003',
    ring_name: '东亚 Library 交换网',
    ring_type: 'library-exchange',
    gossip_config: { fanout: 4, ttl: 6, interval: 180 },
    known_peers: [
      'misskey.io',
      'mastodon.social',
      'pawoo.net',
      'mstdn.jp',
      'social.mikutter.hachune.net',
    ],
    last_sync_at: '2026-03-13T23:30:00Z',
    joined_at: '2026-03-01T08:00:00Z',
  },
  'ring-004': {
    ring_id: 'ring-004',
    ring_name: 'Fediverse Instance Directory',
    ring_type: 'instance-directory',
    gossip_config: { fanout: 5, ttl: 8, interval: 120 },
    known_peers: [
      'mastodon.social',
      'misskey.io',
      'pixelfed.social',
      'lemmy.world',
      'pleroma.example.org',
      'peertube.social',
      'bookwyrm.social',
    ],
    last_sync_at: '2026-03-14T08:00:00Z',
    joined_at: '2026-01-05T06:00:00Z',
  },
}

const MOCK_RING_PEERS: Record<string, RingPeer[]> = {
  'ring-001': [
    {
      actor_url: 'https://mastodon.social/relay',
      instance_domain: 'mastodon.social',
      added_at: '2026-01-10T10:00:00Z',
    },
    {
      actor_url: 'https://misskey.io/relay',
      instance_domain: 'misskey.io',
      added_at: '2026-01-12T08:00:00Z',
    },
    {
      actor_url: 'https://pixelfed.social/relay',
      instance_domain: 'pixelfed.social',
      added_at: '2026-01-15T12:00:00Z',
    },
    {
      actor_url: 'https://lemmy.world/relay',
      instance_domain: 'lemmy.world',
      added_at: '2026-01-20T06:00:00Z',
    },
    {
      actor_url: 'https://pleroma.example.org/relay',
      instance_domain: 'pleroma.example.org',
      added_at: '2026-02-01T14:00:00Z',
    },
  ],
  'ring-002': [
    {
      actor_url: 'https://lemmy.world/relay',
      instance_domain: 'lemmy.world',
      added_at: '2026-02-15T12:00:00Z',
    },
    {
      actor_url: 'https://misskey.io/relay',
      instance_domain: 'misskey.io',
      added_at: '2026-02-18T10:00:00Z',
    },
    {
      actor_url: 'https://myriad.example.com/relay',
      instance_domain: 'myriad.example.com',
      added_at: '2026-03-01T08:00:00Z',
    },
  ],
  'ring-003': [
    {
      actor_url: 'https://misskey.io/relay',
      instance_domain: 'misskey.io',
      added_at: '2026-03-01T08:00:00Z',
    },
    {
      actor_url: 'https://mastodon.social/relay',
      instance_domain: 'mastodon.social',
      added_at: '2026-03-02T10:00:00Z',
    },
    {
      actor_url: 'https://pawoo.net/relay',
      instance_domain: 'pawoo.net',
      added_at: '2026-03-03T12:00:00Z',
    },
    {
      actor_url: 'https://mstdn.jp/relay',
      instance_domain: 'mstdn.jp',
      added_at: '2026-03-05T08:00:00Z',
    },
  ],
  'ring-004': [
    {
      actor_url: 'https://mastodon.social/relay',
      instance_domain: 'mastodon.social',
      added_at: '2026-01-05T06:00:00Z',
    },
    {
      actor_url: 'https://misskey.io/relay',
      instance_domain: 'misskey.io',
      added_at: '2026-01-06T08:00:00Z',
    },
    {
      actor_url: 'https://pixelfed.social/relay',
      instance_domain: 'pixelfed.social',
      added_at: '2026-01-08T10:00:00Z',
    },
    {
      actor_url: 'https://lemmy.world/relay',
      instance_domain: 'lemmy.world',
      added_at: '2026-01-10T12:00:00Z',
    },
    {
      actor_url: 'https://peertube.social/relay',
      instance_domain: 'peertube.social',
      added_at: '2026-01-15T14:00:00Z',
    },
    {
      actor_url: 'https://bookwyrm.social/relay',
      instance_domain: 'bookwyrm.social',
      added_at: '2026-02-01T08:00:00Z',
    },
  ],
}

// ==================== Mock API 响应包装 ====================

/** 模拟网络延迟（保持极短以避免竞态） */
function delay(ms = 60): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function mockActorFromReference(reference: string): {
  actorUrl: string
  username: string
  domain: string
} {
  const trimmed = reference.trim().replace(/^acct:/, '')
  if (trimmed.startsWith('http://') || trimmed.startsWith('https://')) {
    try {
      const url = new URL(trimmed)
      const username = url.pathname.split('/').filter(Boolean).at(-1) || trimmed
      return {
        actorUrl: trimmed,
        username,
        domain: url.host,
      }
    } catch {
      return {
        actorUrl: trimmed,
        username: trimmed,
        domain: 'unknown.social',
      }
    }
  }

  const parts = trimmed.split('@')
  const username = parts[0] || trimmed
  const domain = parts[1] || 'unknown.social'
  return {
    actorUrl: `https://${domain}/users/${username}`,
    username,
    domain,
  }
}

let _msgCounter = 100

export const federationMock = {
  // ---- 列表 ----

  async getIdentity(): Promise<FederationIdentity> {
    await delay()
    return MOCK_IDENTITY
  },

  async getTimeline(): Promise<TimelineResponse> {
    await delay()
    return { items: MOCK_TIMELINE, total: MOCK_TIMELINE.length }
  },

  async getFollowing(): Promise<FollowListResponse> {
    await delay()
    return { items: MOCK_ACTORS.slice(0, 3), total: 3 }
  },

  async getFollowers(): Promise<FollowListResponse> {
    await delay()
    return { items: [MOCK_ACTORS[0], MOCK_ACTORS[4]], total: 2 }
  },

  async getPublished(): Promise<PublishedListResponse> {
    await delay()
    return { items: MOCK_PUBLISHED, total: MOCK_PUBLISHED.length }
  },

  async getChannels(): Promise<ChannelListResponse> {
    await delay()
    return { channels: MOCK_CHANNELS, total: MOCK_CHANNELS.length }
  },

  async getRooms(): Promise<RoomListResponse> {
    await delay()
    return { rooms: MOCK_ROOMS, total: MOCK_ROOMS.length }
  },

  async getRings(): Promise<RingListResponse> {
    await delay()
    return { rings: MOCK_RINGS, total: MOCK_RINGS.length }
  },

  // ---- Channel 详情 ----

  async getChannel(channelId: string): Promise<ChannelDetail> {
    await delay()
    const detail = MOCK_CHANNEL_DETAILS[channelId]
    if (!detail) throw new Error('Channel not found')
    return detail
  },

  async getMessages(channelId: string): Promise<MessageListResponse> {
    await delay()
    const msgs = MOCK_CHANNEL_MESSAGES[channelId] || []
    return { messages: msgs, total: msgs.length }
  },

  async sendMessage(
    channelId: string,
    payload: unknown,
    messageType?: string,
  ): Promise<SendMessageResponse> {
    await delay()
    const id = `msg-mock-${++_msgCounter}`
    const msg: MessageItem = {
      message_id: id,
      sender_actor: 'https://myriad.local/users/me',
      message_type: messageType || 'text',
      payload,
      is_encrypted: false,
      created_at: new Date().toISOString(),
    }
    const bucket = MOCK_CHANNEL_MESSAGES[channelId]
    if (bucket) bucket.push(msg)
    return { success: true, message_id: id, channel_id: channelId }
  },

  // ---- Room 详情 ----

  async getRoom(roomId: string): Promise<RoomDetail> {
    await delay()
    const detail = MOCK_ROOM_DETAILS[roomId]
    if (!detail) throw new Error('Room not found')
    return detail
  },

  async updateRoom(
    roomId: string,
    req: Record<string, unknown>,
  ): Promise<RoomDetail> {
    await delay()
    const detail = MOCK_ROOM_DETAILS[roomId]
    if (!detail) throw new Error('Room not found')
    // 合并更新字段
    if (req.name !== undefined) detail.name = req.name as string
    if (req.description !== undefined)
      detail.description = req.description as string
    if (req.avatar_url !== undefined)
      detail.avatar_url = req.avatar_url as string
    if (req.invite_policy !== undefined)
      detail.invite_policy = req.invite_policy as string
    if (req.max_members !== undefined)
      detail.max_members = req.max_members as number
    if (req.is_public !== undefined) detail.is_public = req.is_public as boolean
    // 同步到 Summary 列表
    const summary = MOCK_ROOMS.find((r) => r.room_id === roomId)
    if (summary) {
      if (req.name !== undefined) summary.name = req.name as string
      if (req.description !== undefined)
        summary.description = req.description as string
      if (req.avatar_url !== undefined)
        summary.avatar_url = req.avatar_url as string
      if (req.invite_policy !== undefined)
        summary.invite_policy = req.invite_policy as string
      if (req.max_members !== undefined)
        summary.max_members = req.max_members as number
      if (req.is_public !== undefined)
        summary.is_public = req.is_public as boolean
    }
    return { ...detail }
  },

  async getRoomMembers(roomId: string): Promise<RoomMembersResponse> {
    await delay()
    const members = MOCK_ROOM_MEMBERS[roomId] || []
    return { members, total: members.length }
  },

  async getRoomMessages(roomId: string): Promise<RoomMessageListResponse> {
    await delay()
    const msgs = MOCK_ROOM_MESSAGES[roomId] || []
    return { messages: msgs, total: msgs.length }
  },

  async sendRoomMessage(
    roomId: string,
    payload: unknown,
    messageType?: string,
  ): Promise<SendRoomMessageResponse> {
    await delay()
    const id = `msg-mock-${++_msgCounter}`
    const msg: RoomMessageItem = {
      message_id: id,
      sender_actor: 'https://myriad.local/users/me',
      message_type: messageType || 'text',
      payload,
      reactions: {},
      is_pinned: false,
      is_encrypted: false,
      created_at: new Date().toISOString(),
    }
    const bucket = MOCK_ROOM_MESSAGES[roomId]
    if (bucket) bucket.push(msg)
    return { success: true, message_id: id, room_id: roomId }
  },

  // ---- Pin Room Message ----

  async pinRoomMessage(
    roomId: string,
    messageId: string,
    pinned: boolean,
  ): Promise<{
    success: boolean
    room_id: string
    message_id: string
    is_pinned: boolean
  }> {
    await delay()
    const msgs = MOCK_ROOM_MESSAGES[roomId]
    if (!msgs) throw new Error('Room not found')
    const msg = msgs.find((m) => m.message_id === messageId)
    if (!msg) throw new Error('Message not found')
    msg.is_pinned = pinned
    return {
      success: true,
      room_id: roomId,
      message_id: messageId,
      is_pinned: pinned,
    }
  },

  // ---- Ring 详情 ----

  async getRing(ringId: string): Promise<RingDetail> {
    await delay()
    const detail = MOCK_RING_DETAILS[ringId]
    if (!detail) throw new Error('Ring not found')
    return detail
  },

  async getRingPeers(ringId: string): Promise<RingPeersResponse> {
    await delay()
    const peers = MOCK_RING_PEERS[ringId] || []
    return { peers, total: peers.length }
  },

  async triggerSync(_ringId: string): Promise<{
    success: boolean
    synced_peers: number
    entries_count: number
  }> {
    await delay(200)
    return { success: true, synced_peers: 5, entries_count: 42 }
  },

  // ---- 通道/房间 创建、关闭、离开 ----

  async createChannel(req: {
    remote_actor: string
  }): Promise<{ channel_id: string }> {
    await delay()
    const id = `ch-mock-${++_msgCounter}`
    const { actorUrl, username } = mockActorFromReference(req.remote_actor)
    const ch: ChannelSummary = {
      channel_id: id,
      remote_actor_url: actorUrl,
      remote_actor_name: username,
      channel_type: 'text',
      status: 'active',
      transport: 'websocket',
      initiated_by: 'local',
      last_activity_at: new Date().toISOString(),
      created_at: new Date().toISOString(),
      unread_count: 0,
    }
    MOCK_CHANNELS.push(ch)
    MOCK_CHANNEL_DETAILS[id] = {
      channel_id: id,
      remote_actor_url: actorUrl,
      remote_actor_name: username,
      remote_actor_avatar: `https://i.pravatar.cc/150?u=${req.remote_actor}`,
      channel_type: 'text',
      status: 'active',
      transport: 'websocket',
      initiated_by: 'local',
      last_activity_at: new Date().toISOString(),
      created_at: new Date().toISOString(),
    }
    MOCK_CHANNEL_MESSAGES[id] = []
    return { channel_id: id }
  },

  async createRoom(req: { name: string }): Promise<{ room_id: string }> {
    await delay()
    const id = `rm-mock-${++_msgCounter}`
    const rm: RoomSummary = {
      room_id: id,
      name: req.name,
      description: '',
      owner_actor: 'https://myriad.local/users/me',
      governance_type: 'owner',
      invite_policy: 'member-invite',
      member_count: 1,
      max_members: 50,
      is_public: false,
      my_role: 'owner',
      last_message_at: new Date().toISOString(),
      created_at: new Date().toISOString(),
      unread_count: 0,
    }
    MOCK_ROOMS.push(rm)
    MOCK_ROOM_DETAILS[id] = {
      room_id: id,
      name: req.name,
      description: '',
      avatar_url: '',
      owner_actor: 'https://myriad.local/users/me',
      home_server: 'myriad.local',
      governance_type: 'owner',
      invite_policy: 'member-invite',
      distribution_strategy: 'full-sync',
      member_count: 1,
      max_members: 50,
      is_public: false,
      my_role: 'owner',
      created_at: new Date().toISOString(),
    }
    MOCK_ROOM_MEMBERS[id] = [
      {
        actor_url: 'https://myriad.local/users/me',
        display_name: 'Me',
        role: 'owner',
        is_local: true,
        joined_at: new Date().toISOString(),
      },
    ]
    MOCK_ROOM_MESSAGES[id] = []
    return { room_id: id }
  },

  async closeChannel(channelId: string): Promise<{ success: boolean }> {
    await delay()
    const ch = MOCK_CHANNELS.find((c) => c.channel_id === channelId)
    if (ch) ch.status = 'closed'
    const detail = MOCK_CHANNEL_DETAILS[channelId]
    if (detail) detail.status = 'closed'
    return { success: true }
  },

  async acceptChannel(channelId: string): Promise<{ success: boolean }> {
    await delay()
    const ch = MOCK_CHANNELS.find((c) => c.channel_id === channelId)
    if (ch) ch.status = 'active'
    const detail = MOCK_CHANNEL_DETAILS[channelId]
    if (detail) detail.status = 'active'
    return { success: true }
  },

  async leaveRoom(roomId: string): Promise<{ success: boolean }> {
    await delay()
    const idx = MOCK_ROOMS.findIndex((r) => r.room_id === roomId)
    if (idx !== -1) MOCK_ROOMS.splice(idx, 1)
    delete MOCK_ROOM_DETAILS[roomId]
    delete MOCK_ROOM_MEMBERS[roomId]
    delete MOCK_ROOM_MESSAGES[roomId]
    return { success: true }
  },

  async inviteMember(
    roomId: string,
    req: { actor: string; role?: string },
  ): Promise<{ success: boolean }> {
    await delay()
    const members = MOCK_ROOM_MEMBERS[roomId]
    if (!members) throw new Error('Room not found')
    // Check if already a member
    if (members.some((m) => m.actor_url === req.actor)) return { success: true }
    const parts = req.actor.split('/')
    const username = parts.at(-1) || req.actor
    members.push({
      actor_url: req.actor,
      display_name: username,
      role: req.role || 'member',
      is_local: req.actor.includes('myriad.local'),
      joined_at: new Date().toISOString(),
    })
    // Update member count
    const detail = MOCK_ROOM_DETAILS[roomId]
    if (detail) detail.member_count = members.length
    const summary = MOCK_ROOMS.find((r) => r.room_id === roomId)
    if (summary) summary.member_count = members.length
    return { success: true }
  },

  async deleteRoom(roomId: string): Promise<{ success: boolean }> {
    await delay()
    const idx = MOCK_ROOMS.findIndex((r) => r.room_id === roomId)
    if (idx !== -1) MOCK_ROOMS.splice(idx, 1)
    delete MOCK_ROOM_DETAILS[roomId]
    delete MOCK_ROOM_MEMBERS[roomId]
    delete MOCK_ROOM_MESSAGES[roomId]
    return { success: true }
  },

  async removeMember(
    roomId: string,
    actorUrl: string,
  ): Promise<{ success: boolean }> {
    await delay()
    const members = MOCK_ROOM_MEMBERS[roomId]
    if (!members) throw new Error('Room not found')
    const idx = members.findIndex((m) => m.actor_url === actorUrl)
    if (idx === -1) throw new Error('Member not found')
    members.splice(idx, 1)
    const detail = MOCK_ROOM_DETAILS[roomId]
    if (detail) detail.member_count = members.length
    const summary = MOCK_ROOMS.find((r) => r.room_id === roomId)
    if (summary) summary.member_count = members.length
    return { success: true }
  },
}
