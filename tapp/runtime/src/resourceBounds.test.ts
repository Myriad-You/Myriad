import assert from 'node:assert/strict'
import test from 'node:test'
import { BoundedResourceCache, HiddenSandboxPool } from './resourceBounds'

test('resource cache rejects oversized entries and evicts by total bytes', () => {
  const cache = new BoundedResourceCache<string>(3, 8)
  cache.set('a', 'aa', 100)
  cache.set('b', 'bb', 100)
  cache.set('c', 'cc', 100)
  assert.equal(cache.get('a'), undefined)
  assert.equal(cache.get('b'), 'bb')
  cache.set('huge', '12345', 100)
  assert.equal(cache.get('huge'), undefined)
  cache.clear()
})

test('resource cache actively frees entries without another read', async () => {
  const cache = new BoundedResourceCache<string>(3, 100)
  cache.set('a', 'a', 5)
  await new Promise(resolve => setTimeout(resolve, 25))
  assert.equal(cache.size, 0)
})

test('hidden pool evicts oldest hidden surface without moving DOM', () => {
  const pool = new HiddenSandboxPool(2)
  const evicted: string[] = []
  pool.add('a', () => evicted.push('a'))
  pool.add('b', () => evicted.push('b'))
  pool.remove('a')
  pool.add('c', () => evicted.push('c'))
  assert.deepEqual(evicted, [])
  pool.add('d', () => evicted.push('d'))
  assert.deepEqual(evicted, ['b'])
})
