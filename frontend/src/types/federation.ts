/**
 * 联邦功能相关类型定义
 */

// ==================== 关注相关 ====================

export interface FollowRequest {
  target: string
}

export interface FollowResponse {
  status: string
  target_actor: string
  activity_id: string
}

export interface FederationIdentity {
  username: string
  display_name?: string
  avatar_url?: string
  domain: string
  handle: string
  acct: string
  webfinger_resource: string
  actor_url: string
  inbox_url: string
  outbox_url: string
  followers_url: string
  following_url: string
  profile_url: string
}

export interface RemoteActor {
  actor_url: string
  username?: string
  domain: string
  display_name?: string
  avatar_url?: string
  status: string
}

export interface FollowListResponse {
  items: RemoteActor[]
  total: number
}

// ==================== 时间线 ====================

export interface TimelineItem {
  activity_id: string
  activity_type?: string
  object_type?: string
  content_preview?: string
  is_read: boolean
  /** ISO timestamp when the activity was received (preferred by Aro timeAgo) */
  created_at?: string
  received_at?: string
  timestamp?: string
  actor: {
    actor_url?: string
    username?: string
    domain?: string
    display_name?: string
    avatar_url?: string
  }
}

export interface TimelineResponse {
  items: TimelineItem[]
  total: number
}

// ==================== 内容发布 ====================

export interface NoteAttachmentInput {
  url: string
  media_type: string
  name?: string
}

export interface PublishRequest {
  content_type: 'report' | 'brew-article' | 'tapp' | 'library' | 'note'
  /** Required except for freeform notes (server generates id). */
  content_id?: string
  visibility?: 'public' | 'followers' | 'direct'
  /** Freeform note body when content_type is `note`. */
  text?: string
  attachments?: NoteAttachmentInput[]
}

export interface CreateNoteRequest {
  text?: string
  attachments?: NoteAttachmentInput[]
  visibility?: 'public' | 'followers' | 'direct'
}

export interface MediaUploadResponse {
  url: string
  media_type: string
  name: string
  size: number
  attachment_type: string
}

export interface PublishResponse {
  success: boolean
  activity_id: string
  content_type: string
  content_id: string
  visibility: string
  /** Follower inboxes enqueued for best-effort delivery (fan-out). */
  delivered_queued?: number
  /** Whether the Create was written to the author's local timeline. */
  author_timeline?: boolean
}

export interface UnpublishRequest {
  content_type: string
  content_id: string
}

export interface PublishedItem {
  id: number
  content_type: string
  content_id: string
  activity_id: string
  visibility: string
  published_at: string
}

export interface PublishedListResponse {
  items: PublishedItem[]
  total: number
}

// ==================== 视图状态 ====================

export type FederationTab =
  'timeline' | 'following' | 'followers' | 'published' | 'rings' | 'profile'

// ==================== Channel 通信 ====================

export interface CreateChannelRequest {
  remote_actor: string
  channel_type?: 'text' | 'file-transfer' | 'rpc' | 'data-exchange' | 'stream'
  tapp_id?: string
  transport?: 'http' | 'websocket'
}

export interface ChannelSummary {
  channel_id: string
  remote_actor_url: string
  remote_actor_name?: string
  remote_actor_avatar?: string
  channel_type: string
  status: string
  transport: string
  initiated_by: string
  last_activity_at?: string
  created_at: string
  unread_count: number
}

export interface ChannelDetail {
  channel_id: string
  remote_actor_url: string
  remote_actor_name?: string
  remote_actor_avatar?: string
  channel_type: string
  status: string
  transport: string
  tapp_id?: string
  properties?: Record<string, unknown>
  initiated_by: string
  last_activity_at?: string
  created_at: string
}

export interface ChannelListResponse {
  channels: ChannelSummary[]
  total: number
}

export interface SendMessageRequest {
  message_type?: string
  payload: unknown
  reply_to?: string
}

export interface MessageItem {
  message_id: string
  sender_actor: string
  message_type: string
  payload: unknown
  reply_to?: string
  is_encrypted: boolean
  created_at: string
}

export interface MessageListResponse {
  messages: MessageItem[]
  total: number
}

export interface SendMessageResponse {
  success: boolean
  message_id: string
  channel_id: string
}

/** WebSocket 消息类型 */
export interface WsMessage {
  type:
    | 'connected'
    | 'message'
    | 'typing'
    | 'channel_closed'
    | 'lagged'
    | 'pong'
    | 'error'
  channel_id?: string
  room_id?: string
  message?: MessageItem | RoomMessageItem
  actor?: string
  is_typing?: boolean
  missed?: number
  error?: unknown
  event?: string
}

// ==================== Room 多方通信 ====================

export interface CreateRoomRequest {
  name: string
  description?: string
  avatar_url?: string
  governance_type?: 'owner' | 'democratic' | 'open'
  invite_policy?: 'admin-only' | 'member-invite' | 'open'
  max_members?: number
  is_public?: boolean
}

export interface UpdateRoomRequest {
  name?: string
  description?: string
  avatar_url?: string
  invite_policy?: 'admin-only' | 'member-invite' | 'open'
  max_members?: number
  is_public?: boolean
}

