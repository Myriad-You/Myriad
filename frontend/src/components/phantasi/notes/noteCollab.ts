/** 协同事件。服务端原样广播，revision 冲突由保存接口判。 */

export interface NoteCollabPeer {
  peerId: string
  userId: number
  name?: string | null
  cursor?: number | null
  lastEditAt?: number
}

export interface NoteCollabEvent {
  type: string
  peer_id: string
  user_id: number
  name?: string | null
  revision?: number | null
  cursor?: number | null
  title?: string | null
  content_md?: string | null
  topic?: string | null
  image?: string | null
  published_at?: number | null
  client_request_id?: string | null
}

export function applyCollabPeers(
  peers: NoteCollabPeer[],
  event: NoteCollabEvent,
  selfPeerId?: string,
): NoteCollabPeer[] {
  if (event.peer_id && event.peer_id === selfPeerId) return peers
  if (event.type === 'leave') {
    return peers.filter((peer) => peer.peerId !== event.peer_id)
  }
  if (
    event.type === 'join' ||
    event.type === 'presence' ||
    event.type === 'edit'
  ) {
    const existing = peers.find((peer) => peer.peerId === event.peer_id)
    const next: NoteCollabPeer = {
      peerId: event.peer_id,
      userId: event.user_id,
      name: event.name ?? existing?.name,
      cursor: event.cursor === undefined ? existing?.cursor : event.cursor,
      lastEditAt: event.type === 'edit' ? Date.now() : existing?.lastEditAt,
    }
    const without = peers.filter((peer) => peer.peerId !== event.peer_id)
    return [...without, next]
  }
  return peers
}

export function shouldApplyRemoteDoc(
  event: NoteCollabEvent,
  currentRevision: number,
): boolean {
  if (event.type !== 'doc') return false
  const revision = event.revision
  return revision != null && revision > currentRevision
}

/** 服务端在文档被删掉时发给房间里每个人的事件；收到就别再连这篇。 */
export const NOTE_DOC_DELETED_EVENT = 'deleted'

/**
 * 这篇云端文档已经不在了（被删、或记着的是个早就没了的 id）。
 * 只认 404：断网、5xx、权限问题都可能自己好，要接着重试。
 */
export function isNoteDocGone(error: unknown): boolean {
  return typeof error === 'object' && error != null && (error as { status?: unknown }).status === 404
}
