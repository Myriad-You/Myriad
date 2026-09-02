/**
 * Federation Bridge — Tapp 运行时联邦能力桥接
 *
 * 为 Tapp 沙箱提供联邦 API 访问能力
 * 通过 TappBridge 消息机制暴露受限的联邦操作
 *
 * 支持的操作域：
 * - federation.getIdentity / rotateKeys — 联邦身份与显式密钥轮换（confirm:true）
 * - federation.timeline — 读取联邦时间线
 * - federation.follow / unfollow — 关注管理
 * - federation.channels — Channel 读取与消息发送
 * - federation.rooms — Room 读取与消息发送
 * - federation.rings — Ring 信息读取
 * - federation.publish / unpublish — 内容发布管理
 * - federation.trust — 实例信任策略管理
 * - federation.delivery* — stats/list/retry/cancel/dismiss/purge（bulk retry 跳过 cancelled: dead）
 * - federation.transfers — 文件传输
 * - federation.subscribeChannel / subscribeRoom — WS 实时事件订阅
 *   (mint one-time `tapp_ws_ticket` via grant-authenticated REST, then upgrade)
 */

import type { ComposeXShareRequest } from '../../services/xShareApi'
import type { TappInstance, TappMessage } from '../types'
import type { TappBridge } from './TappBridge'
import { currentCopy } from '../../i18n/localeCopy'
import { ApiError } from '../../services/api'
import { federationApi } from '../../services/federationApi'
import { xShareApi } from '../../services/xShareApi'
import { isKnownGuest } from '../../utils/authState'
import { userFacingError } from '../../utils/userFacingError'
import {
  getFederationFeed,
  getFederationRoomsFeed,
} from '../services/TappApiService'
import {
  federationMediaUrlRejectionReason,
  isValidFederationMediaUrl,
} from '../utils/federationMediaUrl'

/**
 * 访客闸 —— 确定未登录时，per-user 的联邦读取不必真发请求。
 *
 * 背景：Aro 这类后台通知型 Tapp 会以 core 模式在首页常驻轮询
 * getChannels/getRooms。访客根本没有私信和房间，这些请求只可能拿到
 * 401，却每 15 秒重复一次、每个访客每个页面都来一遍——网络面板和
 * 服务端日志里全是红的，还白白占限流额度。
 *
 * 刻意只盖 Channel/Room 的读取：它们天然是「我的会话」，语义上访客一定
 * 为空；时间线、关注列表、Ring 这些可能有公开语义，不在此列。写操作也
 * 不盖——那些由用户动作触发，不会空转，真失败了该让调用方看见原因。
 *
 * fail-open：只有 isKnownGuest() 为 true（/api/auth/me 给出确定答案）
 * 才短路；状态未知一律照旧走网络，绝不误挡真实用户。鉴权仍在后端。
 */
/**
 * 返回「空成功」而非错误：访客没有会话是正常状态，不是故障。
 * Aro 的 loadConversations 在两个列表都 reject 时会弹错误横幅，
 * 空列表才是它期望的访客表现。
 */
function guestEmptyChannels() {
  return { success: true as const, data: { channels: [], total: 0 } }
}

function guestEmptyRooms() {
  return { success: true as const, data: { rooms: [], total: 0 } }
}

function missingArg() {
  return {
    success: false as const,
    error: currentCopy().errors.agentInputEmpty,
  }
}

function opFailed(fallback = currentCopy().errors.federationActionFailed) {
  return {
    success: false as const,
    error: fallback,
  }
}

/** Map API failures for Tapp sandbox — preserve ROOM_INVITE_PENDING etc. */
function federationFail(error: unknown, fallback: string) {
  if (error instanceof ApiError) {
    return {
      success: false as const,
      error: userFacingError(error, fallback),
      code: error.code,
      status: error.status,
      // Pending invite: Aro can show accept/reject instead of generic 403
      membership_status:
        error.code === 'ROOM_INVITE_PENDING' ? ('pending' as const) : undefined,
    }
  }
  return {
    success: false as const,
    error: userFacingError(error, fallback),
  }
}

