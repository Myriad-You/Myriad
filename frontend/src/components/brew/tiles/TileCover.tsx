/** 没有图返回 null，调用方必须切纯文本，不画灰占位。文字不压封面。 */

import { memo, useEffect, useState } from 'react'

import { WIDGET_RADIUS_NESTED } from '../../widgets/shared/WidgetShell'
import { getImageUrl } from '../constants'

export interface TileCoverProps {
  /** 内部走 getImageUrl，不要自己拼代理 URL。 */
  image: string | null | undefined
  height?: number
  square?: number
  className?: string
  radiusClassName?: string
}

export const TileCover = memo(
  ({
    image,
    height,
    square,
    className,
    radiusClassName = WIDGET_RADIUS_NESTED,
  }: TileCoverProps) => {
    const [broken, setBroken] = useState(false)
    const src = getImageUrl(image ?? null)

    useEffect(() => {
      setBroken(false)
    }, [src])

    if (!src || broken) return null

    const style = square
      ? { width: square, height: square }
      : height
        ? { height }
        : undefined

    return (
      <div
        className={`overflow-hidden ${radiusClassName} ${square ? 'shrink-0' : ''} ${className ?? ''}`}
        style={style}
      >
        <img
          src={src}
          alt=""
          loading="lazy"
          decoding="async"
          draggable={false}
          className="h-full w-full object-cover"
          onLoad={(e) => {
            // 软失败 1×1 PNG 当作没有图。
            const img = e.currentTarget
            if (img.naturalWidth <= 1 && img.naturalHeight <= 1) setBroken(true)
          }}
          onError={() => setBroken(true)}
        />
      </div>
    )
  },
)

TileCover.displayName = 'TileCover'

/** 不足 4 张少画，不补灰块；没有则 null。 */
export const TileCoverMosaic = memo(
  ({
    images,
    height,
    gap,
    radiusClassName = WIDGET_RADIUS_NESTED,
  }: {
    images: (string | null | undefined)[]
    height: number
    gap: number
    radiusClassName?: string
  }) => {
    const usable = images
      .map((i) => getImageUrl(i ?? null))
      .filter((u): u is string => Boolean(u))
      .slice(0, 4)

    if (usable.length === 0) return null

    return (
      <div
        className={`grid overflow-hidden ${radiusClassName}`}
        style={{
          height,
          gap,
          gridTemplateColumns: usable.length === 1 ? '1fr' : '1fr 1fr',
          gridTemplateRows: usable.length <= 2 ? '1fr' : '1fr 1fr',
        }}
      >
        {usable.map((src, i) => (
          <img
            key={`${src}-${i}`}
            src={src}
            alt=""
            loading="lazy"
            decoding="async"
            draggable={false}
            className="h-full w-full object-cover"
            onError={(e) => {
              e.currentTarget.style.display = 'none'
            }}
          />
        ))}
      </div>
    )
  },
)

TileCoverMosaic.displayName = 'TileCoverMosaic'
