import assert from 'node:assert/strict'
import test from 'node:test'
import { createHairChain, hairChainOffset, hairChainTurn, resetHairChain, stepHairChain } from './hairChain'

const tuning = { omega: 14, damping: 1, carry: 0.7, tipStiffness: 0.6, drag: 6 }

/** A head turning: the root eases 60 px aside over a quarter second. */
function turn(t: number) {
  const s = Math.min(1, Math.max(0, t / 0.25))
  return 60 * (10 * s ** 3 - 15 * s ** 4 + 6 * s ** 5)
}

test('a lock at rest hangs as drawn and stays there', () => {
  const chain = createHairChain(10, 20, 10, 320, 5)
  for (let i = 0; i < 60; i++) stepHairChain(chain, 10, 20, tuning, 0, 1 / 60)
  for (let index = 0; index <= 5; index++) {
    assert.ok(Math.abs(chain.x[index] - 10) < 1e-9)
    assert.ok(Math.abs(chain.y[index] - (20 + 60 * index)) < 1e-9)
  }
  assert.deepEqual(hairChainOffset(chain, 0.7, { x: 1, y: 1 }), { x: 0, y: 0 })
})

test('the tip trails the root, links keep their length, and the lock settles', () => {
  const chain = createHairChain(0, 0, 0, 300, 5)
  stepHairChain(chain, 0, 0, tuning, 0, 1 / 60)
  let trailed = 0
  for (let i = 0; i < 240; i++) {
    stepHairChain(chain, turn(i / 60), 0, tuning, 0, 1 / 60)
    trailed = Math.min(trailed, chain.offsetX[5])
    for (let index = 1; index <= 5; index++) {
      const length = Math.hypot(chain.x[index] - chain.x[index - 1], chain.y[index] - chain.y[index - 1])
      assert.ok(Math.abs(length - 60) < 1e-6)
    }
  }
  assert.ok(trailed < -15, `trailed ${trailed}`)
  assert.ok(Math.abs(chain.offsetX[5]) < 0.5)
})

test('the swing travels down the lock: its links bend both ways at once', () => {
  const chain = createHairChain(0, 0, 0, 300, 5)
  stepHairChain(chain, 0, 0, tuning, 0, 1 / 60)
  let bentBothWays = false
  for (let i = 0; i < 120; i++) {
    stepHairChain(chain, turn(i / 60), 0, tuning, 0, 1 / 60)
    const bends: number[] = []
    for (let index = 1; index < 5; index++) {
      const ax = chain.x[index] - chain.x[index - 1]
      const ay = chain.y[index] - chain.y[index - 1]
      const bx = chain.x[index + 1] - chain.x[index]
      const by = chain.y[index + 1] - chain.y[index]
      bends.push(ax * by - ay * bx)
    }
    if (Math.max(...bends) > 1 && Math.min(...bends) < -1) bentBothWays = true
  }
  assert.ok(bentBothWays)
})

test('a root that jumps is moved, not swung', () => {
  const chain = createHairChain(0, 0, 0, 300, 5)
  stepHairChain(chain, 0, 0, tuning, 0, 1 / 60)
  stepHairChain(chain, 400, 0, tuning, 0, 1 / 60)
  assert.equal(chain.offsetX[5], 0)
  assert.equal(chain.x[5], 400)
  resetHairChain(chain, 0, 0)
  assert.equal(chain.x[0], 0)
})

test('a lock with no length still hangs straight down', () => {
  const chain = createHairChain(5, 5, 5, 5, 3)
  stepHairChain(chain, 5, 5, tuning, 0, 1 / 60)
  stepHairChain(chain, 8, 5, tuning, 50, 1 / 60)
  assert.ok(Number.isFinite(chain.offsetX[3]))
  assert.equal(chain.restY, 1)
})

test('the turn along the lock follows its links, blended between them', () => {
  const chain = createHairChain(0, 0, 0, 100, 2)
  resetHairChain(chain, 0, 0)
  assert.equal(hairChainTurn(chain, 0.5), 0)
  // Upper link hangs straight, lower link kicked 90° toward +x.
  chain.x[2] = 50
  chain.y[2] = 50
  assert.equal(hairChainTurn(chain, 0.2), 0)
  assert.ok(Math.abs(hairChainTurn(chain, 1) - -Math.PI / 2) < 1e-9)
  assert.ok(Math.abs(hairChainTurn(chain, 0.5) - -Math.PI / 4) < 1e-9)
})
