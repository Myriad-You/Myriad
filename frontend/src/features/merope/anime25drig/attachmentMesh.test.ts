import assert from 'node:assert/strict'
import test from 'node:test'
import { bindAttachmentMesh, offsetAttachmentMeshSample, sampleAttachmentMesh } from './attachmentMesh'

test('samples the current final host geometry including later hair motion', () => {
  const rest = new Float32Array([0, 0, 10, 0, 0, 10])
  const mesh = {
    rest,
    deformed: rest.slice(),
    indices: new Uint16Array([0, 1, 2]),
  }
  const sample = bindAttachmentMesh(mesh, 2, 3)!
  assert.ok(sample)
  const point = { x: 0, y: 0 }
  sampleAttachmentMesh(sample, point)
  assert.deepEqual(point, { x: 2, y: 3 })
  for (let frame = 1; frame <= 3; frame++) {
    for (let i = 0; i < rest.length; i += 2) {
      mesh.deformed[i] = rest[i] + frame * 5
      mesh.deformed[i + 1] = rest[i + 1] - frame * 2
    }
    sampleAttachmentMesh(sample, point)
    assert.ok(Math.abs(point.x - (2 + frame * 5)) < 1e-6)
    assert.ok(Math.abs(point.y - (3 - frame * 2)) < 1e-6)
  }
  assert.equal(bindAttachmentMesh(mesh, 20, 20), null)
  assert.deepEqual(Iterator.from(rest).toArray(), [0, 0, 10, 0, 0, 10])
})

test('CPU and shader surfaces agree throughout a continuous combined motion', () => {
  const rest = new Float32Array([0, 0, 10, 0, 0, 10])
  for (const fps of [30, 60, 120]) {
    const cpu = { rest, deformed: rest.slice(), indices: new Uint16Array([0, 1, 2]) }
    const shader = { ...cpu, deformed: rest.slice(), transform: new Float32Array(9) }
    const a = bindAttachmentMesh(cpu, 3, 2)!
    const b = bindAttachmentMesh(shader, 3, 2)!
    const pa = { x: 0, y: 0 }
    const pb = { x: 0, y: 0 }
    for (let frame = 0; frame <= fps * 4; frame++) {
      const t = frame / fps
      const angle = Math.sin(t * 2.3) * 0.7
      const c = Math.cos(angle)
      const s = Math.sin(angle)
      const x = Math.sin(t * 1.7) * 12
      const y = Math.cos(t * 0.9) * 6
      shader.transform.set([c, s, 0, -s, c, 0, x, y, 1])
      for (let i = 0; i < rest.length; i += 2) {
        // Local deformation must survive the parent's global rotation/translation.
        const lx = rest[i] + rest[i + 1] * Math.sin(t) * 0.2
        const ly = rest[i + 1] * (1 + Math.sin(t * 1.3) * 0.15)
        shader.deformed[i] = lx
        shader.deformed[i + 1] = ly
        cpu.deformed[i] = c * shader.deformed[i] - s * shader.deformed[i + 1] + x
        cpu.deformed[i + 1] = s * shader.deformed[i] + c * shader.deformed[i + 1] + y
      }
      sampleAttachmentMesh(a, pa)
      sampleAttachmentMesh(b, pb)
      assert.ok(Math.hypot(pa.x - pb.x, pa.y - pb.y) < 3e-6, `${fps} Hz frame ${frame}`)
    }
  }
})

test('an attachment at the mesh edge retains the final host tangent', () => {
  const rest = new Float32Array([0, 0, 10, 0, 0, 10])
  const mesh = { rest, deformed: new Float32Array([4, 2, 4, 22, -6, 2]), indices: new Uint16Array([0, 1, 2]) }
  const root = bindAttachmentMesh(mesh, 10, 0)!
  assert.equal(bindAttachmentMesh(mesh, 11, 0), null)
  const tangent = offsetAttachmentMeshSample(root, 1, 0)
  const a = { x: 0, y: 0 }
  const b = { x: 0, y: 0 }
  sampleAttachmentMesh(root, a)
  sampleAttachmentMesh(tangent, b)
  assert.ok(Math.abs(b.x - a.x) < 1e-6)
  assert.ok(Math.abs(b.y - a.y - 2) < 1e-6)
})
