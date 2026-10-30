// 图标类上传超限时在浏览器里缩小重编码，站长不用自己转码。
// GIF（可能是动图）、SVG、ICO 缩了会丢东西，原样交给调用方判断。
const SHRINKABLE = new Set(['image/png', 'image/jpeg', 'image/webp', 'image/bmp'])
const START_EDGE = 1024
const MIN_EDGE = 64

export function canShrinkImage(type: string): boolean {
  return SHRINKABLE.has(type)
}

function dataUrlBytes(url: string): number {
  const base64 = url.slice(url.indexOf(',') + 1)
  return Math.floor((base64.length * 3) / 4)
}

/** 依次缩小边长，先试 PNG（保留透明），再试 WebP；都放不下返回 null。 */
export async function shrinkImageToFit(file: Blob, maxBytes: number): Promise<string | null> {
  const bitmap = await createImageBitmap(file)
  try {
    const longest = Math.max(bitmap.width, bitmap.height)
    let edge = Math.min(longest, START_EDGE)
    while (edge >= MIN_EDGE) {
      const scale = edge / longest
      const canvas = document.createElement('canvas')
      canvas.width = Math.max(1, Math.round(bitmap.width * scale))
      canvas.height = Math.max(1, Math.round(bitmap.height * scale))
      const context = canvas.getContext('2d')
      if (!context) return null
      context.imageSmoothingQuality = 'high'
      context.drawImage(bitmap, 0, 0, canvas.width, canvas.height)
      for (const [type, quality] of [['image/png', undefined], ['image/webp', 0.9]] as const) {
        const url = canvas.toDataURL(type, quality)
        // 不支持 WebP 编码的浏览器会退回 PNG，照样按大小判断。
        if (dataUrlBytes(url) <= maxBytes) return url
      }
      edge = Math.floor(edge / 2)
    }
    return null
  } finally {
    bitmap.close()
  }
}
