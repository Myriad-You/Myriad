import type { PointerEvent } from 'react'
import type { StickerCrop } from '../../utils/homeStickerCrop'
import { useLayoutEffect, useRef } from 'react'
import {
  clampStickerCrop,
  stickerCropObjectPosition,
} from '../../utils/homeStickerCrop'
import './HomeStickerCrop.css'

export interface HomeStickerCropProps {
  src: string
  aspect: number
  crop: StickerCrop
  hint?: string
  disabled?: boolean
  /** Fill the parent instead of using a max-height aspect box. */
  fill?: boolean
  onChange: (crop: StickerCrop) => void
}

export function HomeStickerCrop({
  src,
  aspect,
  crop,
  hint,
  disabled,
  fill = false,
  onChange,
}: HomeStickerCropProps) {
  const frameRef = useRef<HTMLDivElement>(null)
  const dragRef = useRef<{
    x: number
    y: number
    crop: StickerCrop
  } | null>(null)

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (disabled) return
    event.preventDefault()
    event.currentTarget.setPointerCapture(event.pointerId)
    dragRef.current = { x: event.clientX, y: event.clientY, crop }
  }

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current
    const frame = frameRef.current
    if (!drag || !frame) return
    const rect = frame.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0) return
    const dx = (event.clientX - drag.x) / rect.width
    const dy = (event.clientY - drag.y) / rect.height
    onChange(
      clampStickerCrop({
        x: drag.crop.x - dx / drag.crop.zoom,
        y: drag.crop.y - dy / drag.crop.zoom,
        zoom: drag.crop.zoom,
      }),
    )
  }

  const cropRef = useRef(crop)
  const onChangeRef = useRef(onChange)
  cropRef.current = crop
  onChangeRef.current = onChange

  const handlePointerUp = (event: PointerEvent<HTMLDivElement>) => {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
    dragRef.current = null
  }

  useLayoutEffect(() => {
    const frame = frameRef.current
    if (!frame || disabled) return
    const onWheel = (event: WheelEvent) => {
      event.preventDefault()
      const current = cropRef.current
      const next = event.deltaY > 0 ? current.zoom * 0.94 : current.zoom * 1.06
      onChangeRef.current(clampStickerCrop({ ...current, zoom: next }))
    }
    frame.addEventListener('wheel', onWheel, { passive: false })
    return () => frame.removeEventListener('wheel', onWheel)
  }, [disabled])

  return (
    <div className={`home-sticker-crop${fill ? ' is-fill' : ''}`}>
      <div
        ref={frameRef}
        className={`home-sticker-crop__frame${disabled ? '' : ' is-live'}`}
        style={fill ? undefined : { aspectRatio: `${aspect}` }}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
      >
        <img
          className="home-sticker-crop__image"
          src={src}
          alt=""
          draggable={false}
          style={{
            objectPosition: stickerCropObjectPosition(crop),
            transform: `scale(${crop.zoom})`,
            transformOrigin: stickerCropObjectPosition(crop),
          }}
        />
      </div>
      {hint ? <p className="home-sticker-crop__hint">{hint}</p> : null}
    </div>
  )
}
