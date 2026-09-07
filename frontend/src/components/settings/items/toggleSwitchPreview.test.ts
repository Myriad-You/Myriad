import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { resolveToggleSwitchPreview } from './toggleSwitchPreview'

const preview = {
  on: '会在报告页显示卡片',
  off: '报告页不再显示这张卡片',
  disabled: '先填好凭证才能打开',
}

describe('resolveToggleSwitchPreview', () => {
  it('no preview → nothing', () => {
    assert.deepEqual(resolveToggleSwitchPreview(undefined, false, false), {
      body: null,
      showKicker: false,
    })
  })

  it('off → on-copy with kicker', () => {
    assert.deepEqual(resolveToggleSwitchPreview(preview, false, false), {
      body: preview.on,
      showKicker: true,
    })
  })

  it('on → off-copy with kicker', () => {
    assert.deepEqual(resolveToggleSwitchPreview(preview, true, false), {
      body: preview.off,
      showKicker: true,
    })
  })

  it('disabled + disabled copy → blocker, no kicker', () => {
    assert.deepEqual(resolveToggleSwitchPreview(preview, false, true), {
      body: preview.disabled,
      showKicker: false,
    })
  })

  it('disabled without disabled copy falls back to on/off', () => {
    const partial = { on: preview.on, off: preview.off }
    assert.deepEqual(resolveToggleSwitchPreview(partial, false, true), {
      body: preview.on,
      showKicker: true,
    })
    assert.deepEqual(resolveToggleSwitchPreview(partial, true, true), {
      body: preview.off,
      showKicker: true,
    })
  })

  it('missing side copy → nothing for that state', () => {
    assert.deepEqual(
      resolveToggleSwitchPreview({ on: preview.on }, true, false),
      { body: null, showKicker: false },
    )
    assert.deepEqual(
      resolveToggleSwitchPreview({ off: preview.off }, false, false),
      { body: null, showKicker: false },
    )
  })

  it('empty string is not usable copy', () => {
    assert.deepEqual(
      resolveToggleSwitchPreview({ on: '', disabled: '' }, false, false),
      { body: null, showKicker: false },
    )
    assert.deepEqual(
      resolveToggleSwitchPreview({ on: preview.on, disabled: '' }, false, true),
      { body: preview.on, showKicker: true },
    )
  })
})
