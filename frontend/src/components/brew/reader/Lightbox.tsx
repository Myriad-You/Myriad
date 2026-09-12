import type { MouseEvent } from 'react'
import type { ReaderCopy } from './types'
import {
  LuDownload as Download,
  LuRotateCw as RotateCw,
  LuX as X,
  LuZoomIn as ZoomIn,
  LuZoomOut as ZoomOut,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useCallback, useEffect, useState } from 'react'

interface LightboxProps {
  src: string | null
  alt?: string
  isDark: boolean
  onClose: () => void
  t: ReaderCopy
}

export function Lightbox({ src, alt = '', onClose, t }: LightboxProps) {
  const [scale, setScale] = useState(1)
  const [rotation, setRotation] = useState(0)

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose()
      }
    }

    if (src) {
      document.addEventListener('keydown', handleKeyDown)
      // 锁背景滚动；保存原 overflow 以便恢复。
      const originalOverflow = document.body.style.overflow
      document.body.style.overflow = 'hidden'

      return () => {
        document.removeEventListener('keydown', handleKeyDown)
        document.body.style.overflow = originalOverflow
      }
    }

    return () => {
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [src, onClose])

  const handleZoomIn = useCallback(() => {
    setScale((prev) => Math.min(prev + 0.25, 3))
  }, [])

  const handleZoomOut = useCallback(() => {
    setScale((prev) => Math.max(prev - 0.25, 0.5))
  }, [])

  const handleRotate = useCallback(() => {
    setRotation((prev) => (prev + 90) % 360)
  }, [])

  const handleDownload = useCallback(() => {
    if (!src) return
    const link = document.createElement('a')
    link.href = src
    link.download = alt || 'image'
    link.click()
  }, [src, alt])

  const handleClose = useCallback(() => {
    setScale(1)
    setRotation(0)
    onClose()
  }, [onClose])

  return (
    <AnimatePresence>
      {src && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2, ease: 'easeOut' }}
          className="fixed inset-0 z-100 flex flex-col items-center justify-center"
          onClick={handleClose}
        >
          <div className="brew-reader__lb-scrim" />

          <motion.div
            initial={{ opacity: 0, y: -20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -20 }}
            transition={{ duration: 0.25, ease: [0.16, 1, 0.3, 1] }}
            className="hidden sm:flex absolute top-6 items-center gap-2 px-4 py-2.5 brew-reader__chip z-10"
            onClick={(e: MouseEvent) => e.stopPropagation()}
          >
            <button
              onClick={handleZoomOut}
              disabled={scale <= 0.5}
              className={`brew-reader__btn${scale <= 0.5 ? ' opacity-30 cursor-not-allowed' : ''}`}
              title={t.brew.lightboxZoomOut}
            >
              <ZoomOut className="w-5 h-5" />
            </button>

            <span
              className="text-sm font-medium tabular-nums min-w-[3.5rem] text-center brew-reader__mute"
            >
              {Math.round(scale * 100)}%
            </span>

            <button
              onClick={handleZoomIn}
              disabled={scale >= 3}
              className={`brew-reader__btn${scale >= 3 ? ' opacity-30 cursor-not-allowed' : ''}`}
              title={t.brew.lightboxZoomIn}
            >
              <ZoomIn className="w-5 h-5" />
            </button>

            <div className="brew-reader__rule" />

            <button
              onClick={handleRotate}
              className="brew-reader__btn"
              title={t.brew.lightboxRotate}
            >
              <RotateCw className="w-5 h-5" />
            </button>

            <button
              onClick={handleDownload}
              className="brew-reader__btn"
              title={t.brew.lightboxDownload}
            >
              <Download className="w-5 h-5" />
            </button>

            <div className="brew-reader__rule" />

            <button
              onClick={handleClose}
              className="brew-reader__btn"
              title={`${t.brew.lightboxClose} (ESC)`}
            >
              <X className="w-5 h-5" />
            </button>
          </motion.div>

          <motion.div
            initial={{ opacity: 0, y: -20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -20 }}
            transition={{ duration: 0.25, ease: [0.16, 1, 0.3, 1] }}
            className="sm:hidden absolute top-4 left-4 flex items-center gap-2 px-3 py-2 brew-reader__chip z-10"
            onClick={(e: MouseEvent) => e.stopPropagation()}
          >
            <button
              type="button"
              onClick={handleClose}
              className="brew-reader__btn"
              title={t.brew.lightboxClose}
            >
              <X className="w-5 h-5" />
            </button>

            <div className="brew-reader__rule" />

            <button
              type="button"
              onClick={handleDownload}
              className="brew-reader__btn"
              title={t.brew.lightboxDownload}
            >
              <Download className="w-5 h-5" />
            </button>
          </motion.div>

          <motion.div
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
            className="relative flex items-center justify-center"
            onClick={(e: MouseEvent) => e.stopPropagation()}
          >
            <motion.img
              src={src}
              alt={alt}
              animate={{
                scale,
                rotate: rotation,
              }}
              transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
              className="max-w-[90vw] max-h-[calc(90vh-120px)] object-contain rounded-lg shadow-2xl"
              style={{
                transformOrigin: 'center center',
              }}
            />
          </motion.div>

          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 20 }}
            transition={{ duration: 0.25, delay: 0.1, ease: [0.16, 1, 0.3, 1] }}
            className="absolute bottom-6 px-4 py-2 brew-reader__chip brew-reader__mute text-sm z-10"
          >
            {t.brew.lightboxCloseHint}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
