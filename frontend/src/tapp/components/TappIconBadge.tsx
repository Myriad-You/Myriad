import type { CSSProperties, ReactNode } from 'react'
import type { IconStyle, TappIconStyleSource } from '../utils/tappColors'
import { getTappIconStyle } from '../utils/tappColors'
import { TappIcon } from './TappIcon'

export interface TappIconBadgeProps extends TappIconStyleSource {
  name: string
  shellClassName: string
  glyphSizeClass: string
  glyphTextClass?: string
  iconStyle?: IconStyle
  className?: string
  style?: CSSProperties
  children?: ReactNode
}

export function TappIconBadge({
  icon,
  iconSvg,
  name,
  themeColor,
  category,
  id,
  permissions,
  iconShell,
  shellClassName,
  glyphSizeClass,
  glyphTextClass = 'text-xl',
  iconStyle: iconStyleProp,
  className = '',
  style,
  children,
}: TappIconBadgeProps) {
  const iconStyle =
    iconStyleProp ??
    getTappIconStyle({
      icon,
      iconSvg,
      iconShell,
      themeColor,
      category,
      id,
      permissions,
    })

  const shellClasses = [
    'relative overflow-hidden shrink-0',
    shellClassName,
    iconStyle.standalone
      ? 'tapp-icon-badge--standalone bg-transparent'
      :
        `${iconStyle.className} tapp-icon-shell--material flex items-center justify-center text-white`,
    className,
  ]
    .filter(Boolean)
    .join(' ')

  // standalone 不画 accent/theme 壳。
  const shellStyle: CSSProperties | undefined = iconStyle.standalone
    ? style
      ? (() => {
          const { background: _bg, backgroundImage: _bi, ...rest } = style
          return Object.keys(rest).length > 0 ? rest : undefined
        })()
      : undefined
    : { ...iconStyle.style, ...style }

  const mediaClass = iconStyle.standalone
    ? 'tapp-icon-badge__media relative z-10'
    : iconStyle.insetMedia
      ?
        'tapp-icon-badge__inset-media relative z-10'
      : 'tapp-icon-badge__glyph relative z-10'

  return (
    <div className={shellClasses} style={shellStyle}>
      <TappIcon
        icon={icon}
        iconSvg={iconSvg}
        name={name}
        sizeClass={iconStyle.standalone ? 'w-full h-full' : glyphSizeClass}
        textSizeClass={glyphTextClass}
        svgColor={iconStyle.insetMedia ? null : '#ffffff'}
        className={mediaClass}
      />
      {children}
    </div>
  )
}

export default TappIconBadge
