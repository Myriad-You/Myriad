/** #671：编辑器开着的那篇被删掉（或浏览器记着的 id 早就没了）之后，不能无限重连、关不掉、挡住新建。 */

import type { PhantasiNoteDoc } from '../../../types/phantasi'
import type { NoteCloudFields } from './useNoteCloudSave'
import assert from 'node:assert/strict'
import { afterEach, beforeEach, it } from 'node:test'
import { act, useRef } from 'react'
import { ApiError } from '../../../services/api'
import { isNoteDocGone, NOTE_DOC_DELETED_EVENT } from './noteCollab'
import { mountNoteHook } from './noteHookTestUtils'
import { useNoteCloudSave } from './useNoteCloudSave'
import { useNoteCollab } from './useNoteCollab'
import { useNoteEditorOpen } from './useNoteEditorOpen'
import { useNotePublish } from './useNotePublish'

const fields: NoteCloudFields = { title: '', contentMd: '', topic: null, cover: null, publishedAt: null }

function doc(): PhantasiNoteDoc {
  return { id: 7, item_id: null, title: '', content_md: '', topic: null, image: null,
    published_at: null, scheduled_at: null, status: 'draft', revision: 1, last_error: null, updated_at: 1 }
}

class FakeSocket {
  static OPEN = 1
  static all: FakeSocket[] = []
  readyState = 1
  onopen: (() => void) | null = null
  onmessage: ((event: { data: string }) => void) | null = null
  onerror: (() => void) | null = null
  onclose: (() => void) | null = null
  closed = false
  constructor() { FakeSocket.all.push(this) }
  send() {}
  close() { this.closed = true }
}

let previousSocket: PropertyDescriptor | undefined
beforeEach(() => {
  FakeSocket.all = []
  previousSocket = Object.getOwnPropertyDescriptor(globalThis, 'WebSocket')
  Object.defineProperty(globalThis, 'WebSocket', { value: FakeSocket, configurable: true })
})
afterEach(() => {
  if (previousSocket) Object.defineProperty(globalThis, 'WebSocket', previousSocket)
  else Reflect.deleteProperty(globalThis, 'WebSocket')
})

async function mountCollab(getNoteDoc: () => Promise<PhantasiNoteDoc>) {
  let gone = 0
  const hook = await mountNoteHook(() => {
    const revisionRef = useRef(1)
    const textareaRef = useRef<HTMLTextAreaElement | null>(null)
    const cloud = useNoteCloudSave({ userId: 1, cloudId: 7, loading: false, fields, revisionRef,
      onMerged: () => {}, onSaved: () => {}, onServerDoc: () => {}, onError: () => {},
      labels: { saveFailed: 'failed', conflict: 'conflict' },
    })
    return useNoteCollab({ cloudId: 7, loading: false, textareaRef, fields, cloud,
      onGone: () => { gone += 1 },
      io: { getNoteDoc, noteDocWsUrl: (id) => `ws://test.invalid/${id}` },
    })
  }, undefined)
  return { hook, gone: () => gone }
}

const flush = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 0)) })

it('只有 404 才算这篇没了：断网、5xx 要接着重连', () => {
  assert.equal(isNoteDocGone(new ApiError('missing', 404)), true)
  assert.equal(isNoteDocGone(new ApiError('boom', 500)), false)
  assert.equal(isNoteDocGone(new TypeError('Failed to fetch')), false)
  assert.equal(isNoteDocGone(null), false)
})

it('服务端说这篇删了：退出这篇，不再重连', async () => {
  const { hook, gone } = await mountCollab(async () => doc())
  try {
    await act(async () => FakeSocket.all[0].onmessage!({ data: JSON.stringify({ type: NOTE_DOC_DELETED_EVENT, peer_id: '', user_id: 0 }) }))
    assert.equal(gone(), 1)
    assert.equal(FakeSocket.all[0].closed, true)
    await act(async () => FakeSocket.all[0].onclose?.())
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 1100)) })
    assert.equal(FakeSocket.all.length, 1)
  } finally { await hook.close() }
})

