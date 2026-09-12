/** 先退出全屏再关，以免 fullscreen 关掉 presence 后瞬间跳走。 */

import { useCallback, useEffect, useRef } from 'react'

export function useTappShellClose(options: {
  isFullscreen: boolean
  setIsFullscreen: (value: boolean | ((prev: boolean) => boolean)) => void
  requestClose: (action: () => void) => void
  onClosed: () => void
}): () => void {
  const { isFullscreen, setIsFullscreen, requestClose, onClosed } = options
  const pendingAfterFsRef = useRef(false)

  useEffect(() => {
    if (isFullscreen || !pendingAfterFsRef.current) return
    pendingAfterFsRef.current = false
    requestClose(onClosed)
  }, [isFullscreen, requestClose, onClosed])

  return useCallback(() => {
    if (isFullscreen) {
      pendingAfterFsRef.current = true
      setIsFullscreen(false)
      return
    }
    requestClose(onClosed)
  }, [isFullscreen, setIsFullscreen, requestClose, onClosed])
}
