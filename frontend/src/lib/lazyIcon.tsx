import React from 'react'

/**
 * 图标加载工具
 * 注意：react-icons 已被移除，改用内联 SVG 图标 (@lib/icons.tsx)
 */

export function IconFallback({ size = 16 }: { size?: number }) {
  return <div style={{ width: size, height: size }} className="inline-block align-middle animate-pulse bg-gray-200/40 dark:bg-white/5 rounded" />
}
