/**
 * 思考流光抽签：同一种子可复现；同时在场的数量和底边范围锁死。
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  AURORA_BLOB_BOTTOM,
  AURORA_BLOB_COUNT,
  AURORA_BLOB_HEIGHT,
  AURORA_HUE_COUNT,
  BLOB_PATHS,
  paintAuroraPrism,
} from './agentAuroraRandom'

function mulberry32(seed: number): () => number {
  let t = seed >>> 0
  return () => {
    t += 0x6D2B79F5
    let r = Math.imul(t ^ (t >>> 15), 1 | t)
    r ^= r + Math.imul(r ^ (r >>> 7), 61 | r)
    return ((r ^ (r >>> 14)) >>> 0) / 4294967296
  }
}

function pct(value: string): number {
  return Number.parseFloat(value)
}

function huesOf(paint: ReturnType<typeof paintAuroraPrism>): number[] {
  return paint.blobs.flatMap((blob) =>
    [...blob.color.matchAll(/(\d+(?:\.\d+)?)deg/g)].map((match) =>
      Number(match[1]),
    ),
  )
}

describe('aurora prism paint', () => {
  it('always plants five blobs across the bottom band', () => {
    for (const seed of [1, 3, 7, 11, 19]) {
      const { blobs } = paintAuroraPrism(mulberry32(seed))
      assert.equal(blobs.length, AURORA_BLOB_COUNT)
      const centers = blobs
        .map((blob) => pct(blob.x) + pct(blob.w) / 2)
        .sort((a, b) => a - b)
      assert.ok(centers[0] >= 4 && centers[0] <= 22)
      assert.ok(centers[4] >= 78 && centers[4] <= 96)
      assert.ok(centers[4] - centers[0] >= 58)
      for (const blob of blobs) {
        const width = pct(blob.w)
        const height = pct(blob.h)
        const y = pct(blob.y)
        const opacity = Number(blob.opacity)
        assert.ok(width >= 34 && width <= 42)
        assert.ok(
          height >= AURORA_BLOB_HEIGHT.min && height <= AURORA_BLOB_HEIGHT.max,
        )
        assert.ok(y >= AURORA_BLOB_BOTTOM.min && y <= AURORA_BLOB_BOTTOM.max)
        assert.match(blob.h, /px$/)
        assert.match(blob.y, /px$/)
        assert.ok(opacity >= 0.62)
        assert.ok(BLOB_PATHS.includes(blob.path))
        assert.equal(blob.stagger, String(blobs.indexOf(blob)))
      }
    }
  })

  it('keeps a bounded hue set on screen at once', () => {
    const paint = paintAuroraPrism(mulberry32(7))
    const unique = new Set(huesOf(paint))
    assert.ok(unique.size >= AURORA_HUE_COUNT.min)
    assert.ok(unique.size <= AURORA_HUE_COUNT.max)
  })

  it('draws more than three hues across recipes and does not reuse one', () => {
    const a = paintAuroraPrism(mulberry32(3))
    const b = paintAuroraPrism(mulberry32(11))
    const unique = new Set([...huesOf(a), ...huesOf(b)])
    assert.ok(unique.size > 3)
    assert.notEqual(JSON.stringify(a.blobs), JSON.stringify(b.blobs))
  })

  it('sends blobs along more than one path and direction', () => {
    const paint = paintAuroraPrism(mulberry32(5))
    assert.ok(new Set(paint.blobs.map((blob) => blob.path)).size >= 2)
    assert.equal(new Set(paint.blobs.map((blob) => blob.dir)).size, 2)
    assert.doesNotMatch(JSON.stringify(paint), /linear-gradient/)
  })
})
