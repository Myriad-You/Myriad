import type { AttachmentMesh, AttachmentMeshSample } from './attachmentMesh'
import {
  bindAttachmentMesh,
  offsetAttachmentMeshSample,
  sampleAttachmentMesh,
} from './attachmentMesh'

export interface SurfaceContact {
  samples: (AttachmentMeshSample | null)[]
  weights: Float32Array
  point: { x: number; y: number }
  /** Primary child geometry, kept separate from the final surface consumed by dependents. */
  unconstrained: Float32Array
}

/** The caller supplies an evidence-backed contact field, never a proximity-only guess. */
export function bindSurfaceContact(
  host: AttachmentMesh,
  rest: Float32Array,
  weights: Float32Array,
): SurfaceContact {
  if (
    rest.length !== weights.length * 2 ||
    weights.some((w) => !Number.isFinite(w) || w < 0 || w > 1)
  ) {
    throw new Error(
      'Surface contact requires one finite unit weight per vertex',
    )
  }
  const samples = Iterator.from(weights.entries())
    .map(([vertex, weight]) => {
      if (weight <= 0) return null
      const x = rest[vertex * 2]
      const y = rest[vertex * 2 + 1]
      const inside = bindAttachmentMesh(host, x, y)
      if (inside) return inside
      // Extend the boundary differential into the contact feather, preserving rest
      // positions rather than snapping the arm onto the host's mesh edge.
      let nearest = 0
      let distance = Infinity
      for (const index of host.indices) {
        const d = Math.hypot(
          host.rest[index * 2] - x,
          host.rest[index * 2 + 1] - y,
        )
        if (d < distance) {
          distance = d
          nearest = index * 2
        }
      }
      const root = bindAttachmentMesh(
        host,
        host.rest[nearest],
        host.rest[nearest + 1],
      )
      if (!root)
        throw new Error('Cannot bind contact to a degenerate host surface')
      return offsetAttachmentMeshSample(root, x - root.x, y - root.y)
    })
    .toArray()
  return {
    samples,
    weights: weights.slice(),
    point: { x: 0, y: 0 },
    unconstrained: rest.slice(),
  }
}

/** Blend freshly evaluated child vertices in model space, before common body/view transforms. */
export function applySurfaceContact(
  contact: SurfaceContact,
  unconstrained: Float32Array,
  output: Float32Array = unconstrained,
): boolean {
  let changed = false
  for (let vertex = 0; vertex < contact.samples.length; vertex++) {
    const sample = contact.samples[vertex]
    const i = vertex * 2
    let x = unconstrained[i]
    let y = unconstrained[i + 1]
    if (sample) {
      sampleAttachmentMesh(sample, contact.point)
      const w = contact.weights[vertex]
      x += (contact.point.x - x) * w
      y += (contact.point.y - y) * w
    }
    x = Math.fround(x)
    y = Math.fround(y)
    if (output[i] !== x || output[i + 1] !== y) {
      changed = true
      output[i] = x
      output[i + 1] = y
    }
  }
  return changed
}
