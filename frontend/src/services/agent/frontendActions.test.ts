/**
 *   pnpm exec tsx --test src/services/agent/frontendActions.test.ts
 *
 * Locks the Agent frontendAction chain: every type the backend emits must
 * either have a typed handler, or be listed here as a known gap.
 */

import type { FrontendActionType } from './types.ts'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { frontendActionDedupeKey } from './frontendActions.ts'

const root = join(dirname(fileURLToPath(import.meta.url)), '../../..')

function source(rel: string): string {
  return readFileSync(join(root, rel), 'utf8')
}

/** Types `execute_*` handlers put on `frontendAction.type`. */
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

/** Emitted by the backend, but no typed `registerActionHandler` exists. */
const KNOWN_ORPHANS = [] as const

describe('frontendAction chain', () => {
  it('every backend-emitted type is either typed or a known orphan', () => {
    const typed = new Set<string>(TYPED)
    const orphans = new Set<string>(KNOWN_ORPHANS)
    const leftover = BACKEND_EMITTED.filter(
      (type) => !typed.has(type) && !orphans.has(type),
    )
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

    const missing = TYPED.filter((type) => !registered.has(type))
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
