import { useCallback, useEffect, useRef, useState } from 'react'
import { loadImagePooled } from '../utils/objectPool'
import { observeIntersection } from './animation'

interface UseLazyImageOptions {
  threshold?: number
  rootMargin?: string
  placeholder?: string
}

/**
 * 图片懒加载 Hook - 使用共享 Intersection Observer 实现懒加载
 *
 * 性能优化：
 * - 使用对象池加载图片，减少 GC 压力
 * - 使用共享 IntersectionObserver（通过 AnimationCoordinator）
 *
 * @param src 图片源地址
 * @param options 配置项
 * @returns 当前显示的图片地址、加载状态和 ref 回调
 */
export function useLazyImage(
  src: string,
  options: UseLazyImageOptions = {},
): {
  imageSrc: string
  isLoading: boolean
  hasError: boolean
  containerRef: (node: Element | null) => void
} {
  const {
    threshold = 0.01,
    rootMargin = '50px',
    placeholder = '',
  } = options

  const [imageSrc, setImageSrc] = useState<string>(placeholder)
  const [isLoading, setIsLoading] = useState<boolean>(true)
  const [hasError, setHasError] = useState<boolean>(false)
  const unobserveRef = useRef<(() => void) | null>(null)
  const abortedRef = useRef<boolean>(false)
  const hasLoadedRef = useRef<boolean>(false)
  const elementRef = useRef<Element | null>(null)

  const loadImage = useCallback(async () => {
    if (hasLoadedRef.current || !src)
      return
    hasLoadedRef.current = true

    setIsLoading(true)
    setHasError(false)

    // 使用池化的图片加载
    const success = await loadImagePooled(src, { timeout: 15000 })

    // 检查是否已被取消
    if (abortedRef.current)
      return

    if (success) {
      setImageSrc(src)
      setIsLoading(false)
    }
    else {
      setHasError(true)
      setIsLoading(false)
    }
  }, [src])

  // Ref callback - 连接到共享 IntersectionObserver
  const containerRef = useCallback((node: Element | null) => {
    // 清理旧观察
    if (unobserveRef.current) {
      unobserveRef.current()
      unobserveRef.current = null
    }

    elementRef.current = node

    if (node && !hasLoadedRef.current) {
      if ('IntersectionObserver' in window) {
        unobserveRef.current = observeIntersection(
          node,
          (entry) => {
            if (entry.isIntersecting) {
              loadImage()
              // 图片开始加载后取消观察
              if (unobserveRef.current) {
                unobserveRef.current()
                unobserveRef.current = null
              }
            }
          },
          { threshold, rootMargin },
        )
      }
      else {
        // 不支持 IntersectionObserver 的浏览器直接加载
        loadImage()
      }
    }
  }, [threshold, rootMargin, loadImage])

  // src 变化时重置状态
  useEffect(() => {
    if (!src)
      return

    abortedRef.current = false
    hasLoadedRef.current = false
    setImageSrc(placeholder)
    setIsLoading(true)
    setHasError(false)

    // 如果已经有元素在观察，重新设置观察
    if (elementRef.current) {
      containerRef(elementRef.current)
    }

    return () => {
      abortedRef.current = true
      if (unobserveRef.current) {
        unobserveRef.current()
        unobserveRef.current = null
      }
    }
  }, [src, placeholder, containerRef])

  return { imageSrc, isLoading, hasError, containerRef }
}
