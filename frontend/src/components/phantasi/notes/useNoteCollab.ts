import type { MutableRefObject } from 'react'
import type { NoteCollabEvent, NoteCollabPeer } from './noteCollab'
import type { NoteCloudFields, NoteCloudSaveHandle } from './useNoteCloudSave'
import { useCallback, useEffect, useRef, useState } from 'react'
import * as phantasiApi from '../../../services/phantasiApi'
import { applyCollabPeers, isNoteDocGone, NOTE_DOC_DELETED_EVENT } from './noteCollab'

export function useNoteCollab({
  cloudId,
  loading,
  userName,
  textareaRef,
  cloud,
  onGone,
  io = phantasiApi,
}: {
  cloudId: number | null
  loading: boolean
  userName?: string
  textareaRef: MutableRefObject<HTMLTextAreaElement | null>
  fields: NoteCloudFields
  cloud: Pick<NoteCloudSaveHandle, 'baseRef' | 'receiveRemote' | 'receiveDoc'>
  /** 这篇已经被删了（服务端通知，或断线后查到 404）。之后不再重连。 */
  onGone?: () => void
  io?: Pick<typeof phantasiApi, 'getNoteDoc' | 'noteDocWsUrl'>
}) {
  const [peers, setPeers] = useState<NoteCollabPeer[]>([])
  const [connection, setConnection] = useState<'connecting' | 'connected' | 'reconnecting'>('connecting')
  const lastEditSent = useRef(0)
  const wsRef = useRef<WebSocket | null>(null)
  const liveRef = useRef({ cloud, io, onGone })
  liveRef.current = { cloud, io, onGone }

  useEffect(() => {
    if (loading || cloudId == null) return
    setConnection('connecting')
    let stopped = false
    let retry: ReturnType<typeof setTimeout> | undefined
    let retryDelay = 1000
    const gone = () => {
      stopped = true
      clearTimeout(retry)
      const ws = wsRef.current
      wsRef.current = null
      ws?.close()
      setPeers([])
      liveRef.current.onGone?.()
    }
    const connect = () => {
      if (stopped) return
      const ws = new WebSocket(liveRef.current.io.noteDocWsUrl(cloudId))
      wsRef.current = ws
      ws.onopen = () => {
        if (stopped || wsRef.current !== ws) return
        setConnection('connected')
        ws.send(JSON.stringify({ type: 'presence' }))
        retryDelay = 1000
        // Repair any snapshots missed while disconnected. The same revision gate
        // handles an HTTP fetch arriving after a newer WS frame.
        void liveRef.current.io.getNoteDoc(cloudId).then((doc) => {
          if (!stopped && wsRef.current === ws) liveRef.current.cloud.receiveDoc(doc)
        }).catch(() => { /* Saving will report network errors; reconnect remains active. */ })
      }
      ws.onmessage = (event) => {
        if (stopped || wsRef.current !== ws) return
        try {
          const incoming = JSON.parse(String(event.data)) as NoteCollabEvent
          if (incoming.type === NOTE_DOC_DELETED_EVENT) {
            gone()
            return
          }
          if (incoming.type === 'join') ws.send(JSON.stringify({ type: 'presence' }))
          setPeers((current) => applyCollabPeers(current, incoming))
          // Unversioned edit snapshots have no common ancestor and replay typing.
          // They carry presence only; text sync uses the autosave's committed doc.
          if (incoming.type !== 'doc' || incoming.revision == null) return
          if (typeof incoming.title !== 'string' || typeof incoming.content_md !== 'string') return
          const base = liveRef.current.cloud.baseRef.current
          liveRef.current.cloud.receiveRemote({
            title: incoming.title,
            contentMd: incoming.content_md,
            topic: incoming.topic === undefined ? base.topic : incoming.topic,
            cover: incoming.image === undefined ? base.cover : incoming.image,
            publishedAt: incoming.published_at === undefined ? base.publishedAt : incoming.published_at,
            // 协作通知不带地址：沿用已确认的值，不能当成被清空。
            slug: base.slug,
            oldPaths: base.oldPaths,
          }, incoming.revision, incoming.client_request_id)
        } catch { /* Malformed frames are ignored. */ }
      }
      ws.onerror = () => ws.close()
      ws.onclose = () => {
        if (stopped || wsRef.current !== ws) return
        wsRef.current = null
        setConnection('reconnecting')
        setPeers([])
        // 连接断了先问这篇还在不在：删掉的文档握手永远是 404，不能一直重连下去。
        const delay = retryDelay
        retryDelay = Math.min(retryDelay * 2, 10_000)
        void liveRef.current.io.getNoteDoc(cloudId).then(
          () => false,
          (error: unknown) => isNoteDocGone(error),
        ).then((missing) => {
          if (stopped) return
          if (missing) gone()
          else retry = setTimeout(connect, delay)
        })
      }
    }
    connect()
    const ping = setInterval(() => {
      const ws = wsRef.current
      if (ws?.readyState !== WebSocket.OPEN) return
      ws.send(JSON.stringify({ type: 'presence', name: userName, cursor: document.activeElement === textareaRef.current ? textareaRef.current?.selectionStart : null }))
    }, 4000)
    return () => {
      stopped = true
      clearInterval(ping)
      clearTimeout(retry)
      const ws = wsRef.current
      wsRef.current = null
      ws?.close()
      setPeers([])
    }
  }, [cloudId, loading, textareaRef, userName])

  // Only local input is activity; incoming saved snapshots must not advertise typing.
  const markEditing = useCallback(() => {
    const ws = wsRef.current
    const now = Date.now()
    if (ws?.readyState !== WebSocket.OPEN || now - lastEditSent.current < 200) return
    lastEditSent.current = now
    ws.send(JSON.stringify({ type: 'edit', cursor: document.activeElement === textareaRef.current ? textareaRef.current?.selectionStart : null }))
  }, [textareaRef])

  return { peers, connection, markEditing }
}
