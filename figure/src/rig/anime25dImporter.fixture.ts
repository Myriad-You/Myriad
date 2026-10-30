import type { Layer, Psd } from 'ag-psd'

export function syntheticSeeThroughPsd(): Psd {
  const width = 256
  const height = 256
  const layer = (
    name: string,
    rectangles: Array<[number, number, number, number]>,
  ): Layer => {
    const data = new Uint8ClampedArray(width * height * 4)
    for (const [left, top, right, bottom] of rectangles) {
      for (let y = top; y < bottom; y += 1) {
        for (let x = left; x < right; x += 1) {
          const index = (y * width + x) * 4
          data[index] = 48
          data[index + 1] = 32
          data[index + 2] = 24
          data[index + 3] = 255
        }
      }
    }
    return { name, left: 0, top: 0, imageData: { width, height, data } }
  }
  return {
    width,
    height,
    children: [
      layer('back hair', [[52, 18, 204, 220]]),
      layer('handwear', [
        [12, 132, 72, 244],
        [184, 132, 244, 244],
      ]),
      layer('bottomwear', [[54, 184, 202, 254]]),
      layer('legwear', [[70, 210, 186, 256]]),
      layer('topwear', [[48, 116, 208, 212]]),
      layer('ears', [
        [48, 62, 72, 118],
        [184, 62, 208, 118],
      ]),
      layer('face', [[66, 34, 190, 142]]),
      layer('nose', [[122, 84, 134, 104]]),
      layer('mouth', [[108, 112, 148, 130]]),
      layer('eyewhite', [
        [82, 70, 112, 88],
        [144, 70, 174, 88],
      ]),
      layer('eyelash', [
        [80, 66, 114, 72],
        [142, 66, 176, 72],
      ]),
      layer('eyelash_c', [
        [80, 76, 114, 80],
        [142, 76, 176, 80],
      ]),
      layer('irides', [
        [94, 72, 104, 84],
        [152, 72, 162, 84],
      ]),
      layer('eyebrow', [
        [82, 54, 112, 60],
        [144, 54, 174, 60],
      ]),
      layer('front hair_1', [
        [62, 18, 98, 104],
        [102, 12, 138, 116],
        [142, 18, 184, 102],
      ]),
      layer('mouth_c', [[108, 120, 148, 124]]),
    ],
  } as Psd
}
