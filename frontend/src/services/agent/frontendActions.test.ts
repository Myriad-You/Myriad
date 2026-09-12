import type { FrontendActionType } from './types.ts'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  clearAllHandlers,
  executeFrontendAction,
  frontendActionDedupeKey,
  registerActionHandler,
} from './frontendActions.ts'

const root = join(dirname(fileURLToPath(import.meta.url)), '../../..')

function source(rel: string): string {
  return readFileSync(join(root, rel), 'utf8')
}

const BACKEND_EMITTED: string[] = [
  'query_windows',
  'open_window',
  'close_window',
  'focus_window',
  'agent_interaction',
  'navigate',
  'page_interact',
  'brew_open_article',
  'music_control',
  'music_get_status',
  'music_load_playlist',
  'reading_list',
  'show_notification',
  'copy_clipboard',
  'play_audio',
  'show_data',
  'download_file',
  'show_report',
]

const TYPED: FrontendActionType[] = [
  'query_windows',
  'open_window',
  'close_window',
  'focus_window',
  'agent_interaction',
  'navigate',
  'page_interact',
  'brew_open_article',
  'music_control',
  'music_get_status',
  'music_load_playlist',
  'reading_list',
  'show_notification',
  'copy_clipboard',
  'play_audio',
  'show_data',
  'download_file',
  'show_report',
]

const KNOWN_ORPHANS = [] as const

describe('frontendAction chain', () => {
  it('aborted global handlers cannot fall through into another handler', async () => {
    const subject = new AbortController()
    const held = Promise.withResolvers<void>()
    const entered = Promise.withResolvers<void>()
    let laterCalls = 0
    registerActionHandler(async (_action, signal) => {
      assert.equal(signal, subject.signal)
      entered.resolve()
      await held.promise
      return false
    })
    registerActionHandler(async () => { laterCalls++; return true })
    try {
      const pending = executeFrontendAction({ type: 'show_data' }, subject.signal)
      await entered.promise
      subject.abort()
      held.resolve()
      assert.equal(await pending, undefined)
      assert.equal(laterCalls, 0)
    } finally { held.resolve(); clearAllHandlers() }
  })

  it('typed handler receives cancellation and its stale result is discarded', async () => {
    const subject = new AbortController()
    const held = Promise.withResolvers<void>()
    const entered = Promise.withResolvers<void>()
    registerActionHandler('show_data', async (_action, signal) => {
      assert.equal(signal, subject.signal)
      entered.resolve()
      await held.promise
      return { private: 'A' }
    })
    try {
      const pending = executeFrontendAction({ type: 'show_data' }, subject.signal)
      await entered.promise
      subject.abort()
      held.resolve()
      assert.equal(await pending, undefined)
    } finally { held.resolve(); clearAllHandlers() }
  })

  it('every backend-emitted type is either typed or a known orphan', () => {
    const typed = new Set<string>(TYPED)
    const orphans = new Set<string>(KNOWN_ORPHANS)
    const leftover = Iterator.from(
      new Set(BACKEND_EMITTED).difference(typed.union(orphans)),
    ).toArray()
    assert.deepEqual(leftover, [])
  })

  it('typed handlers in App/AgentGlobalActions/window hook cover the typed union', () => {
    const global = source('src/contexts/AgentGlobalActions.tsx')
    const windows = source('src/tapp/hooks/useWindowAgentHandler.ts')
    const app = source('src/App.tsx')
    const registered = new Set<string>()
    const pattern = /registerActionHandler\(\s*'([^']+)'/g
    for (const text of [global, windows]) {
      let match = pattern.exec(text)
      while (match) {
        registered.add(match[1])
        match = pattern.exec(text)
      }
    }
    assert.match(app, /action\.type !== 'open_window'/)
    assert.match(app, /action\.type !== 'agent_interaction'/)
    assert.match(app, /action\.type === 'close_window'/)

    const missing = Iterator.from(new Set(TYPED).difference(registered)).toArray()
    assert.deepEqual(
      missing,
      [],
      `typed FrontendActionType without registerActionHandler: ${missing.join(', ')}`,
    )
  })

  it('dedupes the same action from step_completed and the final response', () => {
    const action = { type: 'navigate' as const, path: '/brew', timestamp: 42 }
    assert.equal(frontendActionDedupeKey(action), 'navigate:42')
    assert.equal(
      frontendActionDedupeKey(action),
      frontendActionDedupeKey({ ...action }),
    )
  })
})
