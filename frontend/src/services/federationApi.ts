/**
 * 联邦 API 服务
 *
 * 所有 REST 方法接受可选的 runtimeGrant（Tapp 宿主代理归因）：
 * 传入时请求携带 X-Tapp-Runtime-Grant 头，服务端归因中间件据此校验
 * Runtime Grant 的 federation.* 权限。宿主自身 UI 调用不传该参数。
 */

import type {
  AddPeerRequest,
  ChannelDetail,
  ChannelListResponse,
  CreateChannelRequest,
  CreateRingRequest,
  CreateRoomRequest,
  FederationIdentity,
  FollowListResponse,
  FollowRequest,
  FollowResponse,
  InitTransferRequest,
  InstanceListResponse,
  InviteMemberRequest,
  MessageListResponse,
  PublishedListResponse,
  PublishRequest,
  PublishResponse,
  RingDetail,
  RingListResponse,
  RingPeersResponse,
  RoomDetail,
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
  UpdateTrustRequest,
  UploadChunkRequest,
} from '../types/federation'
import type { ApiRequestOptions } from './api'
import { apiService } from './api'
import { federationMock } from './federationMock'

const PREFIX = '/federation'

/** Tapp 宿主代理调用的归因请求选项 */
function attributionOptions(
  runtimeGrant?: string,
): ApiRequestOptions | undefined {
  return runtimeGrant
    ? { headers: { 'X-Tapp-Runtime-Grant': runtimeGrant } }
    : undefined
}

/**
 * 开发环境 mock 模式控制。
 * 默认使用真实后端；只有显式打开 mock 时才使用演示数据：
 *   localStorage.setItem('federation-mock', '1')
 *
 * 兼容旧开关：localStorage.setItem('federation-real', '1') 会强制使用真实后端。
 */
function shouldUseMock(): boolean {
  try {
    if (!import.meta.env.DEV) return false
    if (typeof localStorage === 'undefined') return false
    if (localStorage.getItem('federation-real') === '1') return false
    return localStorage.getItem('federation-mock') === '1'
  } catch {
    return false
  }
}

async function withDevFallback<T>(
  realCall: () => Promise<T>,
  mockCall: () => Promise<T>,
): Promise<T> {
  if (shouldUseMock()) return mockCall()
  return realCall()
}

function dispatchMockWsListener(
  listener: EventListenerOrEventListenerObject,
  event: Event,
): void {
  if (typeof listener === 'function') {
    listener(event)
  } else {
    listener.handleEvent(event)
  }
}

/**
 * Mock WebSocket — 模拟已连接状态，不发送/接收真实数据。
 * 通过 MessageChannel 创建一个合法的 WebSocket-like 对象。
 */
function createMockWs(): WebSocket {
  const _listeners: Record<string, EventListenerOrEventListenerObject | null> =
    {}
  let _readyState = 1 // OPEN

  const proxy = Object.create(new EventTarget(), {
    readyState: { get: () => _readyState },
    send: { value: () => {} },
    close: {
      value: () => {
        _readyState = 3
        if (_listeners.close)
          dispatchMockWsListener(_listeners.close, new CloseEvent('close'))
      },
    },
    onopen: {
      get: () => _listeners.open ?? null,
      set: (fn: any) => {
        _listeners.open = fn
      },
      configurable: true,
    },
    onmessage: {
      get: () => _listeners.message ?? null,
      set: (fn: any) => {
        _listeners.message = fn
      },
      configurable: true,
    },
    onclose: {
      get: () => _listeners.close ?? null,
      set: (fn: any) => {
        _listeners.close = fn
      },
      configurable: true,
    },
    onerror: {
      get: () => _listeners.error ?? null,
      set: (fn: any) => {
        _listeners.error = fn
      },
      configurable: true,
    },
    addEventListener: {
      value: (type: string, fn: any) => {
        _listeners[type] = fn
      },
    },
    removeEventListener: {
      value: (type: string, _fn: any) => {
        delete _listeners[type]
      },
    },
  }) as unknown as WebSocket

  setTimeout(() => {
    if (_listeners.open) {
      dispatchMockWsListener(_listeners.open, new Event('open'))
    }
  }, 30)

  return proxy
}

