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

/** 确定访客时短路 Channel/Room 读。仅 isKnownGuest() 为 true 才短路；未知一律走网络。鉴权仍在后端。 */
/** 访客无会话返回空成功，不是错误。 */
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

function federationFail(error: unknown, fallback: string) {
  if (error instanceof ApiError) {
    return {
      success: false as const,
      error: userFacingError(error, fallback),
      code: error.code,
      status: error.status,
      membership_status:
        error.code === 'ROOM_INVITE_PENDING' ? ('pending' as const) : undefined,
    }
  }
  return {
    success: false as const,
    error: userFacingError(error, fallback),
  }
}

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

export function registerFederationHandlers(
  bridge: TappBridge,
  _tappInstance: TappInstance,
): () => void {
  const channelSockets = new Map<string, WebSocket>()
  const roomSockets = new Map<string, WebSocket>()

  const safeClose = (ws: WebSocket): void => {
    try {
      ws.close()
    } catch {
    }
  }

  const closeAllSockets = (): void => {
    for (const ws of channelSockets.values()) safeClose(ws)
    channelSockets.clear()
    for (const ws of roomSockets.values()) safeClose(ws)
    roomSockets.clear()
  }

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
            bridge.emit('federation:channelUpdate', {
              channelId,
              event: 'accepted',
            })
          } else if (data.type === 'key_exchange') {
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
      }
    })
    ws.addEventListener('close', () => {
      if (roomSockets.get(roomId) !== ws) return
      roomSockets.delete(roomId)
      bridge.emit('federation:roomUpdate', { roomId, event: 'disconnected' })
    })
  }

  bridge.registerHandler('federation.getIdentity', async () => {
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

  /** 显式密钥轮换。payload confirm 必须为 true。 */
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
        const NOTE_TEXT_CHAR_LIMIT = 100_000
        const NOTE_ATTACHMENT_COUNT_LIMIT = 32
        const text = typeof noteReq.text === 'string' ? noteReq.text : ''
        if (Iterator.from(text).reduce((n: number) => n + 1, 0) > NOTE_TEXT_CHAR_LIMIT) {
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

  // 只拼 share 文本 + intent_url；不服务端发帖。

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

  bridge.registerHandler('federation.getChannels', async () => {
    try {
      if (isKnownGuest()) return guestEmptyChannels()
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getChannels(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      const status =
        error &&
        typeof error === 'object' &&
        Object.hasOwn(error, 'status') &&
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

  bridge.registerHandler('federation.getRooms', async () => {
    try {
      if (isKnownGuest()) return guestEmptyRooms()
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getRooms(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      const status =
        error &&
        typeof error === 'object' &&
        Object.hasOwn(error, 'status') &&
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

  /** 在宿主文档触发保存；沙箱不能可靠地流式传输数 MB blob。 */
  bridge.registerHandler(
    'federation.downloadTransfer',
    async (message: TappMessage) => {
      const [transferId] = (message.payload as { args: unknown[] }).args || []
      if (!transferId || typeof transferId !== 'string')
        return missingArg()
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        try {
          const meta = await federationApi.getTransfer(transferId, runtimeGrant)
          if (meta && meta.status && meta.status !== 'completed') {
            return opFailed(currentCopy().errors.transferNotReady)
          }
        } catch {
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
        // 浏览器 WS 不能带 X-Tapp-Runtime-Grant；发一次性 ticket。
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
