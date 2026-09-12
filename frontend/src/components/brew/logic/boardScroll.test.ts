import assert from 'node:assert/strict'
import { it } from 'node:test'
import { captureBoardScroll, restoreBoardScroll } from './boardScroll'

it('restores the captured window position', () => {
  let x = 0
  let y = 0
  const view = {
    get scrollX() {
      return x
    },
    get scrollY() {
      return y
    },
    scrollTo(nextX: number, nextY: number) {
      x = nextX
      y = nextY
    },
  }
  x = 12
  y = 480
  const pos = captureBoardScroll(view)
  view.scrollTo(0, 0)
  assert.equal(y, 0)
  restoreBoardScroll(pos, view)
  assert.deepEqual({ x, y }, { x: 12, y: 480 })
  restoreBoardScroll(null, view)
  assert.equal(y, 480)
})

it('captures rail pan offsets and writes restore tokens', () => {
  const root = {
    nodes: [
      { dataset: { brewRailTrack: 'sites', brewRailScroll: '80' } },
      { dataset: { brewRailTrack: 'items', brewRailScroll: '24' } },
    ],
    querySelectorAll() {
      return this.nodes
    },
    querySelector(selector: string) {
      const key = selector.match(/data-brew-rail-track="([a-z]+)"/)?.[1]
      return this.nodes.find((node) => node.dataset.brewRailTrack === key) ?? null
    },
  }
  const view = {
    scrollX: 0,
    scrollY: 0,
    scrollTo() {},
  }
  const pos = captureBoardScroll(view, root as unknown as ParentNode)
  assert.deepEqual(pos.rails, { sites: 80, items: 24 })
  restoreBoardScroll(pos, view, root as unknown as ParentNode)
  assert.equal(root.nodes[0]?.dataset.brewRailRestore, '80')
  assert.equal(root.nodes[1]?.dataset.brewRailRestore, '24')
})
