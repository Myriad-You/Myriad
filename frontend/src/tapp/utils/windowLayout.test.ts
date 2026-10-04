import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { getTappWindowTiles, getTappWindowWorkArea } from './windowLayout'

describe('TAPP desktop layout', () => {
  it('reserves desktop controls inside the bounds that already exclude the dock', () => {
    assert.deepEqual(getTappWindowWorkArea({ width: 1440, height: 800 }), {
      position: { x: 16, y: 80 },
      size: { width: 1408, height: 704 },
    })
  })

  it('places two windows side by side on a wide screen and stacks them on a tall screen', () => {
    const wide = getTappWindowTiles(2, { width: 1440, height: 800 })
    assert.equal(wide[0].position.y, wide[1].position.y)
    assert.ok(wide[0].position.x < wide[1].position.x)
    const tall = getTappWindowTiles(2, { width: 900, height: 1400 })
    assert.equal(tall[0].position.x, tall[1].position.x)
    assert.ok(tall[0].position.y < tall[1].position.y)
  })

  it('uses one half for a single visible window and leaves the second pane empty', () => {
    for (const bounds of [{ width: 1440, height: 800 }, { width: 900, height: 1400 }]) {
      const single = getTappWindowTiles(1, bounds)
      const pair = getTappWindowTiles(2, bounds)
      const area = getTappWindowWorkArea(bounds)
      assert.deepEqual(single, [pair[0]])
      assert.ok(single[0].size.width * single[0].size.height < area.size.width * area.size.height / 2)
    }
  })

  it('keeps every supported window count inside the work area without overlaps', () => {
    for (const bounds of [{ width: 1920, height: 960 }, { width: 900, height: 1400 }, { width: 640, height: 380 }]) {
      const area = getTappWindowWorkArea(bounds)
      for (let count = 1; count <= 5; count++) {
        const tiles = getTappWindowTiles(count, bounds)
        assert.equal(tiles.length, count)
        for (const [i, tile] of tiles.entries()) {
          assert.ok(tile.size.width > 0 && tile.size.height > 0)
          assert.ok(tile.position.x >= area.position.x && tile.position.y >= area.position.y)
          assert.ok(tile.position.x + tile.size.width <= area.position.x + area.size.width + 0.001)
          assert.ok(tile.position.y + tile.size.height <= area.position.y + area.size.height + 0.001)
          for (const other of tiles.slice(i + 1)) {
            assert.ok(
              tile.position.x + tile.size.width <= other.position.x ||
              other.position.x + other.size.width <= tile.position.x ||
              tile.position.y + tile.size.height <= other.position.y ||
              other.position.y + other.size.height <= tile.position.y,
            )
          }
        }
        if (count > 1) {
          const gapArea = area.size.width * area.size.height - tiles.reduce((sum, tile) => sum + tile.size.width * tile.size.height, 0)
          assert.ok(gapArea >= -0.001 && gapArea < area.size.width * area.size.height * 0.25)
        }
      }
    }
  })

  it('does nothing until the desktop has measurable space or when no windows are visible', () => {
    assert.deepEqual(getTappWindowTiles(0, { width: 1440, height: 800 }), [])
    assert.deepEqual(getTappWindowTiles(3, { width: 0, height: 0 }), [])
  })
})
