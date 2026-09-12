import React, { useMemo } from 'react'
import { resolveTappIconAsset } from '../constants/icons'

export interface TappIconProps {
  icon?: string
  iconSvg?: string
  name: string
  sizeClass?: string
  textSizeClass?: string
  className?: string
  svgColor?: string | null
}

export function isIconUrl(icon: string | undefined): boolean {
  if (!icon) return false
  return (
    icon.startsWith('http://') ||
    icon.startsWith('https://') ||
    icon.startsWith('data:') ||
    icon.startsWith('/')
  )
}

export function isIconSvg(icon: string | undefined): boolean {
  if (!icon) return false
  return icon.trim().startsWith('<svg')
}

export function hasStandaloneTappIcon(source: {
  icon?: string
  iconSvg?: string
  iconShell?: boolean
}): boolean {
  if (source.iconShell === true) return false

  if (
    source.icon &&
    (isIconUrl(source.icon) || Boolean(resolveTappIconAsset(source.icon)))
  ) {
    return true
  }
  const svg =
    (source.iconSvg && isIconSvg(source.iconSvg) && source.iconSvg.trim()) ||
    (source.icon && isIconSvg(source.icon) && source.icon.trim()) ||
    ''
  if (!svg) return false
  if (/currentColor/i.test(svg)) return false
  return true
}

export function isTappIconFullColorMedia(source: {
  icon?: string
  iconSvg?: string
}): boolean {
  if (
    source.icon &&
    (isIconUrl(source.icon) || Boolean(resolveTappIconAsset(source.icon)))
  ) {
    return true
  }
  const svg =
    (source.iconSvg && isIconSvg(source.iconSvg) && source.iconSvg.trim()) ||
    (source.icon && isIconSvg(source.icon) && source.icon.trim()) ||
    ''
  if (!svg) return false
  return !/currentColor/i.test(svg)
}

function svgToDataUri(svg: string, color?: string | null): string {
  let normalized = svg.trim()

  // data URI 必须带 xmlns。
  if (!normalized.includes('xmlns=')) {
    normalized = normalized.replace(
      '<svg',
      '<svg xmlns="http://www.w3.org/2000/svg"',
    )
  }

  if (color) {
    normalized = normalized.replaceAll('currentColor', color)
  }

  const encoded = encodeURIComponent(normalized)
    .replaceAll("'", '%27')
    .replaceAll('"', '%22')

  return `data:image/svg+xml,${encoded}`
}

/** iconSvg > URL/token > emoji > 名称首字母 */
export function TappIcon({
  icon,
  iconSvg,
  name,
  sizeClass = 'w-6 h-6',
  textSizeClass = 'text-xl',
  className = '',
  svgColor = 'white',
}: TappIconProps): React.ReactElement {
  const assetIcon = resolveTappIconAsset(icon)

  const svgDataUri = useMemo(() => {
    // null → leave currentColor; undefined falls back to white via default param
    const color = svgColor === null ? null : (svgColor ?? 'white')
    if (iconSvg && isIconSvg(iconSvg)) {
      return svgToDataUri(iconSvg, color)
    }
    if (icon && isIconSvg(icon)) {
      return svgToDataUri(icon, color)
    }
    return null
  }, [iconSvg, icon, svgColor])

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

  if (assetIcon || (icon && isIconUrl(icon))) {
    return (
      <img
        src={assetIcon ?? icon}
        alt=""
        draggable={false}
        decoding="async"
        className={`object-contain ${sizeClass} ${className}`}
      />
    )
  }

  if (icon) {
    return <span className={`${textSizeClass} ${className}`}>{icon}</span>
  }

  return (
    <span className={`font-bold ${textSizeClass} ${className}`}>
      {name.charAt(0).toUpperCase()}
    </span>
  )
}

export default TappIcon
