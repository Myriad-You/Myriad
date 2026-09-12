import assert from 'node:assert/strict'
import { it } from 'node:test'
import { progressOutbox } from './progressOutbox'
import { ProgressSync } from './progressSync'

function memoryStorage(): Storage {
  const map = new Map<string, string>()
  return {
    get length() {
      return map.size
    },
    clear() {
      map.clear()
    },
    getItem(key) {
      return map.has(key) ? map.get(key)! : null
    },
    key(index) {
      return Iterator.from(map.keys()).toArray()[index] ?? null
    },
    removeItem(key) {
      map.delete(key)
    },
    setItem(key, value) {
      map.set(key, String(value))
    },
  }
}

it('keeps unconfirmed progress for one subject and item only', () => {
  const storage = memoryStorage()
  const alice = progressOutbox(storage, 'alice', 1, 7)
  alice.save(41, 1000)
  assert.deepEqual(alice.load(), { progress: 41, observedAt: 1000 })
  assert.equal(progressOutbox(storage, 'bob', 1, 7).load(), null)
  assert.equal(progressOutbox(storage, 'alice', 2, 7).load(), null)
  assert.equal(progressOutbox(storage, 'alice', 1, 8).load(), null)
  alice.clear()
  assert.equal(alice.load(), null)
})

it('replays the outbox after a new sync is constructed', async () => {
  const storage = memoryStorage()
  const persist = progressOutbox(storage, 'alice', 1, 7)
  persist.save(37, 1000)
  const owner = new AbortController()
  const sent: number[] = []
  const sync = new ProgressSync(
    async (value) => {
      sent.push(value)
    },
    owner.signal,
    () => {},
    persist,
  )
  await sync.flush()
  assert.deepEqual(sent, [37])
  assert.equal(persist.load(), null)
  owner.abort()
})
