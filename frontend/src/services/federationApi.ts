/**
 * 联邦 API 服务
 *
 * 所有 REST 方法接受可选的 runtimeGrant（Tapp 宿主代理归因）：
 * 传入时请求携带 X-Tapp-Runtime-Grant 头，服务端归因中间件据此校验
 * Runtime Grant 的 federation.* 权限。宿主自身 UI 调用不传该参数。
 */

import type {
  AddPeerRequest,
  AnnounceRequest,
  BookmarkListResponse,
  ChannelDetail,
  ChannelListResponse,
  ContentFilterListResponse,
  CreateChannelRequest,
  CreateContentFilterRequest,
  CreateNoteRequest,
  CreateRingRequest,
  CreateRoomRequest,
  FederationIdentity,
  FederationKeyRotationResult,
  FollowListResponse,
  FollowRequest,
  FollowResponse,
  InitTransferRequest,
  InstanceListResponse,
  InteractionResponse,
  InviteMemberRequest,
  ListRoomFilesParams,
  MediaUploadResponse,
  MessageListResponse,
  ObjectIdRequest,
  PublishedListResponse,
  PublishRequest,
  PublishResponse,
  RingDetail,
  RingListResponse,
  RingPeersResponse,
  RoomDetail,
  RoomFileListResponse,
  RoomListResponse,
  RoomMembersResponse,
  RoomMessageListResponse,
  SendMessageRequest,
  SendMessageResponse,
  SendRoomMessageRequest,
  SendRoomMessageResponse,
  TimelineResponse,
  ToggleBlockRequest,
  TransferDetail,
  TransferListResponse,
  TrustPolicyResponse,
  UnpublishRequest,
  UpdateRoomRequest,
  UpdateTrustPolicyRequest,
  UpdateTrustRequest,
  UploadChunkRequest,
} from '../types/federation'
import type { ApiRequestOptions } from './api'
import { apiService } from './api'

const PREFIX = '/federation'

/** Tapp 宿主代理调用的归因请求选项 */
function attributionOptions(
  runtimeGrant?: string,
): ApiRequestOptions | undefined {
  return runtimeGrant
    ? { headers: { 'X-Tapp-Runtime-Grant': runtimeGrant } }
    : undefined
}

