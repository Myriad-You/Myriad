import assert from 'node:assert/strict'
import test from 'node:test'
import { LegOrder } from './legOrder'

function layer(name: string, role: string, side: 'L' | 'R' | null = null, hostName?: string) {
  return { source: { name, role, side }, attachment: hostName ? { hostName } : null }
}

const stack = [
  layer('back hair', 'back-hair'),
  layer('legwear-L', 'legwear', 'L'),
  layer('legwear-R', 'legwear', 'R'),
  layer('footwear-L', 'footwear', 'L'),
  layer('lining', 'bottomwear'),
  layer('footwear-R', 'footwear', 'R'),
  layer('charm', 'objects', null, 'footwear-L'),
  layer('skirt', 'bottomwear'),
]
const names = (layers: readonly { source: { name: string } }[]) => layers.map((l) => l.source.name)

test('the free leg comes over the other, and nothing between them moves', () => {
  const legs = LegOrder.of(stack)!
  assert.equal(legs.order(stack), stack, 'as drawn until a leg leads')
  legs.step(0.8, 0)
  assert.deepEqual(names(legs.order(stack)), ['back hair', 'legwear-R', 'footwear-R', 'legwear-L', 'lining', 'footwear-L', 'charm', 'skirt'])
  legs.step(0, 0.8)
  assert.deepEqual(names(legs.order(stack)), ['back hair', 'legwear-L', 'footwear-L', 'charm', 'lining', 'legwear-R', 'footwear-R', 'skirt'])
})

test('the lead passes as soon as the other leg is freer, but not on a jitter between two alike', () => {
  const legs = LegOrder.of(stack)!
  legs.step(0.8, 0)
  const left = names(legs.order(stack))
  for (const [l, r] of [[0.5, 0.51], [0.4, 0.415], [0.2, 0.2]]) {
    legs.step(l, r)
    assert.deepEqual(names(legs.order(stack)), left)
  }
  legs.step(0.2, 0.25)
  assert.notDeepEqual(names(legs.order(stack)), left)
})

test('without two legs there is nothing to order', () => {
  assert.equal(LegOrder.of(stack.filter((l) => l.source.side !== 'R')), null)
  assert.equal(LegOrder.of([layer('face', 'face')]), null)
})
