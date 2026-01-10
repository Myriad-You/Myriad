/**
 * 图片优化工具
 * 支持响应式图片和WebP格式
 */

import { observeIntersection, unobserveIntersection } from '../hooks/animation'

/**
 * 生成响应式图片的srcset
 * @param src 原始图片路径
 * @param widths 需要的宽度数组
 * @returns srcset字符串
 */
export function generateSrcSet(src: string, widths: number[] = [320, 640, 768, 1024, 1280, 1920]): string {
  if (!src)
    return ''

  const ext = src.split('.').pop()
  const basePath = src.replace(`.${ext}`, '')

  return widths
    .map(width => `${basePath}-${width}w.${ext} ${width}w`)
    .join(', ')
}

/**
 * 生成WebP格式的source标签
 * @param src 原始图片路径
 * @param widths 需要的宽度数组
 * @returns WebP srcset字符串
 */
export function generateWebPSrcSet(src: string, widths: number[] = [320, 640, 768, 1024, 1280, 1920]): string {
  if (!src)
    return ''

  const ext = src.split('.').pop()
  const basePath = src.replace(`.${ext}`, '')

  return widths
    .map(width => `${basePath}-${width}w.webp ${width}w`)
    .join(', ')
}

/**
 * 检测浏览器是否支持WebP
 */
export async function supportsWebP(): Promise<boolean> {
  if (typeof window === 'undefined')
    return false

  if ('ImageDecoder' in window) {
    const supported = await (window as any).ImageDecoder.isTypeSupported('image/webp')
    return supported
  }

  // Fallback检测
  return new Promise((resolve) => {
    const img = new Image()
    img.onload = () => resolve(img.width === 1)
    img.onerror = () => resolve(false)
    img.src = 'data:image/webp;base64,UklGRiQAAABXRUJQVlA4IBgAAAAwAQCdASoBAAEAAwA0JaQAA3AA/vuUAAA='
  })
}

/**
 * 获取最佳图片格式
 */
export async function getBestImageFormat(): Promise<'webp' | 'avif' | 'jpg'> {
  if (typeof window === 'undefined')
    return 'jpg'

  // 检测AVIF支持
  if ('ImageDecoder' in window) {
    try {
      const supported = await (window as any).ImageDecoder.isTypeSupported('image/avif')
      if (supported)
        return 'avif'
    }
    catch {}
  }

  // 检测WebP支持
  const webpSupported = await supportsWebP()
  if (webpSupported)
    return 'webp'

  return 'jpg'
}

/**
 * 懒加载图片
 * 使用共享 IntersectionObserver（通过 AnimationCoordinator）
 */
export function lazyLoadImage(img: HTMLImageElement) {
  if ('loading' in HTMLImageElement.prototype) {
    img.loading = 'lazy'
  }
  else {
    // Fallback: 使用共享 Intersection Observer
    observeIntersection(
      img,
      (entry) => {
        if (entry.isIntersecting) {
          const lazyImg = entry.target as HTMLImageElement
          if (lazyImg.dataset.src) {
            lazyImg.src = lazyImg.dataset.src
          }
          if (lazyImg.dataset.srcset) {
            lazyImg.srcset = lazyImg.dataset.srcset
          }
          unobserveIntersection(lazyImg)
        }
      },
      { rootMargin: '50px' },
    )
  }
}

/**
 * 图片压缩质量建议
 */
export const IMAGE_QUALITY = {
  thumbnail: 60, // 缩略图
  preview: 75, // 预览图
  full: 85, // 完整图
  lossless: 100, // 无损
}

/**
 * 图片尺寸建议
 */
export const IMAGE_SIZES = {
  thumbnail: 200,
  small: 400,
  medium: 800,
  large: 1200,
  xlarge: 1920,
}
