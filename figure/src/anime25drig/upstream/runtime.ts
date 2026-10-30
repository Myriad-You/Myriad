import type {
  UpstreamRig,
  UpstreamRuntimeBoundLayer,
  UpstreamRuntimeDrawCommand,
  UpstreamRuntimeExpression,
  UpstreamRuntimeFrame,
  UpstreamRuntimeLayer,
  UpstreamRuntimeParameters,
  UpstreamRuntimeRigBinding,
  UpstreamRuntimeTickInput,
  UpstreamRuntimeTickState,
} from './types'
import { baseName } from './rigger'

export const UPSTREAM_RUNTIME_DEFAULTS: Readonly<UpstreamRuntimeParameters> = {
  angleX: 0,
  angleY: 0,
  angleZ: 0,
  eyeOpenL: 1,
  eyeOpenR: 1,
  eyeX: 0,
  eyeY: 0,
  brow: 0,
  mouthOpen: 0,
  mouthForm: 0,
  mouthCY: 0,
  body: 0,
  physAmp: 2,
  soft: 2,
  browAngL: 0,
  browAngR: 0,
  browAngSym: 0,
  bangL: 0,
  bangC: 0,
  bangR: 0,
  armY: 0,
  armPos: 0,
  bust: 2.5,
  bustY: 1,
  irisScale: 1,
  mouthEase: 0.45,
  eyeEase: 0.3,
  fhAmp: 2,
  fhSoft: 0.4,
  eyeCY: 0,
  eyeCAng: 0,
  mouthCAng: 0,
  eyeScaleL: 1,
  eyeScaleR: 1,
  mouthScale: 1,
}

/** Exact CPU-side mesh binding from the pinned upstream `applyRig` function. */
export function bindUpstreamRuntimeRig(
  rig: Readonly<UpstreamRig>,
): UpstreamRuntimeRigBinding {
  const anchors = rig.anchors
  const faceWidth = anchors.face.x1 - anchors.face.x0
  const faceHeight = anchors.face.y1 - anchors.face.y0
  const layers = rig.layers.map((source): UpstreamRuntimeBoundLayer => {
    const cell = (source.phys ? 30 : 42) * Math.max(0.6, rig.canvas.w / 768)
    const columns = Math.max(2, Math.round(source.w / cell))
    const rows = Math.max(2, Math.round(source.h / cell))
    const vertexCount = (columns + 1) * (rows + 1)
    const base = new Float32Array(vertexCount * 2)
    const uv = new Float32Array(vertexCount * 2)
    let offset = 0
    for (let row = 0; row <= rows; row += 1) {
      for (let column = 0; column <= columns; column += 1) {
        base[offset] = source.x + (source.w * column) / columns
        base[offset + 1] = source.y + (source.h * row) / rows
        uv[offset] = column / columns
        uv[offset + 1] = row / rows
        offset += 2
      }
    }

    const indexValues: number[] = []
    for (let row = 0; row < rows; row += 1) {
      for (let column = 0; column < columns; column += 1) {
        const upperLeft = row * (columns + 1) + column
        const upperRight = upperLeft + 1
        const lowerLeft = upperLeft + columns + 1
        const lowerRight = lowerLeft + 1
        indexValues.push(
          upperLeft,
          upperRight,
          lowerLeft,
          upperRight,
          lowerRight,
          lowerLeft,
        )
      }
    }

    const layer: UpstreamRuntimeBoundLayer = {
      name: source.name,
      bn: baseName(source.name.replace(/_(l|r)$/, '')),
      group: source.group,
      side: source.side,
      fade: source.fade,
      x: source.x,
      y: source.y,
      w: source.w,
      h: source.h,
      z: source.z,
      depth: source.depth,
      phys: source.phys,
      synthetic: source.synthetic,
      base,
      cur: new Float32Array(base),
      uv,
      indices: new Uint16Array(indexValues),
      nIdx: indexValues.length,
      strands: source.strands,
    }

    if (source.strands?.length) {
      bindUpstreamHairWeights(layer, anchors.face)
    }
    return layer
  })

  return {
    canvas: { w: rig.canvas.w, h: rig.canvas.h },
    anchors,
    faceScale: anchors.faceScale,
    neckPivot: anchors.neckPivot,
    bodyPivot: anchors.bodyPivot,
    faceCenter: { x: anchors.face.cx, y: anchors.face.cy },
    chest: {
      cx: anchors.neckPivot.cx,
      cy: anchors.neckBottom + faceHeight * 0.6,
      rx: faceWidth * 0.6,
      ry: faceHeight * 0.45,
    },
    layers,
  }
}

