/** Bind once to the actual host mesh; sample final vertices without replaying physics. */
export interface AttachmentMesh {
  rest: Float32Array
  deformed: Float32Array
  indices: Uint16Array
  /** Remaining shader-owned layer transform; excludes the shared body/view transform. */
  transform?: Float32Array
}
export interface AttachmentMeshSample {
  mesh: AttachmentMesh
  indices: [number, number, number]
  weights: [number, number, number]
  x: number
  y: number
}
export function bindAttachmentMesh(
  mesh: AttachmentMesh,
  x: number,
  y: number,
): AttachmentMeshSample | null {
  const r = mesh.rest
  for (let t = 0; t < mesh.indices.length; t += 3) {
    const [a, b, c] = [
      mesh.indices[t],
      mesh.indices[t + 1],
      mesh.indices[t + 2],
    ]
    const ax = r[a * 2]
      const ay = r[a * 2 + 1]
      const bx = r[b * 2]
      const by = r[b * 2 + 1]
      const cx = r[c * 2]
      const cy = r[c * 2 + 1]
    const det = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy)
    if (Math.abs(det) < 1e-8) continue
    const u = ((by - cy) * (x - cx) + (cx - bx) * (y - cy)) / det
    const v = ((cy - ay) * (x - cx) + (ax - cx) * (y - cy)) / det
    const w = 1 - u - v
    if (Math.min(u, v, w) < -1e-5) continue
    return { mesh, indices: [a, b, c], weights: [u, v, w], x, y }
  }
  return null
}
export function sampleAttachmentMesh(
  sample: AttachmentMeshSample,
  output: { x: number; y: number },
): void {
  output.x = sample.x
  output.y = sample.y
  for (let k = 0; k < 3; k++) {
    const i = sample.indices[k] * 2
      const w = sample.weights[k]
    output.x += (sample.mesh.deformed[i] - sample.mesh.rest[i]) * w
    output.y += (sample.mesh.deformed[i + 1] - sample.mesh.rest[i + 1]) * w
  }
  const transform = sample.mesh.transform
  if (transform) {
    const { x, y } = output
    output.x = transform[0] * x + transform[3] * y + transform[6]
    output.y = transform[1] * x + transform[4] * y + transform[7]
  }
}

/** Sample the same triangle's differential, including at the host boundary. */
export function offsetAttachmentMeshSample(
  sample: AttachmentMeshSample,
  dx: number,
  dy: number,
): AttachmentMeshSample {
  const r = sample.mesh.rest
  const [a, b, c] = sample.indices.map((index) => index * 2)
  const det = (r[b + 1] - r[c + 1]) * (r[a] - r[c])
    + (r[c] - r[b]) * (r[a + 1] - r[c + 1])
  const du = ((r[b + 1] - r[c + 1]) * dx + (r[c] - r[b]) * dy) / det
  const dv = ((r[c + 1] - r[a + 1]) * dx + (r[a] - r[c]) * dy) / det
  return {
    mesh: sample.mesh,
    indices: sample.indices,
    weights: [sample.weights[0] + du, sample.weights[1] + dv, sample.weights[2] - du - dv],
    x: sample.x + dx,
    y: sample.y + dy,
  }
}
