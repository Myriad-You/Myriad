export interface Anime25DLayerGlobalTransformInput {
  headFollow: number
  headRotationCosine: number
  headRotationSine: number
  neckPivotX: number
  neckPivotY: number
  faceScale: number
  angleX: number
  angleY: number
  depthOffset: number
  faceCenterY: number
  specialOffsetY: number
  breathOffset: number
}

export function writeAnime25DLayerGlobalTransform(
  input: Anime25DLayerGlobalTransformInput,
  output: Float32Array,
): Float32Array {
  if (output.length !== 9) {
    throw new RangeError('Anime2.5D layer transform must contain nine values')
  }
  const follow = input.headFollow
  const cosine = input.headRotationCosine
  const sine = input.headRotationSine
  let a = 1 + follow * (cosine - 1)
  let b = follow * sine
  let c = -follow * sine
  let d = 1 + follow * (cosine - 1)
  let tx =
    follow *
    (input.neckPivotX - cosine * input.neckPivotX + sine * input.neckPivotY)
  let ty =
    follow *
    (input.neckPivotY - sine * input.neckPivotX - cosine * input.neckPivotY)

  const horizontalShear = follow * input.faceScale * input.angleX * 0.028
  const horizontalOffset =
    follow *
    input.faceScale *
    input.angleX *
    (14 + 40 * input.depthOffset + input.neckPivotY * 0.028)
  a -= horizontalShear * b
  c -= horizontalShear * d
  tx += horizontalOffset - horizontalShear * ty

  const verticalScale =
    1 - follow * input.faceScale * input.angleY * input.depthOffset * 0.05
  const verticalOffset =
    follow *
    input.faceScale *
    (-input.angleY * (9 + 30 * input.depthOffset) +
      input.angleY * input.depthOffset * input.faceCenterY * 0.05)
  b *= verticalScale
  d *= verticalScale
  ty = ty * verticalScale + verticalOffset
  ty += input.specialOffsetY - input.breathOffset * input.faceScale

  output[0] = a
  output[1] = b
  output[2] = 0
  output[3] = c
  output[4] = d
  output[5] = 0
  output[6] = tx
  output[7] = ty
  output[8] = 1
  return output
}

export function writeIdentityLayerTransform(output: Float32Array): void {
  output.fill(0)
  output[0] = 1
  output[4] = 1
  output[8] = 1
}