it('断线后查到文档 404：退出这篇，不再重连', async () => {
  const { hook, gone } = await mountCollab(async () => { throw new ApiError('missing', 404) })
  try {
    await act(async () => FakeSocket.all[0].onclose!())
    await flush()
    assert.equal(gone(), 1)
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 1100)) })
    assert.equal(FakeSocket.all.length, 1)
  } finally { await hook.close() }
})

it('断线但文档还在（或查不到）：照旧重连', async () => {
  const { hook, gone } = await mountCollab(async () => { throw new TypeError('Failed to fetch') })
  try {
    await act(async () => FakeSocket.all[0].onclose!())
    await flush()
    assert.equal(hook.current.connection, 'reconnecting')
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 1100)) })
    assert.equal(gone(), 0)
    assert.equal(FakeSocket.all.length, 2)
  } finally { await hook.close() }
})

it('打开浏览器记着的、已经删掉的 id：交给外面退出，不报加载失败', async () => {
  const originalFetch = globalThis.fetch
  globalThis.fetch = async (url) => {
    if (String(url).endsWith('/csrf-token')) return Response.json({ csrf_token: null })
    return Response.json({ success: false, error: 'Note draft not found' }, { status: 404 })
  }
  let gone = 0
  let loading = true
  const hook = await mountNoteHook(() => {
    useNoteEditorOpen({ userId: 1, docId: 7, cloudAck: () => {}, applyServerDoc: () => {},
      applyMergedFields: () => {}, setCloudId: () => {}, setSaved: () => {},
      setLoading: (value) => { loading = value }, onGone: () => { gone += 1 },
      loadFailed: 'failed', scheduleFailed: 'failed',
    })
  }, undefined)
  try {
    await flush()
    assert.equal(gone, 1)
    assert.equal(loading, false)
  } finally { await hook.close(); globalThis.fetch = originalFetch }
})

function publishOptions(overrides: Partial<Parameters<typeof useNotePublish>[0]> = {}): Parameters<typeof useNotePublish>[0] {
  return {
    title: '', contentMd: '', topic: null, cover: null, publishedAt: null, scheduledAt: null,
    cloudId: 7, draftKey: null, docStatus: 'draft', saving: false, setSaving: () => {},
    setContentMd: () => {}, setPublishedAt: () => {}, revisionRef: { current: 1 },
    runWrite: async (write) => write(fields, () => { throw new Error('unused') }) as never,
    onSaved: () => {}, onClose: () => {}, format: (template) => template,
    labels: { titleRequired: '', titleTooLong: '', bodyTooLong: '', saveFailed: '', scheduleNeedTime: '',
      schedulePast: '', deleteConfirm: '', deleteFailed: 'delete failed', discardConfirm: '' },
    ...overrides,
  }
}

it('关空草稿时那篇已经被别处删掉：照样放行，窗口不会关不掉、也不挡新建', async () => {
  const originalFetch = globalThis.fetch
  const deletes: string[] = []
  globalThis.fetch = async (url, options) => {
    if (String(url).endsWith('/csrf-token')) return Response.json({ csrf_token: null })
    if (options?.method === 'DELETE') deletes.push(String(url))
    return Response.json({ success: false, error: 'Note draft not found' }, { status: 404 })
  }
  const hook = await mountNoteHook(() => useNotePublish(publishOptions()), undefined)
  try {
    let ok = false
    await act(async () => { ok = await hook.current.prepareClose(false) })
    assert.equal(ok, true)
    assert.equal(deletes.length, 1)
  } finally { await hook.close(); globalThis.fetch = originalFetch }
})

it('已知这篇没了：离开不再问、不再删，发布也不会另建一篇', async () => {
  const originalFetch = globalThis.fetch
  const requests: string[] = []
  globalThis.fetch = async (url) => {
    requests.push(String(url))
    return Response.json({ success: true, id: 1, link: '', doc: doc() })
  }
  const hook = await mountNoteHook(() => useNotePublish(publishOptions({ cloudId: null, gone: true, title: 'Kept', contentMd: 'body' })), undefined)
  try {
    let ok = false
    await act(async () => { ok = await hook.current.prepareClose(true) })
    assert.equal(ok, true)
    await act(async () => { await hook.current.handleSave() })
    assert.deepEqual(requests, [])
  } finally { await hook.close(); globalThis.fetch = originalFetch }
})