function bindUpstreamHairWeights(
  layer: UpstreamRuntimeBoundLayer,
  face: Readonly<UpstreamRig['anchors']['face']>,
): void {
  const strands = layer.strands!
  const strandCount = strands.length
  const vertexCount = layer.base.length / 2
  let spacing = 120
  if (strandCount > 1) {
    const distances: number[] = []
    for (let strand = 1; strand < strandCount; strand += 1) {
      distances.push(strands[strand].x - strands[strand - 1].x)
    }
    distances.sort((left, right) => left - right)
    spacing = distances[distances.length >> 1]
  }
  const sigma = spacing * 0.6
  const strandWeights = new Float32Array(vertexCount * strandCount)
  const strandProgress = new Float32Array(vertexCount)
  const springs = strands.map((_strand, index) => ({
    stiff: { x: 0, v: 0, dx: 0 },
    soft: { x: 0, v: 0, dx: 0 },
    phase: index * 1.37 + layer.z,
  }))

  for (let vertex = 0; vertex < vertexCount; vertex += 1) {
    const x = layer.base[vertex * 2]
    const y = layer.base[vertex * 2 + 1]
    let total = 0
    for (let strand = 0; strand < strandCount; strand += 1) {
      const weight = Math.exp(
        -(((x - strands[strand].x) / sigma) ** 2),
      )
      strandWeights[vertex * strandCount + strand] = weight
      total += weight
    }
    let rootY = 0
    let tipY = 0
    if (total > 1e-6) {
      for (let strand = 0; strand < strandCount; strand += 1) {
        const weightIndex = vertex * strandCount + strand
        strandWeights[weightIndex] /= total
        rootY += strandWeights[weightIndex] * strands[strand].rootY
        tipY += strandWeights[weightIndex] * strands[strand].tipY
      }
    } else {
      strandWeights[vertex * strandCount] = 1
      rootY = strands[0].rootY
      tipY = strands[0].tipY
    }
    strandProgress[vertex] = Math.min(
      1,
      Math.max(0, (y - rootY) / Math.max(1, tipY - rootY)),
    )
  }
  layer.sw = strandWeights
  layer.su = strandProgress
  layer.spr = springs

  if (layer.bn !== 'front hair') return
  const faceWidth = face.x1 - face.x0
  const leftBoundary = face.cx - faceWidth * 0.22
  const rightBoundary = face.cx + faceWidth * 0.22
  const bangWeights = new Float32Array(vertexCount * 3)
  for (let vertex = 0; vertex < vertexCount; vertex += 1) {
    const x = layer.base[vertex * 2]
    const leftMix = upstreamRuntimeSmooth((x - leftBoundary) / 36 + 0.5)
    const rightMix = upstreamRuntimeSmooth((x - rightBoundary) / 36 + 0.5)
    bangWeights[vertex * 3] = 1 - leftMix
    bangWeights[vertex * 3 + 1] = leftMix * (1 - rightMix)
    bangWeights[vertex * 3 + 2] = rightMix
  }
  layer.bw = bangWeights
}

export function createUpstreamRuntimeTickState(
  initialTimeMs = 0,
  initial: Readonly<UpstreamRuntimeParameters> = UPSTREAM_RUNTIME_DEFAULTS,
): UpstreamRuntimeTickState {
  const current = { ...initial }
  return {
    lastTimeMs: initialTimeMs,
    blinkElapsed: -1,
    nextBlinkAtMs: initialTimeMs + 1_800,
    cameraPhysicsScale: 1,
    current,
    expression: { ...current, breath: 0, breathHead: 0 },
    bounce: { x: 0, v: 0, dy: 0 },
  }
}

