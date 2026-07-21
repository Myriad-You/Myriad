/**
 *   pnpm exec tsx --test src/utils/ime.test.ts
 */
/* eslint-disable test/no-import-node-test -- node:test */

import type { KeyboardEvent as ReactKeyboardEvent } from 'react'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { isImeComposing } from './ime.ts'

function makeReactKeyEvent(partial: {
  isComposing?: boolean
  keyCode?: number
}): ReactKeyboardEvent {
  return {
    nativeEvent: {
      isComposing: partial.isComposing ?? false,
      keyCode: partial.keyCode ?? 13,
    },
  } as ReactKeyboardEvent
}

function makeNativeKeyEvent(partial: {
  isComposing?: boolean
  keyCode?: number
}): KeyboardEvent {
  return {
    isComposing: partial.isComposing ?? false,
    keyCode: partial.keyCode ?? 13,
  } as KeyboardEvent
}

describe('isImeComposing', () => {
  it('returns false for normal Enter', () => {
    assert.equal(isImeComposing(makeReactKeyEvent({})), false)
    assert.equal(isImeComposing(makeNativeKeyEvent({})), false)
  })

  it('returns true when isComposing is set', () => {
    assert.equal(isImeComposing(makeReactKeyEvent({ isComposing: true })), true)
    assert.equal(isImeComposing(makeNativeKeyEvent({ isComposing: true })), true)
  })

  it('returns true for legacy keyCode 229', () => {
    assert.equal(isImeComposing(makeReactKeyEvent({ keyCode: 229 })), true)
    assert.equal(isImeComposing(makeNativeKeyEvent({ keyCode: 229 })), true)
  })
})
