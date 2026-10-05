import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { addDoneId, clearStorageKeepingTourDone, parseDoneIds } from './tourDone'

describe('parseDoneIds', () => {
  it('reads a string array', () => {
    assert.deepEqual(parseDoneIds('["home-owner"]'), ['home-owner'])
  })

  it('returns empty on junk', () => {
    assert.deepEqual(parseDoneIds('{'), [])
    assert.deepEqual(parseDoneIds('null'), [])
    assert.deepEqual(parseDoneIds(null), [])
  })
})

describe('addDoneId', () => {
  it('appends once', () => {
    assert.deepEqual(addDoneId([], 'home-visitor'), ['home-visitor'])
    assert.deepEqual(addDoneId(['home-visitor'], 'home-visitor'), [
      'home-visitor',
    ])
  })
})

describe('clearStorageKeepingTourDone', () => {
  function memoryStorage(entries: Record<string, string>) {
    const map = new Map(Object.entries(entries))
    return {
      map,
      getItem: (key: string) => map.get(key) ?? null,
      setItem: (key: string, value: string) => {
        map.set(key, value)
      },
      clear: () => map.clear(),
    }
  }

  it('clears everything but the finished tours', () => {
    const storage = memoryStorage({
      myriad_tour_done_v1: '["home-visitor","library-canvas"]',
      myriad_session_hint: 'true',
      recent_activities_cache_v4: '{}',
    })
    clearStorageKeepingTourDone(storage)
    assert.deepEqual([...storage.map], [
      ['myriad_tour_done_v1', '["home-visitor","library-canvas"]'],
    ])
  })

  it('does not invent the key when no tour was finished', () => {
    const storage = memoryStorage({ myriad_session_hint: 'true' })
    clearStorageKeepingTourDone(storage)
    assert.equal(storage.map.size, 0)
  })
})
