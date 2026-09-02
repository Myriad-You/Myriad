import type { WindowRef } from './windowAgentTarget.ts'
/**
 *   pnpm exec tsx --test src/tapp/hooks/windowAgentTarget.test.ts
 */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  resolveCloseWindowIds,
  resolveWindowTarget,
} from './windowAgentTarget.ts'

const windows: WindowRef[] = [
  { windowId: 'w1', tappId: 'alpha', position: { x: 10, y: 0 } },
  { windowId: 'w2', tappId: 'beta', position: { x: 400, y: 0 } },
]

describe('resolveWindowTarget', () => {
  it('matches windowId and tappId', () => {
    assert.equal(resolveWindowTarget({ windowId: 'w2' }, windows, 'w1'), 'w2')
    assert.equal(resolveWindowTarget({ tappId: 'alpha' }, windows, 'w1'), 'w1')
  })

  it('resolves left/right/next/previous/active', () => {
    assert.equal(resolveWindowTarget({ position: 'left' }, windows, 'w2'), 'w1')
    assert.equal(
      resolveWindowTarget({ position: 'right' }, windows, 'w1'),
      'w2',
    )
    assert.equal(resolveWindowTarget({ position: 'next' }, windows, 'w1'), 'w2')
    assert.equal(
      resolveWindowTarget({ position: 'previous' }, windows, 'w1'),
      'w2',
    )
    assert.equal(
      resolveWindowTarget({ position: 'active' }, windows, 'w2'),
      'w2',
    )
  })

  it('close all returns every window id', () => {
    assert.deepEqual(
      resolveCloseWindowIds({ position: 'all' }, windows, 'w1'),
      ['w1', 'w2'],
    )
  })
})