/** Exact state advance from the pinned upstream `tick` loop. */
export function stepUpstreamRuntimeTick(
  state: UpstreamRuntimeTickState,
  input: Readonly<UpstreamRuntimeTickInput>,
): UpstreamRuntimeExpression {
  const deltaTime = Math.min(
    0.05,
    (input.nowMs - state.lastTimeMs) / 1_000,
  )
  state.lastTimeMs = input.nowMs
  const time = input.nowMs / 1_000
  const target = { ...input.target }

  if (input.automation.idle && !input.cameraLive) {
    target.angleX += 0.13 * Math.sin(time * 0.42) + 0.05 * Math.sin(time * 1.13)
    target.angleY += 0.08 * Math.sin(time * 0.31 + 1.7)
    target.angleZ += 0.07 * Math.sin(time * 0.23 + 0.5)
    target.body += 0.1 * Math.sin(time * 0.19 + 2.1)
  }

  if (input.automation.blink) {
    if (state.blinkElapsed < 0 && input.nowMs > state.nextBlinkAtMs) {
      state.blinkElapsed = 0
      state.nextBlinkAtMs = input.nowMs + 1_600 + input.random() * 3_800
      if (input.random() < 0.18) state.nextBlinkAtMs = input.nowMs + 280
    }
    if (state.blinkElapsed >= 0) {
      state.blinkElapsed += deltaTime
      const elapsed = state.blinkElapsed
      let eyeOpen: number
      if (elapsed < 0.08) {
        eyeOpen = 1 - elapsed / 0.08
      } else if (elapsed < 0.42) {
        eyeOpen = 0
      } else if (elapsed < 0.58) {
        eyeOpen = (elapsed - 0.42) / 0.16
      } else {
        eyeOpen = 1
        state.blinkElapsed = -1
      }
      target.eyeOpenL = Math.min(target.eyeOpenL, eyeOpen)
      target.eyeOpenR = Math.min(target.eyeOpenR, eyeOpen)
    }
  }

  const response = Math.min(1, deltaTime * 14)
  for (const key of Object.keys(
    state.current,
  ) as Array<keyof UpstreamRuntimeParameters>) {
    state.current[key] += (target[key] - state.current[key]) * response
  }

  const expression = Object.assign(state.expression, state.current)
  state.cameraPhysicsScale +=
    ((input.cameraLive ? 0.5 : 1) - state.cameraPhysicsScale) *
    Math.min(1, deltaTime * 4)
  expression.physAmp *= state.cameraPhysicsScale
  expression.soft *= state.cameraPhysicsScale
  expression.fhAmp *= state.cameraPhysicsScale
  expression.fhSoft *= state.cameraPhysicsScale
  expression.breath = 0.5 + 0.5 * Math.sin((time * 2 * Math.PI) / 3.4)
  expression.breathHead =
    0.5 + 0.5 * Math.sin((time * 2 * Math.PI) / 3.4 - 0.6)

  const headDisplacement =
    (expression.angleX * 14 +
      expression.angleZ * 0.07 *
        (input.frame.neckPivot.cy - input.frame.faceCenter.y)) *
    input.frame.faceScale
  for (const layer of input.layers) {
    if (!layer.spr) continue
    for (const spring of layer.spr) {
      const wind = input.automation.idle
        ? 1.8 * Math.sin(time * 0.8 + spring.phase) +
          1 * Math.sin(time * 1.9 + spring.phase * 2.3)
        : 0
      const targetPosition = headDisplacement + wind * input.frame.faceScale
      let stiffness = 70
      let damping = 9
      let acceleration =
        -stiffness * (spring.stiff.x - targetPosition) -
        damping * spring.stiff.v
      spring.stiff.v += acceleration * deltaTime
      spring.stiff.x += spring.stiff.v * deltaTime
      spring.stiff.dx = -(spring.stiff.x - targetPosition) * 2.2
      stiffness = 16
      damping = 1.3
      acceleration =
        -stiffness * (spring.soft.x - targetPosition) -
        damping * spring.soft.v
      spring.soft.v += acceleration * deltaTime
      spring.soft.x += spring.soft.v * deltaTime
      spring.soft.dx = -(spring.soft.x - targetPosition) * 3
    }
  }

  const bustTarget =
    (expression.breath * 3 - expression.angleY * 6 + expression.body * 4) *
    input.frame.faceScale
  const bustAcceleration =
    -140 * (state.bounce.x - bustTarget) - 4.2 * state.bounce.v
  state.bounce.v += bustAcceleration * deltaTime
  state.bounce.x += state.bounce.v * deltaTime
  state.bounce.dy = -(state.bounce.x - bustTarget) * 3
  return expression
}

/** Exact pure-function port of the runtime helpers embedded in upstream index.html. */
export function upstreamRuntimeSmooth(value: number): number {
  const bounded = value < 0 ? 0 : value > 1 ? 1 : value
  return bounded * bounded * (3 - 2 * bounded)
}

