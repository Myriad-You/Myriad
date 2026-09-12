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
  /** Present when keys exist; omitted for a cold identity. */
  key_id?: string
  public_key_pem?: string
}

/** POST /api/federation/keys/rotate; confirm:true required. */
export interface FederationKeyRotationResult {
  public_key_pem: string
  key_id: string
  previous_public_key_pem?: string | null
  /** Update(Person) fan-out rows (0 if none / enqueue fail). */
  update_queued: number
  note?: string
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

export interface TimelineItem {
  activity_id: string
  activity_type?: string
  object_type?: string
  content_preview?: string
  content_json?: Record<string, unknown> | null
  object_id?: string
  is_read: boolean
  is_bookmarked?: boolean
  liked_by_me?: boolean
  bookmarked_by_me?: boolean
  announced_by_me?: boolean
  like_count?: number
  bookmark_count?: number
  announce_count?: number
  reply_count?: number
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

export interface NoteAttachmentInput {
  url: string
  media_type: string
  name?: string
}

export interface PublishRequest {
  content_type: 'report' | 'brew-article' | 'tapp' | 'library' | 'note'
  /** Required except freeform notes (server generates id). */
  content_id?: string
  visibility?: 'public' | 'followers' | 'direct'
  /** Freeform body when content_type is note. */
  text?: string
  attachments?: NoteAttachmentInput[]
  in_reply_to?: string
  inReplyTo?: string
}

export interface CreateNoteRequest {
  text?: string
  attachments?: NoteAttachmentInput[]
  visibility?: 'public' | 'followers' | 'direct'
  in_reply_to?: string
  inReplyTo?: string
}

export interface ObjectIdRequest {
  object_id: string
}

/** Backend requires non-empty content. */
export interface AnnounceRequest {
  object_id: string
  content: string
}

export interface InteractionResponse {
  success: boolean
  object_id: string
  kind: string
  activity_id?: string
  liked_by_me?: boolean
  bookmarked_by_me?: boolean
  announced_by_me?: boolean
  like_count?: number
  bookmark_count?: number
  announce_count?: number
  reply_count?: number
}

export interface BookmarkListResponse {
  items: TimelineItem[]
  total: number
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
  delivered_queued?: number
  author_timeline?: boolean
}

export interface UnpublishRequest {
  content_type?: string
  content_id?: string
  activity_id?: string
}

export interface PublishedAttachment {
  url: string
  media_type?: string
  type?: string
  name?: string
}

export interface PublishedItem {
  id: number
  content_type: string
  content_id: string
  activity_id: string
  visibility: string
  published_at: string
  content_preview?: string
  title?: string
  summary?: string
  attachments?: PublishedAttachment[]
  content_json?: Record<string, unknown> | null
  object_id?: string
}

export interface PublishedListResponse {
  items: PublishedItem[]
  total: number
}

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

/** Outbound enqueue result (before the HTTP delivery worker). */
export interface DeliveryEnqueueInfo {
  queued: number
  remote_targets: number
  unresolved?: number
  warning?: string
}

export interface SendMessageResponse {
  success: boolean
  message_id: string
  channel_id: string
  is_encrypted?: boolean
  delivery?: DeliveryEnqueueInfo
}

export interface DeliveryStats {
  pending: number
  delivering: number
  delivered: number
  dead: number
  active: number
  failed: number
}

export interface DeliveryQueueItem {
  id: number
  status: string
  target_domain: string
  target_inbox: string
  attempts: number
  max_attempts: number
  error_message?: string | null
  activity_type: string
  activity_id: string
  created_at: string
  last_attempt_at?: string | null
  next_retry_at?: string | null
  /** Intentional `cancelled:…`; not a peer fail. */
  intentional_cancel?: boolean
  /** Host should offer Retry for this row. */
  retryable?: boolean
  /** RoomDissolve / ChannelClose teardown fan-out. */
  is_teardown_activity?: boolean
}

export interface DeliveryListResponse {
  items: DeliveryQueueItem[]
  total: number
}

export interface CreateRoomRequest {
  name: string
  description?: string
  avatar_url?: string
  governance_type?: 'owner' | 'democratic' | 'open'
  invite_policy?: 'admin-only' | 'member-invite' | 'open'
  max_members?: number
  is_public?: boolean
  game?: {
    tapp_id: string
    protocol: string
    max_players?: number
    max_message_bytes?: number
  }
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
  /** active | pending (remote invite not yet accepted). */
  my_membership_status?: string
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
  /** Includes e2e.published_keys and stickers. */
  shared_data_config?: {
    e2e?: {
      published_keys?: Record<string, string>
      [key: string]: unknown
    }
    stickers?: Array<{
      id: string
      data: string
      name?: string
      actor: string
      created_at: string
    }>
    [key: string]: unknown
  }
  my_role?: string
  /** active | pending (remote invite not yet accepted). */
  my_membership_status?: string
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
  /** active | pending (remote invite not yet accepted). */
  membership_status?: string
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
  encrypt?: boolean
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
  is_encrypted?: boolean
  delivery?: DeliveryEnqueueInfo
}

export interface PinRoomMessageResponse {
  success: boolean
  room_id: string
  message_id: string
  is_pinned: boolean
}

export interface CreateRingRequest {
  name: string
  ring_type: string
  fanout?: number
  ttl?: number
  interval?: number
  /** brew-recommend only. Alias: brew_category. */
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

export interface TrustPolicyResponse {
  enforcement: {
    domain_blocklist: boolean
    rate_limit: boolean
    content_filters: boolean
    allowlist: boolean
    min_trust_level: boolean
  }
  notes?: {
    domain_blocklist?: string
    rate_limit?: string
    content_filters?: string
    allowlist?: string
    min_trust_level?: string
  }
  /** federation_instances.is_blocked = true. */
  blocked_domains: string[]
  /** Empty = allow all non-blocked domains. */
  allowed_domains?: string[]
  /** 0=Unknown … 4=Federated; inbound floor. */
  min_trust_level?: number
  auto_discover?: boolean
  rate_limit: {
    max_requests_per_window: number
    window_seconds: number
    trusted_multiplier: number
  }
  content_filters: unknown[]
  stats: {
    total_instances: number
    trusted_count: number
    unknown_count: number
  }
}

export interface UpdateTrustPolicyRequest {
  min_trust_level?: number
  allowed_domains?: string[]
  auto_discover?: boolean
  /** Inbound rate limit; Trusted+ uses trusted_multiplier. */
  rate_limit?: {
    max_requests_per_window?: number
    window_seconds?: number
    trusted_multiplier?: number
  }
}

export interface ContentFilterItem {
  id: number
  name: string
  filter_type: string
  value: string
  enabled: boolean
  created_at?: string
}

export interface ContentFilterListResponse {
  filters: ContentFilterItem[]
  total: number
}

export interface CreateContentFilterRequest {
  name: string
  filter_type: string
  value: string
  enabled?: boolean
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

export interface InitTransferRequest {
  filename: string
  file_size: number
  mime_type?: string
  checksum?: string
}

export interface TransferSummary {
  transfer_id: string
  channel_id: string
  room_id?: string
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
  /** Set for room transfers; omit for DM. */
  room_id?: string
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

/** Library item; no payload bytes. */
export interface RoomFileItem {
  key: string
  message_id: string
  kind: 'image' | 'file' | string
  filename: string
  size: number
  mime_type?: string
  sender_actor: string
  created_at: string
  transfer_id?: string
  has_inline: boolean
  status: 'ready' | 'pending' | 'missing' | string
}

export interface RoomFileListResponse {
  files: RoomFileItem[]
  total: number
  has_more: boolean
}

export interface ListRoomFilesParams {
  before?: string
  limit?: number
  /** all | image | file */
  filter?: string
  /** Filename substring. */
  q?: string
}

export interface UploadChunkRequest {
  chunk_index: number
  chunk_data: string
  chunk_size: number
}
