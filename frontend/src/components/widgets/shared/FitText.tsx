import type { CSSProperties, ElementType, ReactNode } from 'react'
import { useFitText } from '../../../hooks/useFitText'
import './FitText.css'

export interface FitTextProps {
  children: ReactNode
  max?: number
  min?: number
  maxLines?: number
  boxHeight?: number
  marquee?: boolean
  deps?: unknown[]
  enabled?: boolean
  lineHeight?: number
  as?: ElementType
  className?: string
  style?: CSSProperties
  title?: string
  align?: CSSProperties['textAlign']
}

export function FitText({
  children,
  max,
  min,
  maxLines = 1,
  boxHeight,
  marquee = true,
  deps,
  enabled,
  lineHeight,
  as: Tag = 'span',
  className,
  style,
  title,
  align,
}: FitTextProps) {
  const fit = useFitText({ max, min, maxLines, boxHeight, marquee, deps, enabled })

  // 省略号只在不能滚动且已到字号下限时出现。
  const layout: CSSProperties
    = fit.mode === 'wrap'
      ? {
          display: 'block',
          whiteSpace: 'normal',
          overflow: 'hidden',
          maxHeight: boxHeight ? `${boxHeight}px` : undefined,
          ...({ textWrap: 'balance' } as CSSProperties),
        }
      : {
          display: 'block',
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: fit.clamped ? 'ellipsis' : 'clip',
        }

  const fullText = typeof children === 'string' ? children : undefined
  const hoverTitle
    = fit.clamped || fit.mode === 'marquee' ? fullText ?? title : title

  return (
    <Tag
      ref={fit.ref}
      className={className}
      title={hoverTitle}
      style={{
        ...layout,
        minWidth: 0,
        maxWidth: '100%',
        fontSize: `${fit.fontSize}px`,
        lineHeight: lineHeight ?? fit.lineHeight,
        textAlign: align,
        ...style,
      }}
    >
      {fit.mode === 'marquee'
        ? (
            <span
              data-fittext-track
              style={
                {
                  '--ft-shift': `${-fit.marqueeDistance}px`,
                  '--ft-duration': `${fit.marqueeDuration}s`,
                } as CSSProperties
              }
            >
              {children}
            </span>
          )
        : children}
    </Tag>
  )
}

export interface ClampTextProps {
  children: ReactNode
  lines?: number
  as?: ElementType
  className?: string
  style?: CSSProperties
  title?: string
}

export function ClampText({
  children,
  lines = 2,
  as: Tag = 'span',
  className,
  style,
  title,
}: ClampTextProps) {
  return (
    <Tag
      className={className}
      title={title}
      style={{
        display: '-webkit-box',
        WebkitBoxOrient: 'vertical',
        WebkitLineClamp: lines,
        overflow: 'hidden',
        minWidth: 0,
        ...style,
      }}
    >
      {children}
    </Tag>
  )
}
