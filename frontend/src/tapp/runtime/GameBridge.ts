/**
 * Tapp.game — structured session helpers on federation rooms.
 * Handlers still call federation APIs; this layer only normalizes share IDs
 * and the game:<tappId>:<protocol> envelope.
 */

import type { TappInstance, TappMessage } from '../types'
import type { TappBridge } from './TappBridge'

import { federationApi } from '../../services/federationApi'
import { userFacingError } from '../../utils/userFacingError'

const NONCE_CHARS = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_'

export function formatShareRoomId(
  roomId: string,
  homeServer?: string | null,
): string {
  const id = roomId.trim()
  const home = (homeServer || '').trim().replace(/\/+$/, '')
  if (!home || id.includes('@')) return id
  return `${id}@${home}`
}

export function parseShareRoomId(value: string): {
  roomId: string
  homeServer?: string
} {
  let raw = value.trim()
  if (raw.startsWith('myriad:room:')) {
    raw = raw.slice('myriad:room:'.length).trim()
  }
  const publicIdx = raw.lastIndexOf('/public/rooms/')
  if (publicIdx >= 0) {
    const tail = raw.slice(publicIdx + '/public/rooms/'.length)
    const id = (tail.split(/[?#/]/)[0] || '').trim()
    if (id.startsWith('rm_')) {
      try {
        const href = raw.includes('://') ? raw : `https://${raw}`
        const url = new URL(href)
        const home = url.port ? `${url.hostname}:${url.port}` : url.hostname
        return home ? { roomId: id, homeServer: home } : { roomId: id }
      } catch {
        return { roomId: id }
      }
    }
  }
  const at = raw.lastIndexOf('@')
  if (at > 0) {
    const roomId = raw.slice(0, at).trim()
    const homeServer = raw.slice(at + 1).trim()
    if (
      roomId.startsWith('rm_') &&
      homeServer &&
      !/[/?#@]/.test(homeServer)
    ) {
      return { roomId, homeServer }
    }
  }
  return { roomId: raw }
}

export function gameMessageType(tappId: string, protocol: string): string {
  return `game:${tappId}:${protocol}`
}

function randomNonce(): string {
  let out = ''
  for (let i = 0; i < 16; i++) {
    out += NONCE_CHARS[Math.floor(Math.random() * NONCE_CHARS.length)]
  }
  return out
}

function protocolOf(instance: TappInstance): string {
  const protocol = instance.manifest.game?.protocol
  if (typeof protocol === 'string' && protocol.trim()) return protocol.trim()
  return 'session'
}

function classifyJoinError(error: unknown): { error: string; code?: string } {
  const message = error instanceof Error ? error.message : String(error || '')
  if (/not found on this instance/i.test(message)) {
    return { error: userFacingError(error), code: 'ROOM_NOT_FOUND' }
  }
  if (/Public room not found|not public/i.test(message)) {
    return { error: userFacingError(error), code: 'REMOTE_NOT_PUBLIC' }
  }
  if (/unreachable|home returned|timed out|home_server is empty/i.test(message)) {
    return { error: userFacingError(error), code: 'REMOTE_HOME_UNREACHABLE' }
  }
  if (/blocked|trust/i.test(message)) {
    return { error: userFacingError(error), code: 'INSTANCE_BLOCKED' }
  }
  return { error: userFacingError(error) }
}

export function registerGameHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  const protocol = protocolOf(tappInstance)
  const messageType = gameMessageType(tappInstance.id, protocol)

  const asRecord = (
    value: unknown,
  ): Record<string, unknown> | null => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return null
    return value as Record<string, unknown>
  }

  bridge.registerHandler('game.shareId', async (message: TappMessage) => {
    const [room] = (message.payload as { args: unknown[] }).args || []
    const rec = asRecord(room)
    if (!rec) return { success: false, error: 'Room is required' }
    const roomId = typeof rec.room_id === 'string' ? rec.room_id : ''
    const home =
      typeof rec.home_server === 'string' ? rec.home_server : undefined
    if (!roomId) return { success: false, error: 'room_id is required' }
    return { success: true, data: formatShareRoomId(roomId, home) }
  })

  bridge.registerHandler('game.create', async (message: TappMessage) => {
    const [optsRaw] = (message.payload as { args: unknown[] }).args || []
    const opts =
      optsRaw && typeof optsRaw === 'object' && !Array.isArray(optsRaw)
        ? (optsRaw as Record<string, unknown>)
        : {}
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const maxPlayers =
        typeof opts.maxPlayers === 'number'
          ? opts.maxPlayers
          : tappInstance.manifest.game?.maxPlayers
      const data = await federationApi.createRoom(
        {
          name:
            typeof opts.name === 'string' && opts.name.trim()
              ? opts.name
              : tappInstance.manifest.name,
          description:
            typeof opts.description === 'string' ? opts.description : undefined,
          is_public: opts.isPublic === true,
          invite_policy: 'open',
          max_members: typeof maxPlayers === 'number' ? maxPlayers : undefined,
          game: {
            tapp_id: tappInstance.id,
            protocol,
            max_players: typeof maxPlayers === 'number' ? maxPlayers : undefined,
            max_message_bytes: tappInstance.manifest.game?.maxMessageBytes,
          },
        },
        runtimeGrant,
      )
      const shareId = formatShareRoomId(data.room_id, data.home_server)
      return { success: true, data: { ...data, share_id: shareId } }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('game.join', async (message: TappMessage) => {
    const [shareRaw] = (message.payload as { args: unknown[] }).args || []
    if (typeof shareRaw !== 'string' || !shareRaw.trim()) {
      return { success: false, error: 'Share id is required' }
    }
    const { roomId, homeServer } = parseShareRoomId(shareRaw)
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.joinRoom(
        roomId,
        runtimeGrant,
        homeServer ? { home_server: homeServer } : undefined,
      )
      const shareId = formatShareRoomId(
        typeof data.room_id === 'string' ? data.room_id : roomId,
        homeServer,
      )
      return { success: true, data: { ...data, share_id: shareId } }
    } catch (error) {
      const classified = classifyJoinError(error)
      return { success: false, ...classified }
    }
  })

  bridge.registerHandler('game.leave', async (message: TappMessage) => {
    const [roomId] = (message.payload as { args: unknown[] }).args || []
    if (typeof roomId !== 'string' || !roomId) {
      return { success: false, error: 'Room ID is required' }
    }
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.leaveRoom(roomId, runtimeGrant)
      return { success: true, data }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  const sendEnvelope = async (
    roomId: unknown,
    kind: 'intent' | 'state',
    body: unknown,
    seqRaw: unknown,
  ) => {
    if (typeof roomId !== 'string' || !roomId) {
      return { success: false, error: 'Room ID is required' }
    }
    const seq = typeof seqRaw === 'number' && Number.isFinite(seqRaw) ? seqRaw : 0
    try {
      const runtimeGrant = await bridge.getRuntimeGrant()
      const data = await federationApi.sendRoomMessage(
        roomId,
        {
          message_type: messageType,
          encrypt: false,
          payload: {
            kind,
            seq,
            nonce: randomNonce(),
            body: body ?? {},
          },
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
  }

  bridge.registerHandler('game.sendIntent', async (message: TappMessage) => {
    const [roomId, body, seq] = (message.payload as { args: unknown[] }).args || []
    return sendEnvelope(roomId, 'intent', body, seq)
  })

  bridge.registerHandler('game.sendState', async (message: TappMessage) => {
    const [roomId, body, seq] = (message.payload as { args: unknown[] }).args || []
    return sendEnvelope(roomId, 'state', body, seq)
  })
}