/** Convert a data URL or raw base64 string to a Blob for multipart upload. */
function dataUrlOrBase64ToBlob(data: string, fallbackMime: string): Blob {
  let mime = fallbackMime
  let b64 = data
  const dataUrlMatch = /^data:([^;,]+)?(;base64)?,(.*)$/s.exec(data)
  if (dataUrlMatch) {
    mime = dataUrlMatch[1] || fallbackMime
    b64 = dataUrlMatch[3] || ''
  }
  const binary = atob(b64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return new Blob([bytes], { type: mime })
}

/**
 * 注册联邦处理器到 TappBridge
 *
 * 返回 cleanup 函数 — 调用以关闭由该 bridge 持有的所有 WebSocket 订阅。
 */
export function registerFederationHandlers(
  bridge: TappBridge,
  _tappInstance: TappInstance,
): () => void {
  // 此 bridge 持有的实时订阅
  const channelSockets = new Map<string, WebSocket>()
  const roomSockets = new Map<string, WebSocket>()

  const safeClose = (ws: WebSocket): void => {
    try {
      ws.close()
    } catch {
      /* ignore */
    }
  }

  const closeAllSockets = (): void => {
    for (const ws of channelSockets.values()) safeClose(ws)
    channelSockets.clear()
    for (const ws of roomSockets.values()) safeClose(ws)
    roomSockets.clear()
  }

  /** Normalize WS/HTTP publicKey vs public_key for E2E consumers. */
  const pickPublicKey = (data: Record<string, unknown>): string | undefined => {
    const camel = data.publicKey
    const snake = data.public_key
    if (typeof camel === 'string' && camel) return camel
    if (typeof snake === 'string' && snake) return snake
    return undefined
  }

  const attachChannelWs = (channelId: string, ws: WebSocket): void => {
    ws.addEventListener('message', (ev) => {
      if (channelSockets.get(channelId) !== ws) return
      try {
        const data = JSON.parse(typeof ev.data === 'string' ? ev.data : '')
        bridge.emit('federation:message', {
          scope: 'channel',
          channelId,
          data,
        })
        if (data && typeof data === 'object') {
          if (data.type === 'channel_closed') {
            bridge.emit('federation:channelUpdate', {
              channelId,
              event: 'closed',
            })
          } else if (data.type === 'channel_accepted') {
            // Remote accepted our pending ChannelOpen — unlock Aro composer.
            bridge.emit('federation:channelUpdate', {
              channelId,
              event: 'accepted',
            })
          } else if (data.type === 'key_exchange') {
            // Typed E2E key fan-out (WS uses publicKey; HTTP uses public_key).
            const publicKey = pickPublicKey(data as Record<string, unknown>)
            bridge.emit('federation:channelUpdate', {
              channelId,
              event: 'key_exchange',
              from: data.from,
              publicKey,
              public_key: publicKey,
              algorithm: data.algorithm,
              established: data.established,
              direction: data.direction,
            })
          }
        }
      } catch {
        /* ignore non-JSON */
      }
    })
    ws.addEventListener('close', () => {
      if (channelSockets.get(channelId) !== ws) return
      channelSockets.delete(channelId)
      bridge.emit('federation:channelUpdate', {
        channelId,
        event: 'disconnected',
      })
    })
  }

  const attachRoomWs = (roomId: string, ws: WebSocket): void => {
    ws.addEventListener('message', (ev) => {
      if (roomSockets.get(roomId) !== ws) return
      try {
        const data = JSON.parse(typeof ev.data === 'string' ? ev.data : '')
        bridge.emit('federation:message', { scope: 'room', roomId, data })
        if (data && typeof data === 'object') {
          if (data.event === 'governance_changed') {
            bridge.emit('federation:roomUpdate', {
              roomId,
              event: 'governance_changed',
              changes: data.changes,
            })
          } else if (data.event === 'stickers_changed') {
            bridge.emit('federation:roomUpdate', {
              roomId,
              event: 'stickers_changed',
              stickers: data.stickers,
              actor: data.actor,
              op: data.op,
            })
          } else if (data.type === 'room_deleted') {
            bridge.emit('federation:roomUpdate', {
              roomId,
              event: 'deleted',
            })
          } else if (data.type === 'key_exchange') {
            // Room E2E: refresh published_keys via typed roomUpdate (not raw message only).
            const publicKey = pickPublicKey(data as Record<string, unknown>)
            bridge.emit('federation:roomUpdate', {
              roomId,
              event: 'key_exchange',
              from: data.from,
              publicKey,
              public_key: publicKey,
              algorithm: data.algorithm,
              published_key_count: data.published_key_count,
              direction: data.direction,
            })
          } else if (
            data.event === 'member_joined' ||
            data.event === 'member_left' ||
            data.event === 'member_removed' ||
            data.event === 'member_invited'
          ) {
            bridge.emit('federation:roomUpdate', {
              roomId,
              event: data.event,
              actor: data.actor,
              role: data.role,
            })
          }
        }
      } catch {
        /* ignore non-JSON */
      }
    })
    ws.addEventListener('close', () => {
      if (roomSockets.get(roomId) !== ws) return
      roomSockets.delete(roomId)
      bridge.emit('federation:roomUpdate', { roomId, event: 'disconnected' })
    })
  }

  // 身份

  bridge.registerHandler('federation.getIdentity', async () => {
    // Prefer grant-attributed call; session-only works for this endpoint and
    // unblocks Aro after runtime-grant destroy/mint failures.
    try {
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getIdentity(runtimeGrant)
        return { success: true, data }
      } catch {
        const data = await federationApi.getIdentity()
        return { success: true, data }
      }
    } catch (error) {
      return {
        success: false,
        error:
          userFacingError(error),
      }
    }
  })

  /**
   * Explicit key rotation — mirrors POST /api/federation/keys/rotate.
   * Payload: [confirm] where confirm must be boolean true (UI confirm gate).
   */
  bridge.registerHandler(
    'federation.rotateKeys',
    async (message: TappMessage) => {
      const [confirmRaw] = (message.payload as { args: unknown[] }).args || []
      if (confirmRaw !== true) {
        return {
          success: false,
          error: currentCopy().errors.stepNeedsConfirm,
        }
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.rotateKeys(
          { confirm: true },
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return federationFail(error, currentCopy().errors.federationKeyRotateFailed)
      }
    },
  )

  // 时间线

  bridge.registerHandler('federation.getFeed', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await getFederationFeed(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('federation.getRoomsFeed', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await getFederationRoomsFeed(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error:
          userFacingError(error),
      }
    }
  })

  bridge.registerHandler('federation.getTimeline', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getTimeline(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error:
          userFacingError(error),
      }
    }
  })

  bridge.registerHandler(
    'federation.getObject',
    async (message: TappMessage) => {
      const [objectId] = (message.payload as { args: unknown[] }).args || []
      if (!objectId || typeof objectId !== 'string') {
        return missingArg()
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getObject(objectId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  // 关注管理

  bridge.registerHandler('federation.follow', async (message: TappMessage) => {
    const [target] = (message.payload as { args: unknown[] }).args || []
    if (!target || typeof target !== 'string')
      return missingArg()
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.follow(target, runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler(
    'federation.unfollow',
    async (message: TappMessage) => {
      const [target] = (message.payload as { args: unknown[] }).args || []
      if (!target || typeof target !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.unfollow(target, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler('federation.getFollowing', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getFollowing(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('federation.getFollowers', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getFollowers(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  // 内容发布

  bridge.registerHandler('federation.publish', async (message: TappMessage) => {
    const [req] = (message.payload as { args: unknown[] }).args || []
    if (!req) return missingArg()
    try {
      const publishReq = req as Parameters<typeof federationApi.publish>[0]
      const atts = publishReq.attachments
      if (Array.isArray(atts)) {
        for (const att of atts) {
          const url =
            att && typeof att === 'object'
              ? (att as { url?: string }).url
              : undefined
          const reason = federationMediaUrlRejectionReason(url)
          if (reason) {
            console.error(
              '[FederationBridge] publish rejected attachment URL',
              { url, reason },
            )
            return { success: false, error: currentCopy().errors.invalidUrl }
          }
        }
      }
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.publish(publishReq, runtimeGrant)
      if (!data || data.success === false) {
        console.error('[FederationBridge] publish returned unsuccessful', data)
        return {
          success: false,
          error: currentCopy().errors.federationPublishFailed,
        }
      }
      return { success: true, data }
    } catch (error) {
      console.error('[FederationBridge] publish failed', error)
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler(
    'federation.createNote',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return missingArg()
      try {
        const noteReq = req as Parameters<typeof federationApi.createNote>[0]
        // Align with backend federation/limits.rs hard caps (fail early)
        const NOTE_TEXT_CHAR_LIMIT = 100_000
        const NOTE_ATTACHMENT_COUNT_LIMIT = 32
        const text = typeof noteReq.text === 'string' ? noteReq.text : ''
        if ([...text].length > NOTE_TEXT_CHAR_LIMIT) {
          return {
            success: false,
            error: currentCopy().errors.agentInputTooLong,
            max_text_chars: NOTE_TEXT_CHAR_LIMIT,
          }
        }
        const atts = noteReq.attachments
        if (Array.isArray(atts)) {
          if (atts.length > NOTE_ATTACHMENT_COUNT_LIMIT) {
            return {
              success: false,
              error: currentCopy().errors.writeItemsOverCap,
              max_attachments: NOTE_ATTACHMENT_COUNT_LIMIT,
            }
          }
          for (const att of atts) {
            const url =
              att && typeof att === 'object'
                ? (att as { url?: string }).url
                : undefined
            const reason = federationMediaUrlRejectionReason(url)
            if (reason) {
              console.error(
                '[FederationBridge] createNote rejected attachment URL',
                { url, reason },
              )
              return { success: false, error: currentCopy().errors.invalidUrl }
            }
          }
        }
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.createNote(noteReq, runtimeGrant)
        if (!data || data.success === false) {
          console.error('[FederationBridge] createNote returned unsuccessful', data)
          return {
            success: false,
            error: currentCopy().errors.federationPublishFailed,
          }
        }
        return { success: true, data }
      } catch (error) {
        console.error('[FederationBridge] createNote failed', error)
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  const objectIdHandler =
    (
      action: string,
      fn: (
        objectId: string,
        runtimeGrant?: string,
      ) => Promise<unknown>,
    ) =>
    async (message: TappMessage) => {
      const [objectId] = (message.payload as { args: unknown[] }).args || []
      if (!objectId || typeof objectId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await fn(objectId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return federationFail(error, currentCopy().errors.federationActionFailed)
      }
    }

  bridge.registerHandler(
    'federation.like',
    objectIdHandler('like', (id, g) => federationApi.like(id, g)),
  )
  bridge.registerHandler(
    'federation.unlike',
    objectIdHandler('unlike', (id, g) => federationApi.unlike(id, g)),
  )
  bridge.registerHandler(
    'federation.bookmark',
    objectIdHandler('bookmark', (id, g) => federationApi.bookmark(id, g)),
  )
  bridge.registerHandler(
    'federation.unbookmark',
    objectIdHandler('unbookmark', (id, g) => federationApi.unbookmark(id, g)),
  )
  bridge.registerHandler('federation.announce', async (message: TappMessage) => {
    const args = (message.payload as { args: unknown[] }).args || []
    const objectId = args[0]
    const content = typeof args[1] === 'string' ? args[1] : ''
    if (!objectId || typeof objectId !== 'string')
      return missingArg()
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.announce(objectId, content, runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error:
          userFacingError(error),
      }
    }
  })
  bridge.registerHandler(
    'federation.unannounce',
    objectIdHandler('unannounce', (id, g) => federationApi.unannounce(id, g)),
  )

  bridge.registerHandler('federation.getBookmarks', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getBookmarks(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error:
          userFacingError(error),
      }
    }
  })

  // External share intent (X Web Intent only)
  // Compose share text + intent_url. Never posts server-side; user opens intent_url.

  bridge.registerHandler('federation.getExternalShareStatus', async () => {
    try {
      const data = await xShareApi.getStatus()
      return { success: true, data }
    } catch (error) {
      return federationFail(error, currentCopy().errors.federationShareFailed)
    }
  })

  bridge.registerHandler(
    'federation.composeExternalShare',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object') {
        return missingArg()
      }
      const body = req as ComposeXShareRequest
      const hasText =
        typeof body.text === 'string' && body.text.trim().length > 0
      const hasTitle =
        typeof body.title === 'string' && body.title.trim().length > 0
      const hasSummary =
        typeof body.summary === 'string' && body.summary.trim().length > 0
      if (!hasText && !hasTitle && !hasSummary) {
        return {
          success: false,
          error: currentCopy().errors.agentInputEmpty,
        }
      }
      try {
        const data = await xShareApi.compose({
          text: body.text,
          title: body.title,
          summary: body.summary,
          url: body.url,
          hashtags: Array.isArray(body.hashtags) ? body.hashtags : undefined,
          max_length:
            typeof body.max_length === 'number' ? body.max_length : undefined,
        })
        // Explicitly refuse post mode if backend ever changes (defense in depth).
        if (data && (data as { can_post?: boolean }).can_post === true) {
          return {
            success: false,
            error: currentCopy().errors.agentUnsupported,
          }
        }
        if (!data?.intent_url || data.mode !== 'intent') {
          return {
            success: false,
            error: currentCopy().errors.federationShareFailed,
          }
        }
        return { success: true, data }
      } catch (error) {
        return federationFail(error, currentCopy().errors.federationShareFailed)
      }
    },
  )

  bridge.registerHandler(
    'federation.uploadMedia',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return missingArg()
      const body = req as {
        data?: string
        name?: string
        mime?: string
        media_type?: string
      }
      if (!body.data || typeof body.data !== 'string') {
        return missingArg()
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const blob = dataUrlOrBase64ToBlob(
          body.data,
          body.mime || body.media_type || 'application/octet-stream',
        )
        const data = await federationApi.uploadMedia(blob, {
          filename: body.name || 'upload.bin',
          runtimeGrant,
        })
        if (!data?.url || !isValidFederationMediaUrl(data.url)) {
          const reason =
            federationMediaUrlRejectionReason(data?.url) ||
            'Upload response missing a valid media URL'
          console.error('[FederationBridge] uploadMedia bad URL in response', {
            data,
            reason,
          })
          return { success: false, error: currentCopy().errors.invalidUrl }
        }
        return { success: true, data }
      } catch (error) {
        console.error('[FederationBridge] uploadMedia failed', error)
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.unpublish',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req)
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.unpublish(
          req as Parameters<typeof federationApi.unpublish>[0],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler('federation.getPublished', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getPublished(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  // Channel

  bridge.registerHandler('federation.getChannels', async () => {
    try {
      // Channels require a durable user session. Guests would only get 401s.
      // 改用 authState 的权威结论：此前这里读 hasSessionHint()，那是
      // localStorage 启发式，fail-closed——已登录但清过 localStorage 的
      // 用户会拿到空会话列表。现在只有确定是访客才短路，未知一律放行。
      if (isKnownGuest()) return guestEmptyChannels()
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getChannels(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      // Expired JWT with a stale session hint still 401s — treat as empty for guests.
      const status =
        error &&
        typeof error === 'object' &&
        'status' in error &&
        typeof (error as { status: unknown }).status === 'number'
          ? (error as { status: number }).status
          : undefined
      if (status === 401 || status === 403) {
        return { success: true, data: { channels: [], total: 0 } }
      }
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler(
    'federation.createChannel',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req)
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.createChannel(
          req as Parameters<typeof federationApi.createChannel>[0],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.acceptChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.acceptChannel(channelId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.acceptRoomInvite',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.acceptRoomInvite(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return federationFail(error, currentCopy().errors.inviteInvalid)
      }
    },
  )

  bridge.registerHandler(
    'federation.rejectRoomInvite',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.rejectRoomInvite(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return federationFail(error, currentCopy().errors.inviteInvalid)
      }
    },
  )

  bridge.registerHandler(
    'federation.closeChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.closeChannel(channelId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.deleteChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.deleteChannel(channelId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.getChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getChannel(channelId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.getMessages',
    async (message: TappMessage) => {
      const [channelId, before, limit] =
        (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getMessages(
          channelId,
          before as string | undefined,
          limit as number | undefined,
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.sendMessage',
    async (message: TappMessage) => {
      const [channelId, req] =
        (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string' || !req)
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.sendMessage(
          channelId,
          req as Parameters<typeof federationApi.sendMessage>[1],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  // Room

  bridge.registerHandler('federation.getRooms', async () => {
    try {
      // 与 getChannels 同理：房间是 per-user 的，访客必然 401。
      // 线上实测这里被 Aro 的后台轮询每 15s 打一次，每个访客每个页面都在红。
      if (isKnownGuest()) return guestEmptyRooms()
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getRooms(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      // 会话过期（hint 还在）同样按空列表处理，与 getChannels 对齐
      const status =
        error &&
        typeof error === 'object' &&
        'status' in error &&
        typeof (error as { status: unknown }).status === 'number'
          ? (error as { status: number }).status
          : undefined
      if (status === 401 || status === 403) return guestEmptyRooms()
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('federation.getRoom', async (message: TappMessage) => {
    const [roomId] = (message.payload as { args: unknown[] }).args || []
    if (!roomId || typeof roomId !== 'string')
      return missingArg()
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getRoom(roomId, runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler(
    'federation.createRoom',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req)
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.createRoom(
          req as Parameters<typeof federationApi.createRoom>[0],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.updateRoom',
    async (message: TappMessage) => {
      const [roomId, req] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      if (!req)
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.updateRoom(
          roomId,
          req as Parameters<typeof federationApi.updateRoom>[1],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.getRoomMembers',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getRoomMembers(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.inviteMember',
    async (message: TappMessage) => {
      const [roomId, req] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string' || !req) {
        return missingArg()
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.inviteMember(
          roomId,
          req as Parameters<typeof federationApi.inviteMember>[1],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.removeMember',
    async (message: TappMessage) => {
      const [roomId, actorUrl] =
        (message.payload as { args: unknown[] }).args || []
      if (
        !roomId ||
        typeof roomId !== 'string' ||
        !actorUrl ||
        typeof actorUrl !== 'string'
      ) {
        return missingArg()
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.removeMember(roomId, actorUrl, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.setMemberRole',
    async (message: TappMessage) => {
      const [roomId, actorUrl, role] =
        (message.payload as { args: unknown[] }).args || []
      if (
        !roomId ||
        typeof roomId !== 'string' ||
        !actorUrl ||
        typeof actorUrl !== 'string' ||
        (role !== 'admin' && role !== 'member')
      ) {
        return missingArg()
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.setMemberRole(
          roomId,
          actorUrl,
          role,
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.leaveRoom',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.leaveRoom(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.transferRoomOwnership',
    async (message: TappMessage) => {
      const [roomId, newOwner] =
        (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      if (!newOwner || typeof newOwner !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.transferRoomOwnership(
          roomId,
          newOwner,
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.initiateChannelE2e',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.initiateChannelE2e(
          channelId,
          runtimeGrant,
        )
        // HTTP uses public_key; dual-emit camelCase + typed channelUpdate for UI refresh.
        const publicKey =
          (data as { public_key?: string; publicKey?: string }).public_key ||
          (data as { publicKey?: string }).publicKey
        const normalized = { ...data, publicKey, public_key: publicKey }
        bridge.emit('federation:channelUpdate', {
          channelId,
          event: 'key_exchange',
          publicKey,
          public_key: publicKey,
          algorithm: data.algorithm,
          established: data.established,
          direction: 'outbound',
        })
        return { success: true, data: normalized }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.initiateRoomE2e',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.initiateRoomE2e(roomId, runtimeGrant)
        // HTTP snake_case → dual publicKey; typed roomUpdate so UI can refresh published_keys.
        const publicKey =
          (data as { public_key?: string; publicKey?: string }).public_key ||
          (data as { publicKey?: string }).publicKey
        const normalized = { ...data, publicKey, public_key: publicKey }
        bridge.emit('federation:roomUpdate', {
          roomId,
          event: 'key_exchange',
          publicKey,
          public_key: publicKey,
          algorithm: data.algorithm,
          published_key_count: data.published_key_count,
          direction: 'outbound',
        })
        return { success: true, data: normalized }
      } catch (error) {
        return federationFail(error, 'Failed to initiate room E2E')
      }
    },
  )

  bridge.registerHandler(
    'federation.addRoomSticker',
    async (message: TappMessage) => {
      const [roomId, req] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      if (!req || typeof req !== 'object')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.addRoomSticker(
          roomId,
          req as { data: string; name?: string },
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.removeRoomSticker',
    async (message: TappMessage) => {
      const [roomId, stickerId] =
        (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      if (!stickerId || typeof stickerId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.removeRoomSticker(
          roomId,
          stickerId,
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.deleteRoom',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.deleteRoom(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.getRoomMessages',
    async (message: TappMessage) => {
      const [roomId, before, limit] =
        (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getRoomMessages(
          roomId,
          before as string | undefined,
          limit as number | undefined,
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return federationFail(error, 'Failed to load room messages')
      }
    },
  )

  bridge.registerHandler(
    'federation.sendRoomMessage',
    async (message: TappMessage) => {
      const [roomId, req] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string' || !req)
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.sendRoomMessage(
          roomId,
          req as Parameters<typeof federationApi.sendRoomMessage>[1],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return federationFail(error, 'Failed to send room message')
      }
    },
  )

  // Pin Room Message

  bridge.registerHandler(
    'federation.pinRoomMessage',
    async (message: TappMessage) => {
      const [roomId, messageId, pinned] =
        (message.payload as { args: unknown[] }).args || []
      if (
        !roomId ||
        typeof roomId !== 'string' ||
        !messageId ||
        typeof messageId !== 'string'
      ) {
        return missingArg()
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.pinRoomMessage(
          roomId,
          messageId,
          !!pinned,
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  // Ring (只读)

  bridge.registerHandler('federation.getRings', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getRings(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('federation.getRing', async (message: TappMessage) => {
    const [ringId] = (message.payload as { args: unknown[] }).args || []
    if (!ringId || typeof ringId !== 'string')
      return missingArg()
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getRing(ringId, runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler(
    'federation.getRingPeers',
    async (message: TappMessage) => {
      const [ringId] = (message.payload as { args: unknown[] }).args || []
      if (!ringId || typeof ringId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getRingPeers(ringId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.createRing',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.createRing(req as any, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.leaveRing',
    async (message: TappMessage) => {
      const [ringId] = (message.payload as { args: unknown[] }).args || []
      if (!ringId || typeof ringId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.leaveRing(ringId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler('federation.addPeer', async (message: TappMessage) => {
    const [ringId, req] = (message.payload as { args: unknown[] }).args || []
    if (!ringId || typeof ringId !== 'string')
      return missingArg()
    if (!req || typeof req !== 'object')
      return missingArg()
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.addPeer(ringId, req as any, runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler(
    'federation.removePeer',
    async (message: TappMessage) => {
      const [ringId, peerUrl] =
        (message.payload as { args: unknown[] }).args || []
      if (!ringId || typeof ringId !== 'string')
        return missingArg()
      if (!peerUrl || typeof peerUrl !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.removePeer(ringId, peerUrl, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.triggerSync',
    async (message: TappMessage) => {
      const [ringId] = (message.payload as { args: unknown[] }).args || []
      if (!ringId || typeof ringId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.triggerSync(ringId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  // Trust 策略管理

  bridge.registerHandler('federation.getTrustPolicy', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getTrustPolicy(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler(
    'federation.updateTrustPolicy',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.updateTrustPolicy(
          req as Parameters<typeof federationApi.updateTrustPolicy>[0],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler('federation.getDeliveryStats', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getDeliveryStats(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler(
    'federation.listDelivery',
    async (message: TappMessage) => {
      const [limit] = (message.payload as { args: unknown[] }).args || []
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.listDelivery(
          typeof limit === 'number' ? limit : undefined,
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.retryDelivery',
    async (message: TappMessage) => {
      const [queueIdRaw] = (message.payload as { args: unknown[] }).args || []
      const queueId =
        typeof queueIdRaw === 'number'
          ? queueIdRaw
          : typeof queueIdRaw === 'string'
            ? Number.parseInt(queueIdRaw, 10)
            : Number.NaN
      if (!Number.isFinite(queueId) || queueId <= 0)
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.retryDelivery(queueId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.cancelDelivery',
    async (message: TappMessage) => {
      const [queueIdRaw] = (message.payload as { args: unknown[] }).args || []
      // Sandbox may pass string ids from data attributes; accept number | numeric string.
      const queueId =
        typeof queueIdRaw === 'number'
          ? queueIdRaw
          : typeof queueIdRaw === 'string'
            ? Number.parseInt(queueIdRaw, 10)
            : Number.NaN
      if (!Number.isFinite(queueId) || queueId <= 0)
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.cancelDelivery(queueId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.retryAllDeadDelivery',
    async (message: TappMessage) => {
      const [limit] = (message.payload as { args: unknown[] }).args || []
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.retryAllDeadDelivery(
          typeof limit === 'number' ? limit : undefined,
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.cancelAllPendingDelivery',
    async (message: TappMessage) => {
      const [limit] = (message.payload as { args: unknown[] }).args || []
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.cancelAllPendingDelivery(
          typeof limit === 'number' ? limit : undefined,
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.dismissDelivery',
    async (message: TappMessage) => {
      const [queueIdRaw] = (message.payload as { args: unknown[] }).args || []
      const queueId =
        typeof queueIdRaw === 'number'
          ? queueIdRaw
          : typeof queueIdRaw === 'string'
            ? Number.parseInt(queueIdRaw, 10)
            : Number.NaN
      if (!Number.isFinite(queueId) || queueId <= 0)
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.dismissDelivery(queueId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.purgeDeadDelivery',
    async (message: TappMessage) => {
      const [optsRaw] = (message.payload as { args: unknown[] }).args || []
      const opts =
        optsRaw && typeof optsRaw === 'object' && !Array.isArray(optsRaw)
          ? (optsRaw as { limit?: number; cancelledOnly?: boolean })
          : undefined
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.purgeDeadDelivery(
          {
            limit: typeof opts?.limit === 'number' ? opts.limit : undefined,
            cancelledOnly: opts?.cancelledOnly === true,
          },
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.joinRoom',
    async (message: TappMessage) => {
      const args = (message.payload as { args: unknown[] }).args || []
      const roomId = args[0]
      const opts = args[1]
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      const home_server =
        opts &&
        typeof opts === 'object' &&
        opts !== null &&
        typeof (opts as { home_server?: unknown }).home_server === 'string'
          ? (opts as { home_server: string }).home_server
          : undefined
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.joinRoom(
          roomId,
          runtimeGrant,
          home_server ? { home_server } : undefined,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler('federation.getInstances', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getInstances(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler(
    'federation.updateInstanceTrust',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.updateInstanceTrust(
          req as Parameters<typeof federationApi.updateInstanceTrust>[0],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.toggleInstanceBlock',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.toggleInstanceBlock(
          req as Parameters<typeof federationApi.toggleInstanceBlock>[0],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  // 文件传输

  bridge.registerHandler(
    'federation.initiateTransfer',
    async (message: TappMessage) => {
      const [channelId, req] =
        (message.payload as { args: unknown[] }).args || []
      if (
        !channelId ||
        typeof channelId !== 'string' ||
        !req ||
        typeof req !== 'object'
      ) {
        return missingArg()
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.initiateTransfer(
          channelId,
          req as Parameters<typeof federationApi.initiateTransfer>[1],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.listTransfers',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.listTransfers(channelId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.initiateRoomTransfer',
    async (message: TappMessage) => {
      const [roomId, req] =
        (message.payload as { args: unknown[] }).args || []
      if (
        !roomId ||
        typeof roomId !== 'string' ||
        !req ||
        typeof req !== 'object'
      ) {
        return missingArg()
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.initiateRoomTransfer(
          roomId,
          req as Parameters<typeof federationApi.initiateRoomTransfer>[1],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.listRoomTransfers',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.listRoomTransfers(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.listRoomFiles',
    async (message: TappMessage) => {
      const [roomId, params] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.listRoomFiles(
          roomId,
          params && typeof params === 'object'
            ? (params as {
                before?: string
                limit?: number
                filter?: string
                q?: string
              })
            : undefined,
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.getTransfer',
    async (message: TappMessage) => {
      const [transferId] = (message.payload as { args: unknown[] }).args || []
      if (!transferId || typeof transferId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getTransfer(transferId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  /**
   * Download a completed channel transfer and trigger a browser save dialog
   * in the host document (sandbox cannot reliably stream multi-MB blobs alone).
   */
  bridge.registerHandler(
    'federation.downloadTransfer',
    async (message: TappMessage) => {
      const [transferId] = (message.payload as { args: unknown[] }).args || []
      if (!transferId || typeof transferId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        // Prefer status check for clearer errors before streaming large bodies
        try {
          const meta = await federationApi.getTransfer(transferId, runtimeGrant)
          if (meta && meta.status && meta.status !== 'completed') {
            return opFailed(currentCopy().errors.transferNotReady)
          }
        } catch {
          // fall through — content endpoint will return a precise error
        }

        const { blob, filename, contentType } =
          await federationApi.downloadTransfer(transferId, runtimeGrant)
        const name =
          filename ||
          `transfer-${transferId.slice(0, 8)}` ||
          'download'

        const objectUrl = URL.createObjectURL(blob)
        try {
          const a = document.createElement('a')
          a.href = objectUrl
          a.download = name
          a.rel = 'noopener'
          a.style.display = 'none'
          document.body.appendChild(a)
          a.click()
          a.remove()
        } finally {
          // Revoke after the browser has a chance to start the download
          setTimeout(() => URL.revokeObjectURL(objectUrl), 60_000)
        }

        return {
          success: true,
          data: {
            filename: name,
            size: blob.size,
            content_type: contentType || blob.type || undefined,
          },
        }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.uploadChunk',
    async (message: TappMessage) => {
      const [transferId, req] =
        (message.payload as { args: unknown[] }).args || []
      if (
        !transferId ||
        typeof transferId !== 'string' ||
        !req ||
        typeof req !== 'object'
      ) {
        return missingArg()
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.uploadChunk(
          transferId,
          req as Parameters<typeof federationApi.uploadChunk>[1],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.cancelTransfer',
    async (message: TappMessage) => {
      const [transferId] = (message.payload as { args: unknown[] }).args || []
      if (!transferId || typeof transferId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.cancelTransfer(transferId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  // WS 实时事件订阅

  bridge.registerHandler(
    'federation.subscribeChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return missingArg()
      const current = channelSockets.get(channelId)
      if (
        current &&
        (current.readyState === WebSocket.CONNECTING ||
          current.readyState === WebSocket.OPEN)
      ) {
        return { success: true, data: { subscribed: true, alreadyOpen: true } }
      }
      if (current) {
        channelSockets.delete(channelId)
        safeClose(current)
      }
      try {
        // Browser WS cannot carry X-Tapp-Runtime-Grant; mint a one-time ticket
        // over REST with the grant, then present it on the upgrade URL.
        const runtimeGrant = await bridge.getRuntimeGrant()
        const { ticket } = await federationApi.mintChannelWsTicket(
          channelId,
          runtimeGrant,
        )
        const ws = federationApi.connectChannelWs(channelId, ticket)
        channelSockets.set(channelId, ws)
        attachChannelWs(channelId, ws)
        return { success: true, data: { subscribed: true } }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.unsubscribeChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return missingArg()
      const ws = channelSockets.get(channelId)
      if (ws) {
        safeClose(ws)
        channelSockets.delete(channelId)
      }
      return { success: true, data: { unsubscribed: true } }
    },
  )

  bridge.registerHandler(
    'federation.subscribeRoom',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      const current = roomSockets.get(roomId)
      if (
        current &&
        (current.readyState === WebSocket.CONNECTING ||
          current.readyState === WebSocket.OPEN)
      ) {
        return { success: true, data: { subscribed: true, alreadyOpen: true } }
      }
      if (current) {
        roomSockets.delete(roomId)
        safeClose(current)
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const { ticket } = await federationApi.mintRoomWsTicket(
          roomId,
          runtimeGrant,
        )
        const ws = federationApi.connectRoomWs(roomId, ticket)
        roomSockets.set(roomId, ws)
        attachRoomWs(roomId, ws)
        return { success: true, data: { subscribed: true } }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.unsubscribeRoom',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return missingArg()
      const ws = roomSockets.get(roomId)
      if (ws) {
        safeClose(ws)
        roomSockets.delete(roomId)
      }
      return { success: true, data: { unsubscribed: true } }
    },
  )

  return closeAllSockets
}