export function upstreamRuntimeFadeAlpha(
  layer: Pick<UpstreamRuntimeLayer, 'fade' | 'side'>,
  expression: Pick<
    UpstreamRuntimeExpression,
    'eyeEase' | 'eyeOpenL' | 'eyeOpenR' | 'mouthEase' | 'mouthOpen'
  >,
): number {
  if (!layer.fade) return 1
  if (layer.fade === 'eyeOpen') {
    const value = layer.side === 'L' ? expression.eyeOpenL : expression.eyeOpenR
    return upstreamRuntimeSmooth(
      (value - (0.1 + expression.eyeEase * 0.45)) / 0.15,
    )
  }
  if (layer.fade === 'eyeClose') {
    const value = layer.side === 'L' ? expression.eyeOpenL : expression.eyeOpenR
    return (
      1 -
      upstreamRuntimeSmooth(
        (value - (0.1 + expression.eyeEase * 0.45)) / 0.15,
      )
    )
  }
  if (layer.fade === 'mouthOpen') {
    return upstreamRuntimeSmooth(
      (expression.mouthOpen - (0.05 + expression.mouthEase * 0.35)) / 0.12,
    )
  }
  if (layer.fade === 'mouthClose') {
    return (
      1 -
      upstreamRuntimeSmooth(
        (expression.mouthOpen - (0.05 + expression.mouthEase * 0.35)) / 0.12,
      )
    )
  }
  return 1
}

/** Layer traversal and eye stencil only; WebGL stays outside this module. */
export function planUpstreamRuntimeDraw(
  layers: ReadonlyArray<
    Pick<UpstreamRuntimeLayer, 'name' | 'fade' | 'side'>
  >,
  expression: Pick<
    UpstreamRuntimeExpression,
    'eyeEase' | 'eyeOpenL' | 'eyeOpenR' | 'mouthEase' | 'mouthOpen'
  >,
): UpstreamRuntimeDrawCommand[] {
  const commands: UpstreamRuntimeDrawCommand[] = []
  for (let layerIndex = 0; layerIndex < layers.length; layerIndex += 1) {
    const layer = layers[layerIndex]
    const alpha = upstreamRuntimeFadeAlpha(layer, expression)
    const isEyeWhite = layer.name.indexOf('eyewhite') === 0
    if (alpha < 0.004 && !(layer.fade === 'eyeOpen' && isEyeWhite)) continue
    commands.push({
      layerIndex,
      name: layer.name,
      alpha,
      alphaCut: isEyeWhite ? 0.25 : 0,
      stencil: isEyeWhite
        ? 'write'
        : layer.name.indexOf('irides') === 0
          ? 'test'
          : 'none',
    })
  }
  return commands
}

