/**
 * 磁贴封面。
 *
 * 唯一的硬规则：**没有图就返回 `null`**。调用方看到 null 必须切纯文本布局，
 * 不允许画灰色占位块 —— 占位块只是把「没有内容」画成「有一块脏东西」。
 *
 * 文字永不压在封面上：封面独占一块，文字另占一块。
 */

import { memo, useEffect, useState } from 'react'

import { WIDGET_RADIUS_NESTED } from '../../widgets/shared/WidgetShell'
import { getImageUrl } from '../constants'

export interface TileCoverProps {
  /** 原始 image 字段；内部走 getImageUrl，不要自己拼代理 URL */
  image: string | null | undefined
  /** 通栏封面高（px，已缩放）；不传则吃满父容器高度 */
  height?: number
  /** 方图边长（px，已缩放）。给了就忽略 height，画正方形 */
  square?: number
  className?: string
  /** 圆角类，默认 nested（8px） */
  radiusClassName?: string
}

/**
 * 有图渲染 `object-fit: cover` 的一块；无图 / 加载失败 / 软失败占位图 → `null`。
 */
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
            // 图片代理软失败会返回 1×1 透明 PNG（HTTP 200），当作没有图
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

/**
 * 主题卡的 2×2 拼贴。
 *
 * 不足 4 张封面就少画几格，**不补灰块**；一张都没有返回 null。
 */
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
