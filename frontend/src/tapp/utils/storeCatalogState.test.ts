/**
 * cd frontend && node --experimental-strip-types --test src/tapp/utils/storeCatalogState.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isStoreCatalogPending,
  selectFeaturedStoreApps,
} from './storeCatalogState.ts'

describe('isStoreCatalogPending', () => {
  it('holds discover / category while remote catalog is in flight', () => {
    assert.equal(isStoreCatalogPending(true, true, null), true)
    assert.equal(isStoreCatalogPending(true, true, 'utility'), true)
  })

  it('does not treat installed as remote-catalog pending', () => {
    assert.equal(isStoreCatalogPending(true, true, '__installed__'), false)
  })

  it('clears once remote arrived or loading finished', () => {
    assert.equal(isStoreCatalogPending(true, false, null), false)
    assert.equal(isStoreCatalogPending(false, true, null), false)
  })
})

describe('selectFeaturedStoreApps', () => {
  const apps = [
    { id: 'a', featured: true },
    { id: 'b' },
    { id: 'c', featured: true },
    { id: 'd', featured: true },
  ]

  it('returns only explicit featured on discover, capped', () => {
    assert.deepEqual(
      selectFeaturedStoreApps(apps, true).map((app) => app.id),
      ['a', 'c'],
    )
  })

  it('does not pad with unfeatured apps', () => {
    assert.deepEqual(
      selectFeaturedStoreApps([{ id: 'local' }, { id: 'also' }], true),
      [],
    )
  })

  it('is empty off discover', () => {
    assert.deepEqual(selectFeaturedStoreApps(apps, false), [])
  })
})
