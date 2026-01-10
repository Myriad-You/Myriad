/**
 * Tapp 图标渲染组件
 * 统一处理 emoji、URL、内联 SVG 三种图标类型
 *
 * iOS/Safari 兼容性：使用 img+data URI 方式渲染 SVG
 */

import React, { useMemo } from 'react'

/** 图标属性 */
export interface TappIconProps {
  /** emoji 或 URL 图标 */
  icon?: string
  /** 内联 SVG 代码（优先于 icon） */
  iconSvg?: string
  /** 应用名称（用于 fallback 显示首字母） */
  name: string
  /** 图标尺寸 class，如 "w-4 h-4" */
  sizeClass?: string
  /** 字体尺寸 class，如 "text-2xl"（用于 emoji） */
  textSizeClass?: string
  /** 额外的 className */
  className?: string
  /** SVG 颜色（替换 currentColor，默认 white） */
  svgColor?: string
}

/**
 * 检查字符串是否为 URL（用于区分 emoji 和图片 URL）
 */
export function isIconUrl(icon: string | undefined): boolean {
  if (!icon)
    return false
  return icon.startsWith('http://') || icon.startsWith('https://') || icon.startsWith('data:') || icon.startsWith('/')
}

/**
 * 检查字符串是否为内联 SVG
 */
export function isIconSvg(icon: string | undefined): boolean {
  if (!icon)
    return false
  return icon.trim().startsWith('<svg')
}

/**
 * 将 SVG 转换为 data URI（iOS/Safari 兼容方式）
 * 这种方式比 dangerouslySetInnerHTML 更可靠
 */
function svgToDataUri(svg: string, color: string = 'white'): string {
  let normalized = svg.trim()

  // 添加 xmlns（如果缺失）- 必须用于 data URI
  if (!normalized.includes('xmlns=')) {
    normalized = normalized.replace('<svg', '<svg xmlns="http://www.w3.org/2000/svg"')
  }

  // 替换 currentColor 为指定颜色（data URI 中无法继承 CSS 颜色）
  normalized = normalized.replace(/currentColor/g, color)

  // 编码为 data URI
  const encoded = encodeURIComponent(normalized)
    .replace(/'/g, '%27')
    .replace(/"/g, '%22')

  return `data:image/svg+xml,${encoded}`
}

/**
 * 渲染 Tapp 图标
 * 优先级：iconSvg > icon (URL) > icon (emoji) > 名称首字母
 */
export function TappIcon({
  icon,
  iconSvg,
  name,
  sizeClass = 'w-6 h-6',
  textSizeClass = 'text-xl',
  className = '',
  svgColor = 'white',
}: TappIconProps): React.ReactElement {
  // 将 SVG 转换为 data URI（使用 useMemo 避免重复计算）
  const svgDataUri = useMemo(() => {
    if (iconSvg && isIconSvg(iconSvg)) {
      return svgToDataUri(iconSvg, svgColor)
    }
    if (icon && isIconSvg(icon)) {
      return svgToDataUri(icon, svgColor)
    }
    return null
  }, [iconSvg, icon, svgColor])

  // 1. 优先使用内联 SVG（通过 img + data URI 渲染）
  if (svgDataUri) {
    return (
      <img
        src={svgDataUri}
        alt=""
        className={`${sizeClass} ${className}`}
        style={{
          display: 'block',
          objectFit: 'contain',
        }}
      />
    )
  }

  // 2. 检查 icon 是否为 URL
  if (icon && isIconUrl(icon)) {
    return (
      <img
        src={icon}
        alt=""
        className={`object-contain ${sizeClass} ${className}`}
      />
    )
  }

  // 3. 使用 emoji
  if (icon) {
    return (
      <span className={`${textSizeClass} ${className}`}>
        {icon}
      </span>
    )
  }

  // 4. Fallback：显示名称首字母
  return (
    <span className={`font-bold ${textSizeClass} ${className}`}>
      {name.charAt(0).toUpperCase()}
    </span>
  )
}

export default TappIcon