export const federationApi = {
  /** 获取当前用户联邦身份 */
  getIdentity(runtimeGrant?: string): Promise<FederationIdentity> {
    return withDevFallback(
      () =>
        apiService.get<FederationIdentity>(
          `${PREFIX}/identity`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getIdentity(),
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
    return withDevFallback(
      () =>
        apiService.get<FollowListResponse>(
          `${PREFIX}/following`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getFollowing(),
    )
  },

  /** 获取关注我的远程用户 */
  getFollowers(runtimeGrant?: string): Promise<FollowListResponse> {
    return withDevFallback(
      () =>
        apiService.get<FollowListResponse>(
          `${PREFIX}/followers`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getFollowers(),
    )
  },

  // ==================== 时间线 ====================

  /** 获取联邦时间线 */
  getTimeline(runtimeGrant?: string): Promise<TimelineResponse> {
    return withDevFallback(
      () =>
        apiService.get<TimelineResponse>(
          `${PREFIX}/timeline`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getTimeline(),
    )
  },

  // ==================== 内容发布 ====================

  /** 发布内容到联邦网络 */
  publish(req: PublishRequest, runtimeGrant?: string): Promise<PublishResponse> {
    return apiService.post<PublishResponse>(
      `${PREFIX}/publish`,
      req,
      attributionOptions(runtimeGrant),
    )
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
    return withDevFallback(
      () =>
        apiService.get<PublishedListResponse>(
          `${PREFIX}/published`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getPublished(),
    )
  },

  // ==================== Channel 通信 ====================

  /** 获取 Channel 列表 */
  getChannels(runtimeGrant?: string): Promise<ChannelListResponse> {
    return withDevFallback(
      () =>
        apiService.get<ChannelListResponse>(
          `${PREFIX}/channels`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getChannels(),
    )
  },

  /** 创建 Channel */
  createChannel(
    req: CreateChannelRequest,
    runtimeGrant?: string,
  ): Promise<ChannelDetail> {
    return withDevFallback(
      () =>
        apiService.post<ChannelDetail>(
          `${PREFIX}/channels`,
          req,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.createChannel(req) as Promise<ChannelDetail>,
    )
  },

  /** 获取 Channel 详情 */
  getChannel(channelId: string, runtimeGrant?: string): Promise<ChannelDetail> {
    return withDevFallback(
      () =>
        apiService.get<ChannelDetail>(
          `${PREFIX}/channels/${channelId}`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getChannel(channelId),
    )
  },

  /** 关闭 Channel */
  closeChannel(
    channelId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return withDevFallback(
      () =>
        apiService.post<{ success: boolean }>(
          `${PREFIX}/channels/${channelId}/close`,
          {},
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.closeChannel(channelId),
    )
  },

  /** 接受 Channel */
  acceptChannel(
    channelId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return withDevFallback(
      () =>
        apiService.post<{ success: boolean }>(
          `${PREFIX}/channels/${channelId}/accept`,
          {},
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.acceptChannel(channelId),
    )
  },

  /** 获取消息历史 */
  getMessages(
    channelId: string,
    before?: string,
    limit?: number,
    runtimeGrant?: string,
  ): Promise<MessageListResponse> {
    return withDevFallback(
      () => {
        const params = new URLSearchParams()
        if (before) params.set('before', before)
        if (limit) params.set('limit', String(limit))
        const qs = params.toString()
        return apiService.get<MessageListResponse>(
          `${PREFIX}/channels/${channelId}/messages${qs ? `?${qs}` : ''}`,
          attributionOptions(runtimeGrant),
        )
      },
      () => federationMock.getMessages(channelId),
    )
  },

  /** 发送消息 */
  sendMessage(
    channelId: string,
    req: SendMessageRequest,
    runtimeGrant?: string,
  ): Promise<SendMessageResponse> {
    return withDevFallback(
      () =>
        apiService.post<SendMessageResponse>(
          `${PREFIX}/channels/${channelId}/messages`,
          req,
          attributionOptions(runtimeGrant),
        ),
      () =>
        federationMock.sendMessage(channelId, req.payload, req.message_type),
    )
  },

  /**
   * 创建 Channel WebSocket 连接
   *
   * 注意：浏览器 WebSocket 无法携带自定义头，因此该通道不参与
   * X-Tapp-Runtime-Grant 归因（Tapp 宿主代理的 WS 订阅暂以文档豁免，
   * 后续需设计 ticket 查询参数等一次性凭据方案）。
   */
  connectChannelWs(channelId: string): WebSocket {
    if (shouldUseMock()) return createMockWs()
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const base = location.host
    return new WebSocket(
      `${proto}//${base}/api${PREFIX}/channels/${channelId}/ws`,
    )
  },

  // ==================== Room 多方通信 ====================

  /** 获取 Room 列表 */
  getRooms(runtimeGrant?: string): Promise<RoomListResponse> {
    return withDevFallback(
      () =>
        apiService.get<RoomListResponse>(
          `${PREFIX}/rooms`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getRooms(),
    )
  },

  /** 创建 Room */
  createRoom(
    req: CreateRoomRequest,
    runtimeGrant?: string,
  ): Promise<RoomDetail> {
    return withDevFallback(
      () =>
        apiService.post<RoomDetail>(
          `${PREFIX}/rooms`,
          req,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.createRoom(req) as Promise<RoomDetail>,
    )
  },

  /** 更新 Room */
  updateRoom(
    roomId: string,
    req: UpdateRoomRequest,
    runtimeGrant?: string,
  ): Promise<RoomDetail> {
    return withDevFallback(
      () =>
        apiService.put<RoomDetail>(
          `${PREFIX}/rooms/${roomId}`,
          req,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.updateRoom(roomId, req as Record<string, unknown>),
    )
  },

  /** 获取 Room 详情 */
  getRoom(roomId: string, runtimeGrant?: string): Promise<RoomDetail> {
    return withDevFallback(
      () =>
        apiService.get<RoomDetail>(
          `${PREFIX}/rooms/${roomId}`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getRoom(roomId),
    )
  },

  /** 获取 Room 成员 */
  getRoomMembers(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<RoomMembersResponse> {
    return withDevFallback(
      () =>
        apiService.get<RoomMembersResponse>(
          `${PREFIX}/rooms/${roomId}/members`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getRoomMembers(roomId),
    )
  },

  /** 邀请成员 */
  inviteMember(
    roomId: string,
    req: InviteMemberRequest,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return withDevFallback(
      () =>
        apiService.post<{ success: boolean }>(
          `${PREFIX}/rooms/${roomId}/invite`,
          req,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.inviteMember(roomId, req),
    )
  },

  /** 移除成员 */
  removeMember(
    roomId: string,
    actorUrl: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return withDevFallback(
      () =>
        apiService.delete<{ success: boolean }>(
          `${PREFIX}/rooms/${roomId}/members/${encodeURIComponent(actorUrl)}`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.removeMember(roomId, actorUrl),
    )
  },

  /** 离开 Room */
  leaveRoom(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return withDevFallback(
      () =>
        apiService.post<{ success: boolean }>(
          `${PREFIX}/rooms/${roomId}/leave`,
          {},
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.leaveRoom(roomId),
    )
  },

  /** 解散 Room（仅 owner） */
  deleteRoom(
    roomId: string,
    runtimeGrant?: string,
  ): Promise<{ success: boolean }> {
    return withDevFallback(
      () =>
        apiService.delete<{ success: boolean }>(
          `${PREFIX}/rooms/${roomId}`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.deleteRoom(roomId),
    )
  },

  /** 获取 Room 消息 */
  getRoomMessages(
    roomId: string,
    before?: string,
    limit?: number,
    runtimeGrant?: string,
  ): Promise<RoomMessageListResponse> {
    return withDevFallback(
      () => {
        const params = new URLSearchParams()
        if (before) params.set('before', before)
        if (limit) params.set('limit', String(limit))
        const qs = params.toString()
        return apiService.get<RoomMessageListResponse>(
          `${PREFIX}/rooms/${roomId}/messages${qs ? `?${qs}` : ''}`,
          attributionOptions(runtimeGrant),
        )
      },
      () => federationMock.getRoomMessages(roomId),
    )
  },

  /** 发送 Room 消息 */
  sendRoomMessage(
    roomId: string,
    req: SendRoomMessageRequest,
    runtimeGrant?: string,
  ): Promise<SendRoomMessageResponse> {
    return withDevFallback(
      () =>
        apiService.post<SendRoomMessageResponse>(
          `${PREFIX}/rooms/${roomId}/messages`,
          req,
          attributionOptions(runtimeGrant),
        ),
      () =>
        federationMock.sendRoomMessage(roomId, req.payload, req.message_type),
    )
  },

  /** Pin/Unpin Room 消息 */
  pinRoomMessage(
    roomId: string,
    messageId: string,
    pinned: boolean,
    runtimeGrant?: string,
  ): Promise<import('../types/federation').PinRoomMessageResponse> {
    return withDevFallback(
      () =>
        apiService.post<import('../types/federation').PinRoomMessageResponse>(
          `${PREFIX}/rooms/${roomId}/messages/${messageId}/pin`,
          { pinned },
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.pinRoomMessage(roomId, messageId, pinned),
    )
  },

  /**
   * 创建 Room WebSocket 连接
   *
   * 注意：浏览器 WebSocket 无法携带自定义头，归因豁免同 connectChannelWs。
   */
  connectRoomWs(roomId: string): WebSocket {
    if (shouldUseMock()) return createMockWs()
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const base = location.host
    return new WebSocket(`${proto}//${base}/api${PREFIX}/rooms/${roomId}/ws`)
  },

  // ==================== Ring ====================

  /** 获取 Ring 列表 */
  getRings(runtimeGrant?: string): Promise<RingListResponse> {
    return withDevFallback(
      () =>
        apiService.get<RingListResponse>(
          `${PREFIX}/rings`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getRings(),
    )
  },

  /** 创建 Ring */
  createRing(req: CreateRingRequest, runtimeGrant?: string): Promise<RingDetail> {
    return apiService.post<RingDetail>(
      `${PREFIX}/rings`,
      req,
      attributionOptions(runtimeGrant),
    )
  },

  /** 获取 Ring 详情 */
  getRing(ringId: string, runtimeGrant?: string): Promise<RingDetail> {
    return withDevFallback(
      () =>
        apiService.get<RingDetail>(
          `${PREFIX}/rings/${ringId}`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getRing(ringId),
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
    return withDevFallback(
      () =>
        apiService.get<RingPeersResponse>(
          `${PREFIX}/rings/${ringId}/peers`,
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.getRingPeers(ringId),
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
    return withDevFallback(
      () =>
        apiService.post<{
          success: boolean
          synced_peers: number
          entries_count: number
        }>(
          `${PREFIX}/rings/${ringId}/sync`,
          {},
          attributionOptions(runtimeGrant),
        ),
      () => federationMock.triggerSync(ringId),
    )
  },

  // ==================== Trust 策略管理 ====================

  /** 获取信任策略 */
  getTrustPolicy(runtimeGrant?: string): Promise<TrustPolicyResponse> {
    return apiService.get<TrustPolicyResponse>(
      `${PREFIX}/trust/policy`,
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

  // ==================== 文件传输 ====================

  /** 发起文件传输 */
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
}
