/**
 * Federation Bridge — Tapp 运行时联邦能力桥接
 *
 * 为 Tapp 沙箱提供联邦 API 访问能力
 * 通过 TappBridge 消息机制暴露受限的联邦操作
 *
 * 支持的操作域：
 * - federation.timeline — 读取联邦时间线
 * - federation.follow / unfollow — 关注管理
 * - federation.channels — Channel 读取与消息发送
 * - federation.rooms — Room 读取与消息发送
 * - federation.rings — Ring 信息读取
 * - federation.publish / unpublish — 内容发布管理
 * - federation.trust — 实例信任策略管理
 * - federation.transfers — 文件传输
 * - federation.subscribeChannel / subscribeRoom — WS 实时事件订阅
 *   (mint one-time `tapp_ws_ticket` via grant-authenticated REST, then upgrade)
 */

import type { TappInstance, TappMessage } from '../types'
import type { TappBridge } from './TappBridge'
import { federationApi } from '../../services/federationApi'
import { getFederationFeed } from '../services/TappApiService'
import {
  federationMediaUrlRejectionReason,
  isValidFederationMediaUrl,
} from '../utils/federationMediaUrl'

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
          } else if (data.type === 'room_deleted') {
            bridge.emit('federation:roomUpdate', {
              roomId,
              event: 'deleted',
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

  // ==================== 身份 ====================

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
          error instanceof Error ? error.message : 'Failed to get identity',
      }
    }
  })

  bridge.registerHandler(
    'federation.rotateKeys',
    async (message: TappMessage) => {
      const [confirmRaw] = (message.payload as { args: unknown[] }).args || []
      const confirm = confirmRaw === true
      if (!confirm) {
        return {
          success: false,
          error: 'Key rotation requires confirm=true',
        }
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.rotateKeys(true, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            error instanceof Error ? error.message : 'Failed to rotate keys',
        }
      }
    },
  )

  // ==================== 时间线 ====================

  bridge.registerHandler('federation.getFeed', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await getFederationFeed(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed to get feed',
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
          error instanceof Error ? error.message : 'Failed to get timeline',
      }
    }
  })

  // ==================== 关注管理 ====================

  bridge.registerHandler('federation.follow', async (message: TappMessage) => {
    const [target] = (message.payload as { args: unknown[] }).args || []
    if (!target || typeof target !== 'string')
      return { success: false, error: 'Target actor URL is required' }
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.follow(target, runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed to follow',
      }
    }
  })

  bridge.registerHandler(
    'federation.unfollow',
    async (message: TappMessage) => {
      const [target] = (message.payload as { args: unknown[] }).args || []
      if (!target || typeof target !== 'string')
        return { success: false, error: 'Target actor URL is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.unfollow(target, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed to unfollow',
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
        error: error instanceof Error ? error.message : 'Failed',
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
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  // ==================== 内容发布 ====================

  bridge.registerHandler('federation.publish', async (message: TappMessage) => {
    const [req] = (message.payload as { args: unknown[] }).args || []
    if (!req) return { success: false, error: 'Publish request is required' }
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
            return { success: false, error: reason }
          }
        }
      }
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.publish(publishReq, runtimeGrant)
      if (!data || data.success === false) {
        console.error('[FederationBridge] publish returned unsuccessful', data)
        return {
          success: false,
          error: 'Publish did not confirm success',
        }
      }
      return { success: true, data }
    } catch (error) {
      console.error('[FederationBridge] publish failed', error)
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed to publish',
      }
    }
  })

  bridge.registerHandler(
    'federation.createNote',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return { success: false, error: 'Create note request is required' }
      try {
        const noteReq = req as Parameters<typeof federationApi.createNote>[0]
        const atts = noteReq.attachments
        if (Array.isArray(atts)) {
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
              return { success: false, error: reason }
            }
          }
        }
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.createNote(noteReq, runtimeGrant)
        if (!data || data.success === false) {
          console.error('[FederationBridge] createNote returned unsuccessful', data)
          return {
            success: false,
            error: 'Create note did not confirm success',
          }
        }
        return { success: true, data }
      } catch (error) {
        console.error('[FederationBridge] createNote failed', error)
        return {
          success: false,
          error:
            error instanceof Error ? error.message : 'Failed to create note',
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
        return { success: false, error: 'object_id is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await fn(objectId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            error instanceof Error
              ? error.message
              : `Failed to ${action}`,
        }
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
      return { success: false, error: 'object_id is required' }
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.announce(objectId, content, runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error:
          error instanceof Error ? error.message : 'Failed to announce',
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
          error instanceof Error ? error.message : 'Failed to get bookmarks',
      }
    }
  })

  bridge.registerHandler(
    'federation.uploadMedia',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return { success: false, error: 'Upload media request is required' }
      const body = req as {
        data?: string
        name?: string
        mime?: string
        media_type?: string
      }
      if (!body.data || typeof body.data !== 'string') {
        return {
          success: false,
          error: 'data (data URL or base64) is required',
        }
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
          return { success: false, error: reason }
        }
        return { success: true, data }
      } catch (error) {
        console.error('[FederationBridge] uploadMedia failed', error)
        return {
          success: false,
          error:
            error instanceof Error ? error.message : 'Failed to upload media',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.unpublish',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req)
        return { success: false, error: 'Unpublish request is required' }
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
          error: error instanceof Error ? error.message : 'Failed to unpublish',
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
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  // ==================== Channel ====================

  bridge.registerHandler('federation.getChannels', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getChannels(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler(
    'federation.createChannel',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req)
        return { success: false, error: 'Create channel request is required' }
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
            error instanceof Error ? error.message : 'Failed to create channel',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.acceptChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return { success: false, error: 'Channel ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.acceptChannel(channelId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            error instanceof Error ? error.message : 'Failed to accept channel',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.acceptRoomInvite',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return { success: false, error: 'Room ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.acceptRoomInvite(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            error instanceof Error
              ? error.message
              : 'Failed to accept room invite',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.rejectRoomInvite',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return { success: false, error: 'Room ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.rejectRoomInvite(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            error instanceof Error
              ? error.message
              : 'Failed to reject room invite',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.closeChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return { success: false, error: 'Channel ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.closeChannel(channelId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            error instanceof Error ? error.message : 'Failed to close channel',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.deleteChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return { success: false, error: 'Channel ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.deleteChannel(channelId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            error instanceof Error ? error.message : 'Failed to delete channel',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.getChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return { success: false, error: 'Channel ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getChannel(channelId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
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
        return { success: false, error: 'Channel ID is required' }
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
          error: error instanceof Error ? error.message : 'Failed',
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
        return { success: false, error: 'Channel ID and message are required' }
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
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  // ==================== Room ====================

  bridge.registerHandler('federation.getRooms', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getRooms(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler('federation.getRoom', async (message: TappMessage) => {
    const [roomId] = (message.payload as { args: unknown[] }).args || []
    if (!roomId || typeof roomId !== 'string')
      return { success: false, error: 'Room ID is required' }
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getRoom(roomId, runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler(
    'federation.createRoom',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req)
        return { success: false, error: 'Create room request is required' }
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
            error instanceof Error ? error.message : 'Failed to create room',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.updateRoom',
    async (message: TappMessage) => {
      const [roomId, req] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return { success: false, error: 'Room ID is required' }
      if (!req)
        return { success: false, error: 'Update room request is required' }
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
            error instanceof Error ? error.message : 'Failed to update room',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.getRoomMembers',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return { success: false, error: 'Room ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getRoomMembers(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            error instanceof Error ? error.message : 'Failed to get members',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.inviteMember',
    async (message: TappMessage) => {
      const [roomId, req] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string' || !req) {
        return {
          success: false,
          error: 'Room ID and invite request are required',
        }
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
          error: error instanceof Error ? error.message : 'Failed to invite',
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
        return { success: false, error: 'Room ID and actor URL are required' }
      }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.removeMember(roomId, actorUrl, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            error instanceof Error ? error.message : 'Failed to remove member',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.leaveRoom',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return { success: false, error: 'Room ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.leaveRoom(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            error instanceof Error ? error.message : 'Failed to leave room',
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
        return { success: false, error: 'Room ID is required' }
      if (!newOwner || typeof newOwner !== 'string')
        return { success: false, error: 'New owner is required' }
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
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.initiateChannelE2e',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return { success: false, error: 'Channel ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.initiateChannelE2e(
          channelId,
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.initiateRoomE2e',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return { success: false, error: 'Room ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.initiateRoomE2e(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.deleteRoom',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return { success: false, error: 'Room ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.deleteRoom(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            error instanceof Error ? error.message : 'Failed to delete room',
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
        return { success: false, error: 'Room ID is required' }
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
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.sendRoomMessage',
    async (message: TappMessage) => {
      const [roomId, req] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string' || !req)
        return { success: false, error: 'Room ID and message are required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.sendRoomMessage(
          roomId,
          req as Parameters<typeof federationApi.sendRoomMessage>[1],
          runtimeGrant,
        )
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  // ==================== Pin Room Message ====================

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
        return { success: false, error: 'Room ID and Message ID are required' }
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
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  // ==================== Ring (只读) ====================

  bridge.registerHandler('federation.getRings', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getRings(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler('federation.getRing', async (message: TappMessage) => {
    const [ringId] = (message.payload as { args: unknown[] }).args || []
    if (!ringId || typeof ringId !== 'string')
      return { success: false, error: 'Ring ID is required' }
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getRing(ringId, runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler(
    'federation.getRingPeers',
    async (message: TappMessage) => {
      const [ringId] = (message.payload as { args: unknown[] }).args || []
      if (!ringId || typeof ringId !== 'string')
        return { success: false, error: 'Ring ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getRingPeers(ringId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.createRing',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return { success: false, error: 'Ring request is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.createRing(req as any, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.leaveRing',
    async (message: TappMessage) => {
      const [ringId] = (message.payload as { args: unknown[] }).args || []
      if (!ringId || typeof ringId !== 'string')
        return { success: false, error: 'Ring ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.leaveRing(ringId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler('federation.addPeer', async (message: TappMessage) => {
    const [ringId, req] = (message.payload as { args: unknown[] }).args || []
    if (!ringId || typeof ringId !== 'string')
      return { success: false, error: 'Ring ID is required' }
    if (!req || typeof req !== 'object')
      return { success: false, error: 'Peer request is required' }
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.addPeer(ringId, req as any, runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler(
    'federation.removePeer',
    async (message: TappMessage) => {
      const [ringId, peerUrl] =
        (message.payload as { args: unknown[] }).args || []
      if (!ringId || typeof ringId !== 'string')
        return { success: false, error: 'Ring ID is required' }
      if (!peerUrl || typeof peerUrl !== 'string')
        return { success: false, error: 'Peer URL is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.removePeer(ringId, peerUrl, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.triggerSync',
    async (message: TappMessage) => {
      const [ringId] = (message.payload as { args: unknown[] }).args || []
      if (!ringId || typeof ringId !== 'string')
        return { success: false, error: 'Ring ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.triggerSync(ringId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  // ==================== Trust 策略管理 ====================

  bridge.registerHandler('federation.getTrustPolicy', async () => {
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.getTrustPolicy(runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler(
    'federation.updateTrustPolicy',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return { success: false, error: 'Policy update request is required' }
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
          error: error instanceof Error ? error.message : 'Failed',
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
        error: error instanceof Error ? error.message : 'Failed',
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
          error: error instanceof Error ? error.message : 'Failed',
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
        return { success: false, error: 'Delivery id is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.retryDelivery(queueId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
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
        return { success: false, error: 'Delivery id is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.cancelDelivery(queueId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
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
          error: error instanceof Error ? error.message : 'Failed',
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
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.joinRoom',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return { success: false, error: 'Room ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.joinRoom(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed to join room',
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
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler(
    'federation.updateInstanceTrust',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return { success: false, error: 'Trust request is required' }
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
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.toggleInstanceBlock',
    async (message: TappMessage) => {
      const [req] = (message.payload as { args: unknown[] }).args || []
      if (!req || typeof req !== 'object')
        return { success: false, error: 'Block request is required' }
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
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  // ==================== 文件传输 ====================

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
        return {
          success: false,
          error: 'Channel ID and transfer request are required',
        }
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
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.listTransfers',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return { success: false, error: 'Channel ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.listTransfers(channelId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
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
        return {
          success: false,
          error: 'Room ID and transfer request are required',
        }
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
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.listRoomTransfers',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return { success: false, error: 'Room ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.listRoomTransfers(roomId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.listRoomFiles',
    async (message: TappMessage) => {
      const [roomId, params] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return { success: false, error: 'Room ID is required' }
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
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.getTransfer',
    async (message: TappMessage) => {
      const [transferId] = (message.payload as { args: unknown[] }).args || []
      if (!transferId || typeof transferId !== 'string')
        return { success: false, error: 'Transfer ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.getTransfer(transferId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
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
        return { success: false, error: 'Transfer ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        // Prefer status check for clearer errors before streaming large bodies
        try {
          const meta = await federationApi.getTransfer(transferId, runtimeGrant)
          if (meta && meta.status && meta.status !== 'completed') {
            return {
              success: false,
              error: `Transfer is not ready (status=${meta.status})`,
            }
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
          error: error instanceof Error ? error.message : 'Download failed',
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
        return { success: false, error: 'Transfer ID and chunk are required' }
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
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.cancelTransfer',
    async (message: TappMessage) => {
      const [transferId] = (message.payload as { args: unknown[] }).args || []
      if (!transferId || typeof transferId !== 'string')
        return { success: false, error: 'Transfer ID is required' }
      try {
        const runtimeGrant = await bridge.getRuntimeGrant()
        const data = await federationApi.cancelTransfer(transferId, runtimeGrant)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed',
        }
      }
    },
  )

  // ==================== WS 实时事件订阅 ====================

  bridge.registerHandler(
    'federation.subscribeChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return { success: false, error: 'Channel ID is required' }
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
          error: error instanceof Error ? error.message : 'Failed to subscribe',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.unsubscribeChannel',
    async (message: TappMessage) => {
      const [channelId] = (message.payload as { args: unknown[] }).args || []
      if (!channelId || typeof channelId !== 'string')
        return { success: false, error: 'Channel ID is required' }
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
        return { success: false, error: 'Room ID is required' }
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
          error: error instanceof Error ? error.message : 'Failed to subscribe',
        }
      }
    },
  )

  bridge.registerHandler(
    'federation.unsubscribeRoom',
    async (message: TappMessage) => {
      const [roomId] = (message.payload as { args: unknown[] }).args || []
      if (!roomId || typeof roomId !== 'string')
        return { success: false, error: 'Room ID is required' }
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
