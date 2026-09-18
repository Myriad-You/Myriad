/** runtimeGrant → X-Tapp-Runtime-Grant. */

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
import { currentCopy } from '../i18n/localeCopy'
import { formatUserFacingError } from '../utils/formatUserFacingError'
import { apiService } from './api'

const PREFIX = '/federation'

function attributionOptions(
  runtimeGrant?: string,
): ApiRequestOptions | undefined {
  return runtimeGrant
    ? { headers: { 'X-Tapp-Runtime-Grant': runtimeGrant } }
    : undefined
}

export const federationApi = {
  getPublicLimits(): Promise<{
    profile: string
    message_payload_bytes: number
    note_image_bytes: number
    note_video_bytes: number
  }> {
    return apiService.get(`${PREFIX}/public/limits`)
  },

  getIdentity(runtimeGrant?: string): Promise<FederationIdentity> {
    return apiService.get<FederationIdentity>(
      `${PREFIX}/identity`,
      attributionOptions(runtimeGrant),
    )
  },

  /** Body must include confirm: true. */
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

  follow(target: string, runtimeGrant?: string): Promise<FollowResponse> {
    return apiService.post<FollowResponse>(
      `${PREFIX}/follow`,
      { target } satisfies FollowRequest,
      attributionOptions(runtimeGrant),
    )
  },

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

  getFollowing(runtimeGrant?: string): Promise<FollowListResponse> {
    return apiService.get<FollowListResponse>(
      `${PREFIX}/following`,
      attributionOptions(runtimeGrant),
    )
  },

  getFollowers(runtimeGrant?: string): Promise<FollowListResponse> {
    return apiService.get<FollowListResponse>(
      `${PREFIX}/followers`,
      attributionOptions(runtimeGrant),
    )
  },

  getTimeline(runtimeGrant?: string): Promise<TimelineResponse> {
    return apiService.get<TimelineResponse>(
      `${PREFIX}/timeline`,
      attributionOptions(runtimeGrant),
    )
  },

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

  like(objectId: string, runtimeGrant?: string): Promise<InteractionResponse> {
    return apiService.post<InteractionResponse>(
      `${PREFIX}/like`,
      { object_id: objectId } satisfies ObjectIdRequest,
      attributionOptions(runtimeGrant),
    )
  },

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

  getBookmarks(runtimeGrant?: string): Promise<BookmarkListResponse> {
    return apiService.get<BookmarkListResponse>(
      `${PREFIX}/bookmarks`,
      attributionOptions(runtimeGrant),
    )
  },

  getObject(
    objectId: string,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    object_id: string
    object: Record<string, unknown>
    source: string
    actor?: Record<string, unknown> | null
  }> {
    const base = attributionOptions(runtimeGrant) ?? {}
    return apiService.get(`${PREFIX}/objects`, {
      ...base,
      params: { id: objectId },
    })
  },

  /** Commentary must be non-empty. */
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
        await formatUserFacingError(
          (errBody as { error?: string; message?: string }).error ||
            (errBody as { message?: string }).message ||
            `Media upload failed: ${response.status}`,
          currentCopy().errors.federationMediaUploadFailed,
        ),
      )
    }
    return (await response.json()) as MediaUploadResponse
  },

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

  getPublished(runtimeGrant?: string): Promise<PublishedListResponse> {
    return apiService.get<PublishedListResponse>(
      `${PREFIX}/published`,
      attributionOptions(runtimeGrant),
    )
  },

  getChannels(runtimeGrant?: string): Promise<ChannelListResponse> {
    return apiService.get<ChannelListResponse>(
      `${PREFIX}/channels`,
      attributionOptions(runtimeGrant),
    )
  },

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

  getChannel(channelId: string, runtimeGrant?: string): Promise<ChannelDetail> {
    return apiService.get<ChannelDetail>(
      `${PREFIX}/channels/${channelId}`,
      attributionOptions(runtimeGrant),
    )
  },

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

  deleteChannel(
    channelId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${PREFIX}/channels/${channelId}`,
      attributionOptions(runtimeGrant),
    )
  },

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

  /** Requires federation:message; pass tapp_ws_ticket. */
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

  getRooms(runtimeGrant?: string): Promise<RoomListResponse> {
    return apiService.get<RoomListResponse>(
      `${PREFIX}/rooms`,
      attributionOptions(runtimeGrant),
    )
  },

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

  getRoom(roomId: string, runtimeGrant?: string): Promise<RoomDetail> {
    return apiService.get<RoomDetail>(
      `${PREFIX}/rooms/${roomId}`,
      attributionOptions(runtimeGrant),
    )
  },

  getRoomMembers(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<RoomMembersResponse> {
    return apiService.get<RoomMembersResponse>(
      `${PREFIX}/rooms/${roomId}/members`,
      attributionOptions(runtimeGrant),
    )
  },

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

  setMemberRole(
    roomId: string,
    actorUrl: string,
    role: 'admin' | 'member',
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    room_id?: string
    actor?: string
    role?: string
  }> {
    return apiService.put(
      `${PREFIX}/rooms/${roomId}/members/${encodeURIComponent(actorUrl)}/role`,
      { role },
      attributionOptions(runtimeGrant),
    )
  },

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

  deleteRoom(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${PREFIX}/rooms/${roomId}`,
      attributionOptions(runtimeGrant),
    )
  },

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

  /** Requires federation:message; pass tapp_ws_ticket. */
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

  getRings(runtimeGrant?: string): Promise<RingListResponse> {
    return apiService.get<RingListResponse>(
      `${PREFIX}/rings`,
      attributionOptions(runtimeGrant),
    )
  },

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

  getRing(ringId: string, runtimeGrant?: string): Promise<RingDetail> {
    return apiService.get<RingDetail>(
      `${PREFIX}/rings/${ringId}`,
      attributionOptions(runtimeGrant),
    )
  },

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

  getRingPeers(
    ringId: string,
    runtimeGrant?: string,
  ): Promise<RingPeersResponse> {
    return apiService.get<RingPeersResponse>(
      `${PREFIX}/rings/${ringId}/peers`,
      attributionOptions(runtimeGrant),
    )
  },

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

  getTrustPolicy(runtimeGrant?: string): Promise<TrustPolicyResponse> {
    return apiService.get<TrustPolicyResponse>(
      `${PREFIX}/trust/policy`,
      attributionOptions(runtimeGrant),
    )
  },

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

  getInstances(runtimeGrant?: string): Promise<InstanceListResponse> {
    return apiService.get<InstanceListResponse>(
      `${PREFIX}/trust/instances`,
      attributionOptions(runtimeGrant),
    )
  },

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

  listTransfers(
    channelId: string,
    runtimeGrant?: string,
  ): Promise<TransferListResponse> {
    return apiService.get<TransferListResponse>(
      `${PREFIX}/channels/${channelId}/transfers`,
      attributionOptions(runtimeGrant),
    )
  },

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

  listRoomTransfers(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<TransferListResponse> {
    return apiService.get<TransferListResponse>(
      `${PREFIX}/rooms/${roomId}/transfers`,
      attributionOptions(runtimeGrant),
    )
  },

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

  getTransfer(
    transferId: string,
    runtimeGrant?: string,
  ): Promise<TransferDetail> {
    return apiService.get<TransferDetail>(
      `${PREFIX}/transfers/${transferId}`,
      attributionOptions(runtimeGrant),
    )
  },

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

  initiateChannelE2e(
    channelId: string,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    channel_id: string
    /** HTTP snake_case; WS/bridge also expose publicKey. */
    public_key: string
    publicKey?: string
    algorithm: string
    established: boolean
  }> {
    return apiService.post(
      `${PREFIX}/channels/${channelId}/e2e/key-exchange`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  initiateRoomE2e(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    room_id: string
    /** HTTP snake_case; WS/bridge also expose publicKey. */
    public_key: string
    publicKey?: string
    algorithm: string
    published_key_count: number
  }> {
    return apiService.post(
      `${PREFIX}/rooms/${roomId}/e2e/key-exchange`,
      {},
      attributionOptions(runtimeGrant),
    )
  },

  addRoomSticker(
    roomId: string,
    req: { data: string; name?: string },
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    room_id: string
    stickers: Array<{
      id: string
      data: string
      name?: string
      actor: string
      created_at: string
    }>
  }> {
    return apiService.post(
      `${PREFIX}/rooms/${roomId}/stickers`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  removeRoomSticker(
    roomId: string,
    stickerId: string,
    runtimeGrant?: string,
  ): Promise<{
    success: boolean
    room_id: string
    stickers: Array<{
      id: string
      data: string
      name?: string
      actor: string
      created_at: string
    }>
  }> {
    return apiService.delete(
      `${PREFIX}/rooms/${roomId}/stickers/${encodeURIComponent(stickerId)}`,
      attributionOptions(runtimeGrant),
    )
  },

  downloadTransfer(
    transferId: string,
    runtimeGrant?: string,
  ): Promise<{ blob: Blob; filename?: string; contentType?: string }> {
    return apiService.getBlob(`${PREFIX}/transfers/${transferId}/content`, {
      ...attributionOptions(runtimeGrant),
      // 10 min
      timeout: 600_000,
    })
  },

  getDeliveryStats(
    runtimeGrant?: string,
  ): Promise<import('../types/federation').DeliveryStats> {
    return apiService.get<import('../types/federation').DeliveryStats>(
      `${PREFIX}/delivery/stats`,
      attributionOptions(runtimeGrant),
    )
  },

  listDelivery(
    limit?: number,
    runtimeGrant?: string,
    status?: string,
  ): Promise<import('../types/federation').DeliveryListResponse> {
    const params = new URLSearchParams()
    if (limit != null) params.set('limit', String(limit))
    if (status) params.set('status', status)
    const qs = params.toString()
    return apiService.get<import('../types/federation').DeliveryListResponse>(
      `${PREFIX}/delivery${qs ? `?${qs}` : ''}`,
      attributionOptions(runtimeGrant),
    )
  },

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

  /** Capped; skips cancelled:*. */
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

  joinRoom(
    roomId: string,
    runtimeGrant?: string,
    options?: { home_server?: string },
  ): Promise<{
    success: boolean
    room_id?: string
    membership_status?: string
    role?: string
    already_member?: boolean
  }> {
    const body =
      options?.home_server && options.home_server.trim()
        ? { home_server: options.home_server.trim() }
        : {}
    return apiService.post(
      `${PREFIX}/rooms/${encodeURIComponent(roomId)}/join`,
      body,
      attributionOptions(runtimeGrant),
    )
  },
}
