import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  applyTappAppCardOrder,
  isSiteOwnerLayoutPending,
} from './tappAppCardOrder.ts'

describe('applyTappAppCardOrder', () => {
  const items = [{ id: 'a' }, { id: 'b' }, { id: 'c' }]

  it('no-ops when order is empty (catalog order)', () => {
    assert.deepEqual(applyTappAppCardOrder(items, []), items)
  })

  it('reorders known ids and appends unknowns in catalog order', () => {
    assert.deepEqual(applyTappAppCardOrder(items, ['c', 'a']), [
      { id: 'c' },
      { id: 'a' },
      { id: 'b' },
    ])
  })

  it('ignores order ids not in the list', () => {
    assert.deepEqual(applyTappAppCardOrder(items, ['x', 'b', 'a']), [
      { id: 'b' },
      { id: 'a' },
      { id: 'c' },
    ])
  })
})

describe('isSiteOwnerLayoutPending', () => {
  it('holds guest public list until layoutReady', () => {
    assert.equal(
      isSiteOwnerLayoutPending({
        layoutReady: false,
        isAuthenticated: false,
        isSiteScope: false,
      }),
      true,
    )
  })

  it('releases guest once layoutReady', () => {
    assert.equal(
      isSiteOwnerLayoutPending({
        layoutReady: true,
        isAuthenticated: false,
        isSiteScope: false,
      }),
      false,
    )
  })

  it('holds regular-user site scope until layoutReady', () => {
    assert.equal(
      isSiteOwnerLayoutPending({
        layoutReady: false,
        isAuthenticated: true,
        isSiteScope: true,
      }),
      true,
    )
  })

  it('does not hold personal mine scope (local cache ok)', () => {
    assert.equal(
      isSiteOwnerLayoutPending({
        layoutReady: false,
        isAuthenticated: true,
        isSiteScope: false,
      }),
      false,
    )
  })
})

describe('guest public order contract', () => {
  it('does not apply provisional localStorage-like order before remote', () => {
    // 访客临时顺序为空，直到远程站点布局到达。
    const catalog = [{ id: '1' }, { id: '2' }, { id: '3' }]
    const guestProvisionalOrder: string[] = []
    assert.deepEqual(
      applyTappAppCardOrder(catalog, guestProvisionalOrder),
      catalog,
    )

    assert.equal(
      isSiteOwnerLayoutPending({
        layoutReady: false,
        isAuthenticated: false,
        isSiteScope: false,
      }),
      true,
    )

    const siteOrder = ['3', '1', '2']
    assert.deepEqual(applyTappAppCardOrder(catalog, siteOrder), [
      { id: '3' },
      { id: '1' },
      { id: '2' },
    ])
  })
})
