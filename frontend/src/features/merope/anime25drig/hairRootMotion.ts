import type { AttachmentMeshSample } from './attachmentMesh'
import type { BodyLift } from './bodyLift'
import type { Anime25DSecondaryDeformationBinding, Anime25DSecondaryDeformationFrame } from './secondaryDeformation'
import type { Anime25DPlaybackLayer } from './types'
import { bindAttachmentMesh, offsetAttachmentMeshSample } from './attachmentMesh'
import { applyBodyLift } from './bodyLift'
import { deformAnime25DSecondaryPoint } from './secondaryDeformation'

export interface HairRootMotion {
  samples: AttachmentMeshSample[]
  deformation: Anime25DSecondaryDeformationBinding
  point: { x: number; y: number }
}

/** Root samples use the same primary deformation as the hair, never its lagged tips. */
export function bindHairRootMotion(
  source: Anime25DPlaybackLayer,
  deformation: Anime25DSecondaryDeformationBinding,
  rest: Float32Array,
  indices: Uint16Array,
): HairRootMotion | null {
  if (!deformation.springs?.length) return null
  const mesh = { rest, deformed: rest, indices }
  let minX = Infinity; let minY = Infinity; let maxX = -Infinity; let maxY = -Infinity
  for (let i = 0; i < rest.length; i += 2) {
    minX = Math.min(minX, rest[i]); maxX = Math.max(maxX, rest[i])
    minY = Math.min(minY, rest[i + 1]); maxY = Math.max(maxY, rest[i + 1])
  }
  const samples = source.strands.map(strand => {
    const x = Math.max(minX, Math.min(maxX, strand.x))
    const y = Math.max(minY, Math.min(maxY, strand.rootY))
    const sample = bindAttachmentMesh(mesh, x, y)
    if (!sample) throw new Error('Hair root must bind to its generated mesh')
    // A root may be above a cropped lock. Extend the boundary triangle's
    // deformation, rather than inventing a separate head transform there.
    return offsetAttachmentMeshSample(sample, strand.x - x, strand.rootY - y)
  })
  return {
    samples,
    point: { x: 0, y: 0 },
    deformation,
  }
}

export function writeHairRootMotion(
  roots: HairRootMotion,
  frame: Readonly<Anime25DSecondaryDeformationFrame>,
  bodyPivotX: number,
  bodyPivotY: number,
  bodyCosine: number,
  bodySine: number,
  bodyLift?: Readonly<BodyLift>,
): void {
  const { samples, point, deformation } = roots
  for (let i = 0; i < samples.length; i++) {
    const sample = samples[i]
    const rest = sample.mesh.rest
    let x = sample.x; let y = sample.y
    for (let k = 0; k < 3; k++) {
      const vertex = sample.indices[k]
      const rx = rest[vertex * 2]; const ry = rest[vertex * 2 + 1]
      point.x = rx; point.y = ry
      deformAnime25DSecondaryPoint(point, rx, ry, vertex, deformation, frame)
      x += (Math.fround(point.x) - rx) * sample.weights[k]
      y += (Math.fround(point.y) - ry) * sample.weights[k]
    }
    // The body rotation is shader-owned for the visible mesh. Include it once
    // in the support input, after the local head/shell and torso translation.
    deformation.springs![i].supportX = bodyPivotX +
      (x - bodyPivotX) * bodyCosine - (y - bodyPivotY) * bodySine - sample.x
    deformation.springs![i].supportY = bodyPivotY +
      (x - bodyPivotX) * bodySine + (y - bodyPivotY) * bodyCosine - sample.y
    point.x = deformation.springs![i].supportX + sample.x
    point.y = deformation.springs![i].supportY + sample.y
    applyBodyLift(point, bodyLift)
    deformation.springs![i].supportX = point.x - sample.x
    deformation.springs![i].supportY = point.y - sample.y
  }
}