export interface RoomSummary {
  room_id: string
  name: string
  description?: string
  avatar_url?: string
  owner_actor: string
  governance_type: string
  invite_policy: string
  member_count: number
  max_members: number
  is_public: boolean
  my_role?: string
  last_message_at?: string
  created_at: string
  unread_count: number
}

export interface RoomDetail {
  room_id: string
  name: string
  description?: string
  avatar_url?: string
  owner_actor: string
  home_server: string
  governance_type: string
  governance_config?: Record<string, unknown>
  invite_policy: string
  distribution_strategy: string
  max_members: number
  is_public: boolean
  enabled_tapps?: unknown
  my_role?: string
  member_count: number
  created_at: string
}

export interface RoomListResponse {
  rooms: RoomSummary[]
  total: number
}

export interface RoomMember {
  actor_url: string
  is_local: boolean
  display_name?: string
  avatar_url?: string
  role: string
  joined_at: string
  invited_by?: string
}

export interface RoomMembersResponse {
  members: RoomMember[]
  total: number
}

export interface InviteMemberRequest {
  actor: string
  role?: 'member' | 'admin' | 'observer'
}

export interface SendRoomMessageRequest {
  message_type?: string
  payload: unknown
  thread_id?: string
  reply_to?: string
}

export interface RoomMessageItem {
  message_id: string
  sender_actor: string
  message_type: string
  payload: unknown
  thread_id?: string
  reply_to?: string
  reactions: Record<string, unknown>
  is_pinned: boolean
  is_encrypted: boolean
  created_at: string
}

export interface RoomMessageListResponse {
  messages: RoomMessageItem[]
  total: number
}

export interface SendRoomMessageResponse {
  success: boolean
  message_id: string
  room_id: string
}

export interface PinRoomMessageResponse {
  success: boolean
  room_id: string
  message_id: string
  is_pinned: boolean
}

// ==================== Ring 相关 ====================

export interface CreateRingRequest {
  name: string
  ring_type: string
  fanout?: number
  ttl?: number
  interval?: number
  /** Optional brew category filter (brew-recommend only). Alias: brew_category. */
  category?: string
  brew_category?: string
}

export interface RingSummary {
  ring_id: string
  ring_name?: string
  ring_type: string
  peer_count: number
  last_sync_at?: string
  joined_at: string
}

export interface RingDetail {
  ring_id: string
  ring_name?: string
  ring_type: string
  gossip_config?: Record<string, unknown>
  known_peers: string[]
  last_sync_at?: string
  joined_at: string
}

export interface RingListResponse {
  rings: RingSummary[]
  total: number
}

export interface RingPeer {
  actor_url: string
  instance_domain: string
  added_at: string
}

export interface RingPeersResponse {
  peers: RingPeer[]
  total: number
}

export interface AddPeerRequest {
  peer: string
}

// ==================== Trust 策略管理 ====================

/** Effective trust enforcement only — no fake allowlist/min_trust fields. */
export interface TrustPolicyResponse {
  /** What enforce_inbound / enforce_outbound actually apply today */
  enforcement: {
    domain_blocklist: boolean
    rate_limit: boolean
    content_filters: boolean
    allowlist: false
    min_trust_level: false
  }
  notes?: {
    domain_blocklist?: string
    rate_limit?: string
    content_filters?: string
    allowlist?: string
    min_trust_level?: string
  }
  /** Domains with federation_instances.is_blocked = true */
  blocked_domains: string[]
  rate_limit: {
    max_requests_per_window: number
    window_seconds: number
    trusted_multiplier: number
  }
  /** Always empty until a persisted filter table is wired */
  content_filters: unknown[]
  stats: {
    total_instances: number
    trusted_count: number
    unknown_count: number
  }
}

export interface FederationInstance {
  domain: string
  trust_level: number
  software?: string
  version?: string
  blocked: boolean
  first_seen_at: string
  last_fetched_at?: string
}

export interface InstanceListResponse {
  instances: FederationInstance[]
  total: number
}

export interface UpdateTrustRequest {
  domain: string
  trust_level: number
}

export interface ToggleBlockRequest {
  domain: string
  block: boolean
}

// ==================== 文件传输 ====================

export interface InitTransferRequest {
  filename: string
  file_size: number
  mime_type?: string
  checksum?: string
}

export interface TransferSummary {
  transfer_id: string
  channel_id: string
  filename: string
  file_size: number
  mime_type?: string
  status: string
  direction: string
  progress: number
  created_at: string
}

export interface TransferDetail {
  transfer_id: string
  channel_id: string
  filename: string
  file_size: number
  mime_type?: string
  checksum?: string
  status: string
  direction: string
  chunks_total: number
  chunks_received: number
  bytes_transferred: number
  progress: number
  created_at: string
  completed_at?: string
}

export interface TransferListResponse {
  transfers: TransferSummary[]
  total: number
}

export interface UploadChunkRequest {
  chunk_index: number
  chunk_data: string
  chunk_size: number
}