/** Writes vertex deformation into `layer.cur`. */
export function deformUpstreamRuntimeLayer(
  layer: UpstreamRuntimeLayer,
  expression: Readonly<UpstreamRuntimeExpression>,
  frame: Readonly<UpstreamRuntimeFrame>,
): void {
  const base = layer.base
  const output = layer.cur
  const length = base.length
  const isHead = layer.group === 'head'
  const angleZ = expression.angleZ * 0.07
  const cosZ = Math.cos(angleZ)
  const sinZ = Math.sin(angleZ)
  const bodyAngle = expression.body * 0.028
  const cosBody = Math.cos(bodyAngle)
  const sinBody = Math.sin(bodyAngle)
  const baseName = layer.bn
  const eyeSide = layer.side
  const eyeAnchor =
    eyeSide === 'L'
      ? frame.anchors.eyeL
      : eyeSide === 'R'
        ? frame.anchors.eyeR
        : null
  const eyeOpen =
    eyeSide === 'L' ? expression.eyeOpenL : expression.eyeOpenR
  const mouthOpen = expression.mouthOpen
  const mouthHalfWidth =
    (frame.anchors.mouth.x1 - frame.anchors.mouth.x0) / 2
  const strandCount = layer.strands ? layer.strands.length : 0
  const layerCenterX = layer.x + layer.w / 2
  const layerCenterY = layer.y + layer.h / 2
  const isFrontHair = baseName === 'front hair'

  for (let offset = 0; offset < length; offset += 2) {
    let x = base[offset]
    let y = base[offset + 1]
    const vertexIndex = offset >> 1

    if (eyeAnchor && baseName === 'eye_close') {
      const scale =
        eyeSide === 'L' ? expression.eyeScaleL : expression.eyeScaleR
      if (scale !== 1) {
        const centerX = (eyeAnchor.x0 + eyeAnchor.x1) / 2
        const centerY = (eyeAnchor.y0 + eyeAnchor.y1) / 2
        x = centerX + (x - centerX) * scale
        y = centerY + (y - centerY) * scale
      }
    }
    if (baseName === 'mouth_open' || baseName === 'mouth_close') {
      const scale = expression.mouthScale
      if (scale !== 1) {
        x =
          frame.anchors.mouth.cx +
          (x - frame.anchors.mouth.cx) * scale
        y =
          frame.anchors.mouth.cy +
          (y - frame.anchors.mouth.cy) * scale
      }
    }
    if (layer.fade === 'eyeOpen' && eyeAnchor) {
      if (baseName === 'irides') {
        const irisScale = expression.irisScale
        x = eyeAnchor.icx + (x - eyeAnchor.icx) * irisScale
        y = eyeAnchor.icy + (y - eyeAnchor.icy) * irisScale
        x += expression.eyeX * 11 * frame.faceScale
        y += expression.eyeY * 6 * frame.faceScale
        const closing = upstreamRuntimeSmooth((0.32 - eyeOpen) / 0.32)
        y = eyeAnchor.closeY + (y - eyeAnchor.closeY) * (1 - 0.8 * closing)
      } else {
        y = eyeAnchor.closeY + (y - eyeAnchor.closeY) * (1 - 0.85 * (1 - eyeOpen))
      }
    }
    if (layer.fade === 'eyeClose' && eyeAnchor) {
      y -= eyeOpen * 3
      y += expression.eyeCY * 14 * frame.faceScale
      const rotation = expression.eyeCAng * 0.3 * (eyeSide === 'L' ? 1 : -1)
      if (rotation) {
        const cosRotation = Math.cos(rotation)
        const sinRotation = Math.sin(rotation)
        const relativeX = x - layerCenterX
        const relativeY = y - layerCenterY
        x = layerCenterX + relativeX * cosRotation - relativeY * sinRotation
        y = layerCenterY + relativeX * sinRotation + relativeY * cosRotation
      }
    }
    if (baseName === 'eyebrow') {
      y += (-expression.brow * 9 + (1 - eyeOpen) * 3.5) * frame.faceScale
      const rotation =
        (eyeSide === 'L'
          ? expression.browAngL + expression.browAngSym
          : expression.browAngR - expression.browAngSym) * 0.3
      if (rotation) {
        const cosRotation = Math.cos(rotation)
        const sinRotation = Math.sin(rotation)
        const relativeX = x - layerCenterX
        const relativeY = y - layerCenterY
        x = layerCenterX + relativeX * cosRotation - relativeY * sinRotation
        y = layerCenterY + relativeX * sinRotation + relativeY * cosRotation
      }
    }
    if (layer.fade === 'mouthOpen') {
      y =
        frame.anchors.mouth.y0 +
        (y - frame.anchors.mouth.y0) * (0.5 + 0.5 * mouthOpen)
      const horizontal = Math.abs(x - frame.anchors.mouth.cx)
      const shape = (horizontal / (mouthHalfWidth + 4)) ** 1.5
      y -= expression.mouthForm * 6 * frame.faceScale * (shape - 0.35)
    }
    if (layer.fade === 'mouthClose') {
      y += expression.mouthCY * 14 * frame.faceScale
      const rotation = expression.mouthCAng * 0.35
      if (rotation) {
        const cosRotation = Math.cos(rotation)
        const sinRotation = Math.sin(rotation)
        const relativeX = x - frame.anchors.mouth.cx
        const relativeY = y - frame.anchors.mouth.cy
        x =
          frame.anchors.mouth.cx +
          relativeX * cosRotation -
          relativeY * sinRotation
        y =
          frame.anchors.mouth.cy +
          relativeX * sinRotation +
          relativeY * cosRotation
      }
    }
    if (baseName === 'face' && y > frame.anchors.mouth.cy) {
      y +=
        mouthOpen *
        6 *
        frame.faceScale *
        upstreamRuntimeSmooth(
          (y - frame.anchors.mouth.cy) /
            (frame.anchors.face.y1 - frame.anchors.mouth.cy),
        )
    }

    let headWeight = isHead ? 1 : layer.group === 'body' ? 0.16 : 0
    if (baseName === 'neck') {
      headWeight =
        0.55 *
        upstreamRuntimeSmooth(
          (frame.anchors.neckBottom - y) /
            Math.max(
              1,
              frame.anchors.neckBottom - frame.anchors.neckTop,
            ),
        )
    }
    if (headWeight > 0) {
      const relativeX = x - frame.neckPivot.cx
      const relativeY = y - frame.neckPivot.cy
      const rotatedX = relativeX * cosZ - relativeY * sinZ
      const rotatedY = relativeX * sinZ + relativeY * cosZ
      x += (rotatedX - relativeX) * headWeight
      y += (rotatedY - relativeY) * headWeight
      const depth = layer.depth
      x +=
        headWeight *
        frame.faceScale *
        (expression.angleX * (14 + 40 * (depth - 1)) +
          expression.angleX * (frame.neckPivot.cy - y) * 0.028)
      y +=
        headWeight *
        frame.faceScale *
        (-expression.angleY * (9 + 30 * (depth - 1)) -
          expression.angleY * (depth - 1) * (y - frame.faceCenter.y) * 0.05)
    }

    y -=
      (layer.group === 'body'
        ? expression.breath * 2
        : expression.breathHead * 1.6) * frame.faceScale
    if (baseName === 'topwear' && y < frame.chest.cy) {
      y -=
        expression.breath *
        2.2 *
        frame.faceScale *
        upstreamRuntimeSmooth(
          (frame.chest.cy - y) / (frame.chest.ry * 2),
        )
    }
    if (baseName === 'topwear') {
      x =
        frame.neckPivot.cx +
        (x - frame.neckPivot.cx) * (1 + expression.breath * 0.003)
      const chestX = (x - frame.chest.cx) / frame.chest.rx
      const chestY =
        (y - (frame.chest.cy + expression.bustY * 70 * frame.faceScale)) /
        frame.chest.ry
      y +=
        frame.bustDisplacement *
        expression.bust *
        Math.exp(-chestX * chestX - chestY * chestY)
    }
    if (baseName === 'handwear') {
      const weight = upstreamRuntimeSmooth(((y - layer.y) / layer.h) * 1.15)
      y -= expression.armY * 30 * frame.faceScale * weight
      y += expression.armPos * 40 * frame.faceScale
      x +=
        expression.armY *
        6 *
        frame.faceScale *
        weight *
        (x < frame.neckPivot.cx ? 1 : -1)
    }
    if (layer.bw && layer.su) {
      const magnitude =
        layer.su[vertexIndex] ** 1.4 * 22 * frame.faceScale
      x +=
        (expression.bangL * layer.bw[vertexIndex * 3] +
          expression.bangC * layer.bw[vertexIndex * 3 + 1] +
          expression.bangR * layer.bw[vertexIndex * 3 + 2]) * magnitude
    }
    if (strandCount && frame.physicsEnabled) {
      const position = isFrontHair
        ? Math.min(1, layer.su![vertexIndex] * 1.6)
        : layer.su![vertexIndex]
      const amplitude =
        position ** (isFrontHair ? 1.8 : 2.1) *
        (isFrontHair ? expression.fhAmp : expression.physAmp)
      const softMix =
        position ** 1.2 *
        (isFrontHair ? expression.fhSoft : expression.soft)
      let displacementX = 0
      for (let strand = 0; strand < strandCount; strand += 1) {
        const weight = layer.sw![vertexIndex * strandCount + strand]
        if (weight < 0.001) continue
        const spring = layer.spr![strand]
        displacementX +=
          weight *
          (spring.stiff.dx * (1 - softMix) + spring.soft.dx * softMix)
      }
      x += displacementX * amplitude
      y += Math.abs(displacementX) * amplitude * 0.12
    }
    output[offset] = x
    output[offset + 1] = y
  }

  if (Math.abs(bodyAngle) > 0.0001) {
    for (let offset = 0; offset < length; offset += 2) {
      const relativeX = output[offset] - frame.bodyPivot.cx
      const relativeY = output[offset + 1] - frame.bodyPivot.cy
      output[offset] =
        frame.bodyPivot.cx + relativeX * cosBody - relativeY * sinBody
      output[offset + 1] =
        frame.bodyPivot.cy + relativeX * sinBody + relativeY * cosBody
    }
  }
}
