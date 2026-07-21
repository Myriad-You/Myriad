import { describe, expect, it } from 'vitest'
import { isImeComposing } from './ime'

function makeReactKeyEvent(partial: {
  isComposing?: boolean
  keyCode?: number
}): React.KeyboardEvent {
  return {
    nativeEvent: {
      isComposing: partial.isComposing ?? false,
      keyCode: partial.keyCode ?? 13,
    },
  } as React.KeyboardEvent
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
    expect(isImeComposing(makeReactKeyEvent({}))).toBe(false)
    expect(isImeComposing(makeNativeKeyEvent({}))).toBe(false)
  })

  it('returns true when isComposing is set', () => {
    expect(isImeComposing(makeReactKeyEvent({ isComposing: true }))).toBe(true)
    expect(isImeComposing(makeNativeKeyEvent({ isComposing: true }))).toBe(true)
  })

  it('returns true for legacy keyCode 229', () => {
    expect(isImeComposing(makeReactKeyEvent({ keyCode: 229 }))).toBe(true)
    expect(isImeComposing(makeNativeKeyEvent({ keyCode: 229 }))).toBe(true)
  })
})
