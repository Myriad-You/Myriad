import assert from 'node:assert/strict'
import test from 'node:test'
import { importTimeoutMs } from './psdImportClient'

function psdHeader(width: number, height: number): ArrayBuffer {
  const buffer = new ArrayBuffer(26)
  const view = new DataView(buffer)
  view.setUint32(0, 0x38425053) // 8BPS
  view.setUint16(4, 1)
  view.setUint16(12, 4)
  view.setUint32(14, height)
  view.setUint32(18, width)
  view.setUint16(22, 8)
  view.setUint16(24, 3)
  return buffer
}

test('a bust gets the usual minute and a wide stitched figure proportionally more', () => {
  assert.equal(importTimeoutMs(psdHeader(1088, 1664)), 60_000)
  const gown = importTimeoutMs(psdHeader(2731, 4096))
  assert.ok(gown > 150_000 && gown < 170_000, `${gown}`)
  assert.equal(importTimeoutMs(new ArrayBuffer(4)), 60_000, 'no header: the usual minute')
})
