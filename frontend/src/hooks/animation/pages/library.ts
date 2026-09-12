import { useCallback, useEffect, useRef, useState } from 'react'
import {
  getPageResizeManager,
  isPageVisible,
  registerPageCleanup,
} from '../core'
import { Feature, hasFeature } from '../pageFeatures'

const PAGE_ID = 'library'

/** startPage('library') 由 useRouteScheduler 统一调用。 */
export function useLibraryScheduler(): void {
  useEffect(() => {
    return () => cleanupLibrary()
  }, [])
}

function getResizeManager() {
  return getPageResizeManager(PAGE_ID)
}

export function useLibraryResize<T extends Element>(
  ref: React.RefObject<T | null>,
): { width: number; height: number } {
  const [size, setSize] = useState({ width: 0, height: 0 })

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Resize)) {
      return
    }

    const el = ref.current
    if (!el) return

    const manager = getResizeManager()
    const callback = (entry: ResizeObserverEntry) => {
      const { width, height } = entry.contentRect
      setSize((prev) => {
        if (
          Math.abs(prev.width - width) < 1 &&
          Math.abs(prev.height - height) < 1
        ) {
          return prev
        }
        return { width, height }
      })
    }

    manager.observe(el, callback)

    const rect = el.getBoundingClientRect()
    setSize({ width: rect.width, height: rect.height })

    return () => {
      manager.unobserve(el)
    }
  }, [ref])

  return size
}

let _libraryIntersectionObserver: IntersectionObserver | null = null
const _libraryIntersectionCallbacks = new Map<
  Element,
  (entry: IntersectionObserverEntry) => void
>()

function getLibraryIntersectionObserver(): IntersectionObserver {
  if (!_libraryIntersectionObserver) {
    _libraryIntersectionObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const cb = _libraryIntersectionCallbacks.get(entry.target)
          if (cb) cb(entry)
        }
      },
      {
        rootMargin: '200px', // 提前 200px 开始加载
        threshold: 0,
      },
    )
  }
  return _libraryIntersectionObserver
}

export function useLibraryInView<T extends Element>(): {
  ref: React.RefObject<T | null>
  isInView: boolean
} {
  const ref = useRef<T>(null)
  const [isInView, setIsInView] = useState(false)

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Intersection)) {
      setIsInView(true)
      return
    }

    const el = ref.current
    if (!el) return

    const observer = getLibraryIntersectionObserver()
    const callback = (entry: IntersectionObserverEntry) => {
      setIsInView(entry.isIntersecting)
    }

    _libraryIntersectionCallbacks.set(el, callback)
    observer.observe(el)

    return () => {
      _libraryIntersectionCallbacks.delete(el)
      observer.unobserve(el)
    }
  }, [])

  return { ref, isInView }
}

export function useLibraryLazyLoad<T extends Element>(): {
  ref: React.RefObject<T | null>
  shouldLoad: boolean
} {
  const ref = useRef<T>(null)
  const [shouldLoad, setShouldLoad] = useState(false)

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Intersection)) {
      setShouldLoad(true)
      return
    }

    const el = ref.current
    if (!el) return

    const observer = getLibraryIntersectionObserver()
    const callback = (entry: IntersectionObserverEntry) => {
      if (entry.isIntersecting) {
        setShouldLoad(true)

        _libraryIntersectionCallbacks.delete(el)
        observer.unobserve(el)
      }
    }

    _libraryIntersectionCallbacks.set(el, callback)
    observer.observe(el)

    return () => {
      _libraryIntersectionCallbacks.delete(el)
      observer.unobserve(el)
    }
  }, [])

  return { ref, shouldLoad }
}

export function useLibraryInfiniteScroll(
  onLoadMore: () => void | Promise<void>,
  hasMore: boolean,
  isLoading?: boolean,
): React.RefObject<HTMLDivElement | null> {
  const sentinelRef = useRef<HTMLDivElement>(null)
  const loadingRef = useRef(false)

  useEffect(() => {
    if (isLoading !== undefined) {
      loadingRef.current = isLoading
    }
  }, [isLoading])

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Intersection) || !hasMore) {
      return
    }

    const el = sentinelRef.current
    if (!el) return

    const observer = getLibraryIntersectionObserver()
    const callback = async (entry: IntersectionObserverEntry) => {
      if (entry.isIntersecting && !loadingRef.current && hasMore) {
        loadingRef.current = true
        try {
          await onLoadMore()
        } finally {
          loadingRef.current = false
        }
      }
    }

    _libraryIntersectionCallbacks.set(el, callback)
    observer.observe(el)

    return () => {
      _libraryIntersectionCallbacks.delete(el)
      observer.unobserve(el)
    }
  }, [onLoadMore, hasMore])

  return sentinelRef
}

export function useLibraryIntersectionObserver(): {
  observeLibraryIntersection: (
    el: Element,
    callback: (entry: IntersectionObserverEntry) => void,
  ) => void
  unobserveLibraryIntersection: (el: Element) => void
} {
  const observeLibraryIntersection = useCallback(
    (el: Element, callback: (entry: IntersectionObserverEntry) => void) => {
      if (!hasFeature(PAGE_ID, Feature.Intersection)) {
        callback({ isIntersecting: true } as IntersectionObserverEntry)
        return
      }
      const observer = getLibraryIntersectionObserver()
      _libraryIntersectionCallbacks.set(el, callback)
      observer.observe(el)
    },
    [],
  )

  const unobserveLibraryIntersection = useCallback((el: Element) => {
    _libraryIntersectionCallbacks.delete(el)
    if (_libraryIntersectionObserver) {
      _libraryIntersectionObserver.unobserve(el)
    }
  }, [])

  return { observeLibraryIntersection, unobserveLibraryIntersection }
}

export function useLibraryPrefetch(
  prefetchFn: () => void,
  deps: React.DependencyList = [],
): void {
  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Idle)) {
      return
    }

    const id = requestIdleCallback(
      () => {
        if (isPageVisible()) {
          prefetchFn()
        }
      },
      { timeout: 5000 },
    )

    return () => cancelIdleCallback(id)
  }, deps)
}

export function cleanupLibrary(): void {
  getPageResizeManager(PAGE_ID).cleanup()

  if (_libraryIntersectionObserver) {
    _libraryIntersectionObserver.disconnect()
    _libraryIntersectionObserver = null
  }
  _libraryIntersectionCallbacks.clear()
}

registerPageCleanup(PAGE_ID, cleanupLibrary)
