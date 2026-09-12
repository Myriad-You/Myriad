import assert from 'node:assert/strict'
import test from 'node:test'
import { bindAttachmentMesh, sampleAttachmentMesh } from './attachmentMesh'
import { applySurfaceContact, bindSurfaceContact } from './surfaceContact'
import { buildContactSurfaceMesh } from './surfaceMesh'

function fixture() {
  const rest = new Float32Array([0, 0, 100, 0, 100, 100, 0, 100])
  const host = { rest, deformed: rest.slice(), indices: new Uint16Array([0, 1, 2, 0, 2, 3]) }
  const source = { x: 17, y: 13, w: 61, h: 73, atlas: { x: 0.2, y: 0.3, w: 0.1, h: 0.2 } }
  return { host, source }
}

test('contact triangulation preserves full drawing coverage, UVs and conforming edges', () => {
  const { host, source } = fixture()
  const mesh = buildContactSurfaceMesh(host, source)!
  assert.ok(mesh)
  assert.ok(mesh.rest.length > host.rest.length)
  let area = 0
  const edges = new Map<string, { count: number; a: number; b: number }>()
  for (let i = 0; i < mesh.indices.length; i += 3) {
    const ids = Iterator.from(mesh.indices.slice(i, i + 3)).toArray()
    const [a, b, c] = ids.map(id => id * 2)
    const cross = (mesh.rest[b] - mesh.rest[a]) * (mesh.rest[c + 1] - mesh.rest[a + 1]) -
      (mesh.rest[c] - mesh.rest[a]) * (mesh.rest[b + 1] - mesh.rest[a + 1])
    assert.ok(cross > 0)
    area += cross / 2
    for (let j = 0; j < 3; j++) {
      const a = ids[j]
      const b = ids[(j + 1) % 3]
      const key = [a, b].toSorted((x, y) => x - y).join(':')
      const edge = edges.get(key) ?? { count: 0, a, b }
      edge.count++
      edges.set(key, edge)
    }
  }
  assert.ok(Math.abs(area - source.w * source.h) < 0.01)
  for (const { a, b, count } of edges.values()) {
    if (count === 2) continue
    assert.equal(count, 1)
    const boundary = [source.x, source.x + source.w].some(x => mesh.rest[a * 2] === x && mesh.rest[b * 2] === x) ||
      [source.y, source.y + source.h].some(y => mesh.rest[a * 2 + 1] === y && mesh.rest[b * 2 + 1] === y)
    assert.ok(boundary, `unpaired interior edge ${a}:${b}`)
  }
  for (let i = 0; i < mesh.rest.length; i += 2) {
    assert.ok(Math.abs(mesh.atlasUvs[i] - (source.atlas.x + (mesh.rest[i] - source.x) / source.w * source.atlas.w)) < 1e-7)
    assert.ok(Math.abs(mesh.atlasUvs[i + 1] - (source.atlas.y + (mesh.rest[i + 1] - source.y) / source.h * source.atlas.h)) < 1e-7)
  }
})

test('fully bound triangle interiors follow a non-affine host, not just pinned vertices', () => {
  const { host, source } = fixture()
  const mesh = buildContactSurfaceMesh(host, source)!
  const arm = { ...mesh, deformed: mesh.rest.slice() }
  const contact = bindSurfaceContact(host, arm.rest, new Float32Array(arm.rest.length / 2).fill(1))
  // Different affine maps on the two sides of the host diagonal.
  host.deformed.set([0, 0, 115, -9, 97, 121, -14, 98])
  applySurfaceContact(contact, arm.rest, arm.deformed)
  for (let i = 0; i < arm.indices.length; i += 3) {
    const ids = Iterator.from(arm.indices.slice(i, i + 3)).toArray()
    const x = ids.reduce((sum, id) => sum + arm.rest[id * 2], 0) / 3
    const y = ids.reduce((sum, id) => sum + arm.rest[id * 2 + 1], 0) / 3
    const expected = { x: 0, y: 0 }
    sampleAttachmentMesh(bindAttachmentMesh(host, x, y)!, expected)
    const actualX = ids.reduce((sum, id) => sum + arm.deformed[id * 2], 0) / 3
    const actualY = ids.reduce((sum, id) => sum + arm.deformed[id * 2 + 1], 0) / 3
    assert.ok(Math.hypot(actualX - expected.x, actualY - expected.y) < 1e-5)
  }
})

test('partial coverage and invalid geometry keep the original drawing mesh', () => {
  const { host, source } = fixture()
  assert.equal(buildContactSurfaceMesh(host, { ...source, x: -1 }), null)
  assert.equal(buildContactSurfaceMesh({ ...host, indices: new Uint16Array([0, 1, 2]) }, source), null)
  for (const w of [0, -1, NaN, Infinity]) assert.equal(buildContactSurfaceMesh(host, { ...source, w }), null)
  assert.equal(buildContactSurfaceMesh({ ...host, indices: new Uint16Array([0, 1, 9]) }, source), null)
  assert.equal(buildContactSurfaceMesh({ ...host, rest: new Float32Array([NaN, 0]) }, source), null)
})