export const federationApi = {
  /** 获取当前用户联邦身份 */
  getIdentity(runtimeGrant?: string): Promise<FederationIdentity> {
    return apiService.get<FederationIdentity>(
      `${PREFIX}/identity`,
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * Explicit federation signing-key rotation.
   * Body must include `confirm: true` (server rejects otherwise).
   * Replaces RSA keypair + best-effort Update(Person) fan-out.
   */
  rotateKeys(
    body: { confirm: true },
    runtimeGrant?: string,
  ): Promise<FederationKeyRotationResult> {
    return apiService.post<FederationKeyRotationResult>(
      `${PREFIX}/keys/rotate`,
      body,
      attributionOptions(runtimeGrant),
    )
  },

  // ==================== 关注管理 ====================

  /** 关注远程用户 */
  follow(target: string, runtimeGrant?: string): Promise<FollowResponse> {
    return apiService.post<FollowResponse>(
      `${PREFIX}/follow`,
      { target } satisfies FollowRequest,
      attributionOptions(runtimeGrant),
    )
  },

  /** 取消关注 */
  unfollow(
    target: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/unfollow`,
      { target } satisfies FollowRequest,
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取我关注的远程用户 */
  getFollowing(runtimeGrant?: string): Promise<FollowListResponse> {
    return apiService.get<FollowListResponse>(
      `${PREFIX}/following`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取关注我的远程用户 */
  getFollowers(runtimeGrant?: string): Promise<FollowListResponse> {
    return apiService.get<FollowListResponse>(
      `${PREFIX}/followers`,
      attributionOptions(runtimeGrant),
    )
  },

  // ==================== 时间线 ====================

  /** 获取联邦时间线 */
  getTimeline(runtimeGrant?: string): Promise<TimelineResponse> {
    return apiService.get<TimelineResponse>(
      `${PREFIX}/timeline`,
      attributionOptions(runtimeGrant),
    )
  },

  // ==================== 内容发布 ====================

  /** 发布内容到联邦网络 */
  publish(
    req: PublishRequest,
    runtimeGrant?: string,
  ): Promise<PublishResponse> {
    return apiService.post<PublishResponse>(
      `${PREFIX}/publish`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 创建 freeform Note（文本 + 图片/视频附件） */
  createNote(
    req: CreateNoteRequest,
    runtimeGrant?: string,
  ): Promise<PublishResponse> {
    return apiService.post<PublishResponse>(
      `${PREFIX}/notes`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** Like an AP object (Note id / URL) */
  like(objectId: string, runtimeGrant?: string): Promise<InteractionResponse> {
    return apiService.post<InteractionResponse>(
      `${PREFIX}/like`,
      { object_id: objectId } satisfies ObjectIdRequest,
      attributionOptions(runtimeGrant),
    )
  },

  /** Unlike an AP object */
  unlike(
    objectId: string,
    runtimeGrant?: string,
  ): Promise<InteractionResponse> {
    return apiService.post<InteractionResponse>(
      `${PREFIX}/unlike`,
      { object_id: objectId } satisfies ObjectIdRequest,
      attributionOptions(runtimeGrant),
    )
  },

  /** Bookmark (local-first) */
  bookmark(
    objectId: string,
    runtimeGrant?: string,
  ): Promise<InteractionResponse> {
    return apiService.post<InteractionResponse>(
      `${PREFIX}/bookmark`,
      { object_id: objectId } satisfies ObjectIdRequest,
      attributionOptions(runtimeGrant),
    )
  },

  /** Remove bookmark */
  unbookmark(
    objectId: string,
    runtimeGrant?: string,
  ): Promise<InteractionResponse> {
    return apiService.post<InteractionResponse>(
      `${PREFIX}/unbookmark`,
      { object_id: objectId } satisfies ObjectIdRequest,
      attributionOptions(runtimeGrant),
    )
  },

  /** List bookmarked posts for current user */
  getBookmarks(runtimeGrant?: string): Promise<BookmarkListResponse> {
    return apiService.get<BookmarkListResponse>(
      `${PREFIX}/bookmarks`,
      attributionOptions(runtimeGrant),
    )
  },

  /** Quote-repost an object (requires non-empty commentary). */
  announce(
    objectId: string,
    content?: string,
    runtimeGrant?: string,
  ): Promise<InteractionResponse> {
    return apiService.post<InteractionResponse>(
      `${PREFIX}/announce`,
      {
        object_id: objectId,
        content: content ?? '',
      } satisfies AnnounceRequest,
      attributionOptions(runtimeGrant),
    )
  },

  /** Undo announce / unrepost */
  unannounce(
    objectId: string,
    runtimeGrant?: string,
  ): Promise<InteractionResponse> {
    return apiService.post<InteractionResponse>(
      `${PREFIX}/unannounce`,
      { object_id: objectId } satisfies ObjectIdRequest,
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * 上传联邦媒体（multipart）。返回可嵌入 AP attachment 的公开 URL。
   * 不经过 apiService JSON Content-Type，以便浏览器设置 multipart boundary。
   */
  async uploadMedia(
    file: Blob,
    options?: { filename?: string; runtimeGrant?: string },
  ): Promise<MediaUploadResponse> {
    const { API_URL } = await import('../config')
    const { getCSRFToken } = await import('../utils/csrf')
    const formData = new FormData()
    const filename =
      options?.filename ||
      (file instanceof File && file.name ? file.name : 'upload.bin')
    formData.append('file', file, filename)

    const headers: Record<string, string> = {}
    const csrf = await getCSRFToken()
    if (csrf) headers['X-CSRF-Token'] = csrf
    if (options?.runtimeGrant) {
      headers['X-Tapp-Runtime-Grant'] = options.runtimeGrant
    }

    const response = await fetch(`${API_URL}/api${PREFIX}/media`, {
      method: 'POST',
      headers,
      body: formData,
      credentials: 'include',
    })
    if (!response.ok) {
      const errBody = await response.json().catch(() => ({}))
      throw new Error(
        (errBody as { error?: string; message?: string }).error ||
          (errBody as { message?: string }).message ||
          `Media upload failed: ${response.status}`,
      )
    }
    return (await response.json()) as MediaUploadResponse
  },

  /** 取消发布 */
  unpublish(
    req: UnpublishRequest,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/unpublish`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取已发布内容列表 */
  getPublished(runtimeGrant?: string): Promise<PublishedListResponse> {
    return apiService.get<PublishedListResponse>(
      `${PREFIX}/published`,
      attributionOptions(runtimeGrant),
    )
  },

  // ==================== Channel 通信 ====================

  /** 获取 Channel 列表 */
  getChannels(runtimeGrant?: string): Promise<ChannelListResponse> {
    return apiService.get<ChannelListResponse>(
      `${PREFIX}/channels`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 创建 Channel */
  createChannel(
    req: CreateChannelRequest,
    runtimeGrant?: string,
  ): Promise<ChannelDetail> {
    return apiService.post<ChannelDetail>(
      `${PREFIX}/channels`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取 Channel 详情 */
  getChannel(channelId: string, runtimeGrant?: string): Promise<ChannelDetail> {
    return apiService.get<ChannelDetail>(
      `${PREFIX}/channels/${channelId}`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 关闭 Channel */
  closeChannel(
    channelId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/channels/${channelId}/close`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /** 删除已关闭的 Channel（本地硬删除） */
  deleteChannel(
    channelId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${PREFIX}/channels/${channelId}`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 接受 Channel */
  acceptChannel(
    channelId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/channels/${channelId}/accept`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取消息历史 */
  getMessages(
    channelId: string,
    before?: string,
    limit?: number,
    runtimeGrant?: string,
  ): Promise<MessageListResponse> {
    const params = new URLSearchParams()
    if (before) params.set('before', before)
    if (limit) params.set('limit', String(limit))
    const qs = params.toString()
    return apiService.get<MessageListResponse>(
      `${PREFIX}/channels/${channelId}/messages${qs ? `?${qs}` : ''}`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 发送消息 */
  sendMessage(
    channelId: string,
    req: SendMessageRequest,
    runtimeGrant?: string,
  ): Promise<SendMessageResponse> {
    return apiService.post<SendMessageResponse>(
      `${PREFIX}/channels/${channelId}/messages`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * Mint a one-time federation channel WebSocket ticket.
   *
   * Requires a runtime grant with `federation:message`. Present the returned
   * ticket as `tapp_ws_ticket` on {@link connectChannelWs}. Host UI does not
   * call this — it connects without a ticket.
   */
  mintChannelWsTicket(
    channelId: string,
    runtimeGrant: string,
  ): Promise<{ ticket: string; expiresAt: string }> {
    return apiService.post<{ ticket: string; expiresAt: string }>(
      `${PREFIX}/channels/${channelId}/ws-ticket`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * Create a Channel WebSocket connection.
   *
   * Browsers cannot send custom headers on WebSocket. Tapp traffic mints a
   * short-lived ticket via {@link mintChannelWsTicket} (REST + grant header)
   * and passes it here; the server consumes it at upgrade and attributes the
   * connection. Host UI callers omit `ticket` and authenticate with cookie/JWT only.
   */
  connectChannelWs(channelId: string, ticket?: string): WebSocket {
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const base = location.host
    const qs =
      ticket && ticket.length > 0
        ? `?tapp_ws_ticket=${encodeURIComponent(ticket)}`
        : ''
    return new WebSocket(
      `${proto}//${base}/api${PREFIX}/channels/${channelId}/ws${qs}`,
    )
  },

  // ==================== Room 多方通信 ====================

  /** 获取 Room 列表 */
  getRooms(runtimeGrant?: string): Promise<RoomListResponse> {
    return apiService.get<RoomListResponse>(
      `${PREFIX}/rooms`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 创建 Room */
  createRoom(
    req: CreateRoomRequest,
    runtimeGrant?: string,
  ): Promise<RoomDetail> {
    return apiService.post<RoomDetail>(
      `${PREFIX}/rooms`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 更新 Room */
  updateRoom(
    roomId: string,
    req: UpdateRoomRequest,
    runtimeGrant?: string,
  ): Promise<RoomDetail> {
    return apiService.put<RoomDetail>(
      `${PREFIX}/rooms/${roomId}`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取 Room 详情 */
  getRoom(roomId: string, runtimeGrant?: string): Promise<RoomDetail> {
    return apiService.get<RoomDetail>(
      `${PREFIX}/rooms/${roomId}`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取 Room 成员 */
  getRoomMembers(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<RoomMembersResponse> {
    return apiService.get<RoomMembersResponse>(
      `${PREFIX}/rooms/${roomId}/members`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 邀请成员 */
  inviteMember(
    roomId: string,
    req: InviteMemberRequest,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/rooms/${roomId}/invite`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 接受群组邀请（pending → active） */
  acceptRoomInvite(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean; membership_status?: string }> {
    return apiService.post<{ success: boolean; membership_status?: string }>(
      `${PREFIX}/rooms/${roomId}/accept`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /** 拒绝群组邀请 */
  rejectRoomInvite(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/rooms/${roomId}/reject`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /** 移除成员 */
  removeMember(
    roomId: string,
    actorUrl: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${PREFIX}/rooms/${roomId}/members/${encodeURIComponent(actorUrl)}`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 离开 Room */
  leaveRoom(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/rooms/${roomId}/leave`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /** 解散 Room（仅 owner） */
  deleteRoom(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${PREFIX}/rooms/${roomId}`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取 Room 消息 */
  getRoomMessages(
    roomId: string,
    before?: string,
    limit?: number,
    runtimeGrant?: string,
  ): Promise<RoomMessageListResponse> {
    const params = new URLSearchParams()
    if (before) params.set('before', before)
    if (limit) params.set('limit', String(limit))
    const qs = params.toString()
    return apiService.get<RoomMessageListResponse>(
      `${PREFIX}/rooms/${roomId}/messages${qs ? `?${qs}` : ''}`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 发送 Room 消息 */
  sendRoomMessage(
    roomId: string,
    req: SendRoomMessageRequest,
    runtimeGrant?: string,
  ): Promise<SendRoomMessageResponse> {
    return apiService.post<SendRoomMessageResponse>(
      `${PREFIX}/rooms/${roomId}/messages`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** Pin/Unpin Room 消息 */
  pinRoomMessage(
    roomId: string,
    messageId: string,
    pinned: boolean,
    runtimeGrant?: string,
  ): Promise<import('../types/federation').PinRoomMessageResponse> {
    return apiService.post<
      import('../types/federation').PinRoomMessageResponse
    >(
      `${PREFIX}/rooms/${roomId}/messages/${messageId}/pin`,
      { pinned },
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * Mint a one-time federation room WebSocket ticket.
   *
   * Requires a runtime grant with `federation:message`. Present the returned
   * ticket as `tapp_ws_ticket` on {@link connectRoomWs}.
   */
  mintRoomWsTicket(
    roomId: string,
    runtimeGrant: string,
  ): Promise<{ ticket: string; expiresAt: string }> {
    return apiService.post<{ ticket: string; expiresAt: string }>(
      `${PREFIX}/rooms/${roomId}/ws-ticket`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * Create a Room WebSocket connection.
   *
   * Same ticket handshake as {@link connectChannelWs}: Tapp passes a ticket;
   * host UI connects ticket-less.
   */
  connectRoomWs(roomId: string, ticket?: string): WebSocket {
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const base = location.host
    const qs =
      ticket && ticket.length > 0
        ? `?tapp_ws_ticket=${encodeURIComponent(ticket)}`
        : ''
    return new WebSocket(
      `${proto}//${base}/api${PREFIX}/rooms/${roomId}/ws${qs}`,
    )
  },

  // ==================== Ring ====================

  /** 获取 Ring 列表 */
  getRings(runtimeGrant?: string): Promise<RingListResponse> {
    return apiService.get<RingListResponse>(
      `${PREFIX}/rings`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 创建 Ring */
  createRing(
    req: CreateRingRequest,
    runtimeGrant?: string,
  ): Promise<RingDetail> {
    return apiService.post<RingDetail>(
      `${PREFIX}/rings`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取 Ring 详情 */
  getRing(ringId: string, runtimeGrant?: string): Promise<RingDetail> {
    return apiService.get<RingDetail>(
      `${PREFIX}/rings/${ringId}`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 离开 Ring */
  leaveRing(
    ringId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/rings/${ringId}/leave`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取 Ring Peer 列表 */
  getRingPeers(
    ringId: string,
    runtimeGrant?: string,
  ): Promise<RingPeersResponse> {
    return apiService.get<RingPeersResponse>(
      `${PREFIX}/rings/${ringId}/peers`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 添加 Peer */
  addPeer(
    ringId: string,
    req: AddPeerRequest,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/rings/${ringId}/peers`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 移除 Peer */
  removePeer(
    ringId: string,
    peerUrl: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${PREFIX}/rings/${ringId}/peers/${encodeURIComponent(peerUrl)}`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 触发 Gossip 同步 */
  triggerSync(
    ringId: string,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    synced_peers: number
    entries_count: number
  }> {
    return apiService.post<{
      success: boolean
      synced_peers: number
      entries_count: number
    }>(`${PREFIX}/rings/${ringId}/sync`, {}, attributionOptions(runtimeGrant))
  },

  // ==================== Trust 策略管理 ====================

  /** 获取信任策略 */
  getTrustPolicy(runtimeGrant?: string): Promise<TrustPolicyResponse> {
    return apiService.get<TrustPolicyResponse>(
      `${PREFIX}/trust/policy`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 更新实例级策略（allowlist / min_trust / auto_discover / rate limit）— admin */
  updateTrustPolicy(
    req: UpdateTrustPolicyRequest,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    min_trust_level?: number
    allowed_domains?: string[]
    auto_discover?: boolean
    rate_limit?: {
      max_requests_per_window: number
      window_seconds: number
      trusted_multiplier: number
    }
  }> {
    return apiService.put(
      `${PREFIX}/trust/policy`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 列出所有已知实例 */
  getInstances(runtimeGrant?: string): Promise<InstanceListResponse> {
    return apiService.get<InstanceListResponse>(
      `${PREFIX}/trust/instances`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 更新实例信任层级 */
  updateInstanceTrust(
    req: UpdateTrustRequest,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/trust/update`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 封禁/解封实例 */
  toggleInstanceBlock(
    req: ToggleBlockRequest,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/trust/block`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 内容过滤规则列表（admin） */
  listContentFilters(
    runtimeGrant?: string,
  ): Promise<ContentFilterListResponse> {
    return apiService.get<ContentFilterListResponse>(
      `${PREFIX}/trust/filters`,
      attributionOptions(runtimeGrant),
    )
  },

  createContentFilter(
    req: CreateContentFilterRequest,
    runtimeGrant?: string,
  ): Promise<{ success: boolean; id: number }> {
    return apiService.post(
      `${PREFIX}/trust/filters`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  updateContentFilter(
    id: number,
    req: Partial<CreateContentFilterRequest & { enabled: boolean }>,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.put(
      `${PREFIX}/trust/filters/${id}`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  deleteContentFilter(
    id: number,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.delete(
      `${PREFIX}/trust/filters/${id}`,
      attributionOptions(runtimeGrant),
    )
  },

  // ==================== 文件传输 ====================

  /** 发起文件传输（私信 Channel） */
  initiateTransfer(
    channelId: string,
    req: InitTransferRequest,
    runtimeGrant?: string,
  ): Promise<TransferDetail> {
    return apiService.post<TransferDetail>(
      `${PREFIX}/channels/${channelId}/transfers`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 列出 Channel 的文件传输 */
  listTransfers(
    channelId: string,
    runtimeGrant?: string,
  ): Promise<TransferListResponse> {
    return apiService.get<TransferListResponse>(
      `${PREFIX}/channels/${channelId}/transfers`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 发起文件传输（群聊 Room） */
  initiateRoomTransfer(
    roomId: string,
    req: InitTransferRequest,
    runtimeGrant?: string,
  ): Promise<TransferDetail> {
    return apiService.post<TransferDetail>(
      `${PREFIX}/rooms/${roomId}/transfers`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 列出 Room 的文件传输 */
  listRoomTransfers(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<TransferListResponse> {
    return apiService.get<TransferListResponse>(
      `${PREFIX}/rooms/${roomId}/transfers`,
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * Group attachment library index (messages + local transfers).
   * Does not return payload.data; download via transfer_id or live chat payload.
   */
  listRoomFiles(
    roomId: string,
    params?: ListRoomFilesParams,
    runtimeGrant?: string,
  ): Promise<RoomFileListResponse> {
    const qs = new URLSearchParams()
    if (params?.before) qs.set('before', params.before)
    if (params?.limit != null) qs.set('limit', String(params.limit))
    if (params?.filter) qs.set('filter', params.filter)
    if (params?.q) qs.set('q', params.q)
    const query = qs.toString()
    return apiService.get<RoomFileListResponse>(
      `${PREFIX}/rooms/${roomId}/files${query ? `?${query}` : ''}`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取传输详情 */
  getTransfer(
    transferId: string,
    runtimeGrant?: string,
  ): Promise<TransferDetail> {
    return apiService.get<TransferDetail>(
      `${PREFIX}/transfers/${transferId}`,
      attributionOptions(runtimeGrant),
    )
  },

  /** 上传文件分块 */
  uploadChunk(
    transferId: string,
    req: UploadChunkRequest,
    runtimeGrant?: string,
  ): Promise<{ success: boolean; progress: number }> {
    return apiService.post<{ success: boolean; progress: number }>(
      `${PREFIX}/transfers/${transferId}/chunks`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 取消传输 */
  cancelTransfer(
    transferId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.post<{ success: boolean }>(
      `${PREFIX}/transfers/${transferId}/cancel`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /** Transfer room ownership to another member (actor URL or local username) */
  transferRoomOwnership(
    roomId: string,
    newOwner: string,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    room_id: string
    previous_owner: string
    new_owner: string
  }> {
    return apiService.post(
      `${PREFIX}/rooms/${roomId}/transfer-ownership`,
      { new_owner: newOwner },
      attributionOptions(runtimeGrant),
    )
  },

  /** Start channel E2E key exchange (publishes local pubkey to peer) */
  initiateChannelE2e(
    channelId: string,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    channel_id: string
    public_key: string
    algorithm: string
    established: boolean
  }> {
    return apiService.post(
      `${PREFIX}/channels/${channelId}/e2e/key-exchange`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /** Publish room E2E public key to other members */
  initiateRoomE2e(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    room_id: string
    public_key: string
    algorithm: string
    published_key_count: number
  }> {
    return apiService.post(
      `${PREFIX}/rooms/${roomId}/e2e/key-exchange`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * Download completed transfer bytes.
   * - Channel (DM): channel owner
   * - Room (group): any room member on this instance
   * Used for file-meta messages that only store transfer_id in payload.
   * Routed via /api/* so production Myriad proxy already forwards to backend;
   * response is streamed (do not buffer in outer proxies).
   */
  downloadTransfer(
    transferId: string,
    runtimeGrant?: string,
  ): Promise<{ blob: Blob; filename?: string; contentType?: string }> {
    return apiService.getBlob(`${PREFIX}/transfers/${transferId}/content`, {
      ...attributionOptions(runtimeGrant),
      // Large files: 10 min
      timeout: 600_000,
    })
  },

  /** Outbound delivery queue stats for the current user */
  getDeliveryStats(
    runtimeGrant?: string,
  ): Promise<import('../types/federation').DeliveryStats> {
    return apiService.get<import('../types/federation').DeliveryStats>(
      `${PREFIX}/delivery/stats`,
      attributionOptions(runtimeGrant),
    )
  },

  /** Recent delivery queue rows (dead first) */
  listDelivery(
    limit?: number,
    runtimeGrant?: string,
  ): Promise<import('../types/federation').DeliveryListResponse> {
    const qs =
      limit != null ? `?limit=${encodeURIComponent(String(limit))}` : ''
    return apiService.get<import('../types/federation').DeliveryListResponse>(
      `${PREFIX}/delivery${qs}`,
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * Re-queue a single dead/stuck delivery item.
   * When the prior error was user cancel, server sets `revived_cancelled: true`.
   */
  retryDelivery(
    queueId: number,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    id: number
    status: string
    previous_status?: string
    revived_cancelled?: boolean
  }> {
    return apiService.post(
      `${PREFIX}/delivery/${queueId}/retry`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /** Cancel a pending/delivering delivery item (marks dead) */
  cancelDelivery(
    queueId: number,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    id: number
    status: string
    previous_status?: string
    already?: boolean
  }> {
    return apiService.post(
      `${PREFIX}/delivery/${queueId}/cancel`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * Re-queue all dead delivery items (capped).
   * Server skips `cancelled:*` rows and reports `skipped_cancelled`.
   */
  retryAllDeadDelivery(
    limit?: number,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    retried: number
    skipped_cancelled?: number
    limit?: number
  }> {
    const qs =
      limit != null ? `?limit=${encodeURIComponent(String(limit))}` : ''
    return apiService.post(
      `${PREFIX}/delivery/retry-dead${qs}`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /** Cancel all pending/delivering delivery items (capped) */
  cancelAllPendingDelivery(
    limit?: number,
    runtimeGrant?: string,
  ): Promise<{ success: boolean; cancelled: number }> {
    const qs =
      limit != null ? `?limit=${encodeURIComponent(String(limit))}` : ''
    return apiService.post(
      `${PREFIX}/delivery/cancel-pending${qs}`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * Dismiss (hard-delete) a single dead delivery queue row.
   * Pending/delivering/delivered are rejected by the server.
   */
  dismissDelivery(
    queueId: number,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    id: number
    dismissed?: boolean
    was_cancelled?: boolean
    previous_status?: string
  }> {
    return apiService.delete(
      `${PREFIX}/delivery/${queueId}`,
      attributionOptions(runtimeGrant),
    )
  },

  /**
   * Bulk-delete dead delivery rows (capped).
   * When `cancelledOnly`, only `cancelled:%` error rows are removed.
   */
  purgeDeadDelivery(
    opts?: { limit?: number; cancelledOnly?: boolean },
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    purged: number
    limit?: number
    cancelled_only?: boolean
  }> {
    const params = new URLSearchParams()
    if (opts?.limit != null) params.set('limit', String(opts.limit))
    if (opts?.cancelledOnly) params.set('cancelled_only', 'true')
    const qs = params.toString() ? `?${params.toString()}` : ''
    return apiService.post(
      `${PREFIX}/delivery/purge-dead${qs}`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  /** Self-join an open-policy room */
  joinRoom(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean; membership_status?: string }> {
    return apiService.post(
      `${PREFIX}/rooms/${roomId}/join`,
      {},
      attributionOptions(runtimeGrant),
    )
  },
}
