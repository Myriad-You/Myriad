/**
 * 图片灯箱组件
 * 支持深浅双模式，优雅的动画效果
 */

import { LuDownload as Download, LuRotateCw as RotateCw, LuX as X, LuZoomIn as ZoomIn, LuZoomOut as ZoomOut } from '@lib/icons'
import { AnimatePresence, motion } from 'framer-motion'
import { useCallback, useEffect, useState } from 'react'

interface LightboxProps {
  src: string | null
  alt?: string
  isDark: boolean
  onClose: () => void
  t: Record<string, any>
}

export function Lightbox({ src, alt = '', isDark, onClose, t }: LightboxProps) {
  const [scale, setScale] = useState(1)
  const [rotation, setRotation] = useState(0)

  // ESC 键关闭
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose()
      }
    }

    if (src) {
      document.addEventListener('keydown', handleKeyDown)
      // 防止背景滚动
      document.body.style.overflow = 'hidden'
    }

    return () => {
      document.removeEventListener('keydown', handleKeyDown)
      document.body.style.overflow = ''
    }
  }, [src, onClose])

  // 缩放控制
  const handleZoomIn = useCallback(() => {
    setScale(prev => Math.min(prev + 0.25, 3))
  }, [])

  const handleZoomOut = useCallback(() => {
    setScale(prev => Math.max(prev - 0.25, 0.5))
  }, [])

  // 旋转控制
  const handleRotate = useCallback(() => {
    setRotation(prev => (prev + 90) % 360)
  }, [])

  // 下载图片
  const handleDownload = useCallback(() => {
    if (!src)
      return
    const link = document.createElement('a')
    link.href = src
    link.download = alt || 'image'
    link.click()
  }, [src, alt])

  // 重置状态
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
          className="fixed inset-0 z-[100] flex flex-col items-center justify-center"
          onClick={handleClose}
        >
          {/* 背景遮罩 */}
          <div
            className={`absolute inset-0 backdrop-blur-md ${
              isDark ? 'bg-black/90' : 'bg-white/90'
            }`}
          />

          {/* 桌面端控制栏 - 居中显示，包含所有功能 */}
          <motion.div
            initial={{ opacity: 0, y: -20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -20 }}
            transition={{ duration: 0.25, ease: [0.16, 1, 0.3, 1] }}
            className={`hidden sm:flex absolute top-6 items-center gap-2 px-4 py-2.5 rounded-2xl backdrop-blur-xl border shadow-xl z-10 ${
              isDark
                ? 'bg-neutral-900/80 border-neutral-700 text-white'
                : 'bg-white/80 border-gray-200 text-gray-900'
            }`}
            onClick={e => e.stopPropagation()}
          >
            {/* 缩小 */}
            <button
              onClick={handleZoomOut}
              disabled={scale <= 0.5}
              className={`p-2 rounded-lg transition-all duration-200 ease-out ${
                scale <= 0.5
                  ? 'opacity-30 cursor-not-allowed'
                  : isDark
                    ? 'hover:bg-white/10'
                    : 'hover:bg-black/5'
              }`}
              title={t.brew.lightboxZoomOut}
            >
              <ZoomOut className="w-5 h-5" />
            </button>

            {/* 缩放比例 */}
            <span className={`text-sm font-medium tabular-nums min-w-[3.5rem] text-center ${
              isDark ? 'text-gray-300' : 'text-gray-600'
            }`}
            >
              {Math.round(scale * 100)}
              %
            </span>

            {/* 放大 */}
            <button
              onClick={handleZoomIn}
              disabled={scale >= 3}
              className={`p-2 rounded-lg transition-all duration-200 ease-out ${
                scale >= 3
                  ? 'opacity-30 cursor-not-allowed'
                  : isDark
                    ? 'hover:bg-white/10'
                    : 'hover:bg-black/5'
              }`}
              title={t.brew.lightboxZoomIn}
            >
              <ZoomIn className="w-5 h-5" />
            </button>

            <div className={`w-px h-6 ${isDark ? 'bg-white/10' : 'bg-black/10'}`} />

            {/* 旋转 */}
            <button
              onClick={handleRotate}
              className={`p-2 rounded-lg transition-all duration-200 ease-out ${
                isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'
              }`}
              title={t.brew.lightboxRotate}
            >
              <RotateCw className="w-5 h-5" />
            </button>

            {/* 下载 */}
            <button
              onClick={handleDownload}
              className={`p-2 rounded-lg transition-all duration-200 ease-out ${
                isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'
              }`}
              title={t.brew.lightboxDownload}
            >
              <Download className="w-5 h-5" />
            </button>

            <div className={`w-px h-6 ${isDark ? 'bg-white/10' : 'bg-black/10'}`} />

            {/* 关闭 */}
            <button
              onClick={handleClose}
              className={`p-2 rounded-lg transition-all duration-200 ease-out ${
                isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'
              }`}
              title={`${t.brew.lightboxClose} (ESC)`}
            >
              <X className="w-5 h-5" />
            </button>
          </motion.div>

          {/* 移动端控制栏 - 左对齐，只有关闭和下载 */}
          <motion.div
            initial={{ opacity: 0, y: -20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -20 }}
            transition={{ duration: 0.25, ease: [0.16, 1, 0.3, 1] }}
            className={`sm:hidden absolute top-4 left-4 flex items-center gap-2 px-3 py-2 rounded-xl backdrop-blur-xl border shadow-xl z-10 ${
              isDark
                ? 'bg-neutral-900/80 border-neutral-700 text-white'
                : 'bg-white/80 border-gray-200 text-gray-900'
            }`}
            onClick={e => e.stopPropagation()}
          >
            {/* 关闭 */}
            <button
              type="button"
              onClick={handleClose}
              className={`p-1.5 rounded-lg transition-all duration-200 ease-out ${
                isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'
              }`}
              title={t.brew.lightboxClose}
            >
              <X className="w-5 h-5" />
            </button>

            <div className={`w-px h-5 ${isDark ? 'bg-white/10' : 'bg-black/10'}`} />

            {/* 下载 */}
            <button
              type="button"
              onClick={handleDownload}
              className={`p-1.5 rounded-lg transition-all duration-200 ease-out ${
                isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'
              }`}
              title={t.brew.lightboxDownload}
            >
              <Download className="w-5 h-5" />
            </button>
          </motion.div>

          {/* 图片容器 */}
          <motion.div
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
            className="relative flex items-center justify-center"
            onClick={e => e.stopPropagation()}
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

          {/* 提示文本 */}
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 20 }}
            transition={{ duration: 0.25, delay: 0.1, ease: [0.16, 1, 0.3, 1] }}
            className={`absolute bottom-6 px-4 py-2 rounded-xl backdrop-blur-xl text-sm z-10 ${
              isDark
                ? 'bg-neutral-900/60 text-gray-300'
                : 'bg-white/60 text-gray-600'
            }`}
          >
            {t.brew.lightboxCloseHint}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
