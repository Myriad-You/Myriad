import type { CSSProperties } from 'react'
import type { WidgetComponentProps } from '../widgetGridTypes'
import { memo } from 'react'
import {
  parseStickerCrop,
  stickerCropObjectPosition,
} from '../../utils/homeStickerCrop'
import './StickerWidget.css'

function stickerImageUrl(config: WidgetComponentProps['config']): string {
  const url = config.config?.imageUrl
  return typeof url === 'string' ? url.trim() : ''
}

export function stickerFloatLoop(config?: Record<string, unknown> | null): boolean {
  if (!config || typeof config !== 'object') return true
  if (typeof config.floatLoop === 'boolean') return config.floatLoop
  if (config.float === false) return false
  return true
}

export function stickerFloatHover(config?: Record<string, unknown> | null): boolean {
  if (!config || typeof config !== 'object') return true
  return config.floatHover !== false
}

export type StickerFloatMode = 'loop' | 'hover' | 'off'

export function stickerFloatMode(
  config?: Record<string, unknown> | null,
): StickerFloatMode {
  if (stickerFloatLoop(config)) return 'loop'
  if (stickerFloatHover(config)) return 'hover'
  return 'off'
}

export function stickerFloatPatch(mode: StickerFloatMode): {
  floatLoop: boolean
  floatHover: boolean
} {
  return {
    floatLoop: mode === 'loop',
    floatHover: mode === 'hover',
  }
}

function stickerAlt(config: WidgetComponentProps['config']): string {
  const prompt = config.config?.prompt
  return typeof prompt === 'string' ? prompt.trim() : ''
}

function hashUnit(id: string): () => number {
  let h = 2166136261
  for (let i = 0; i < id.length; i += 1) {
    h = Math.imul(h ^ id.charCodeAt(i), 16777619)
  }
  return () => {
    h = Math.imul(h ^ (h >>> 16), 2246822507)
    h = Math.imul(h ^ (h >>> 13), 3266489917)
    return (h >>> 0) / 4294967296
  }
}

function stickerFloatVars(id: string): CSSProperties {
  const next = hashUnit(id)
  const span = (min: number, max: number) => min + next() * (max - min)
  const x1 = span(-4.8, 4.8)
  let x2 = span(-4.8, 4.8)
  if (x1 * x2 > 0) x2 = -x2
  const r1 = span(-1.35, 1.35)
  let r2 = span(-1.35, 1.35)
  if (r1 * r2 > 0) r2 = -r2
  return {
    '--sticker-bob-dur': `${span(3.6, 7.4).toFixed(2)}s`,
    '--sticker-bob-delay': `${span(-6.8, 0).toFixed(2)}s`,
    '--sticker-sway-dur': `${span(5.1, 10.4).toFixed(2)}s`,
    '--sticker-sway-delay': `${span(-8.6, 0).toFixed(2)}s`,
    '--sticker-float-ox': `${span(40, 60).toFixed(1)}%`,
    '--sticker-float-oy': `${span(68, 94).toFixed(1)}%`,
    '--sticker-float-y': `${span(-6.2, -2.2).toFixed(1)}px`,
    '--sticker-float-x1': `${x1.toFixed(1)}px`,
    '--sticker-float-x2': `${x2.toFixed(1)}px`,
    '--sticker-float-r1': `${r1.toFixed(2)}deg`,
    '--sticker-float-r2': `${r2.toFixed(2)}deg`,
  } as CSSProperties
}

const StickerWidget = memo(({
  config,
  isEditMode,
}: WidgetComponentProps) => {
  const src = stickerImageUrl(config)
  const alt = stickerAlt(config)
  const crop = parseStickerCrop(config.config?.crop)
  const loopOn = stickerFloatLoop(config.config)
  const hoverOn = stickerFloatHover(config.config)
  if (!src) {
    return <div className="sticker-widget" aria-hidden />
  }
  const style: CSSProperties | undefined = crop
    ? {
        objectFit: 'cover',
        objectPosition: stickerCropObjectPosition(crop),
        transform: `scale(${crop.zoom})`,
        transformOrigin: stickerCropObjectPosition(crop),
      }
    : undefined
  return (
    <div
      className={[
        'sticker-widget',
        loopOn ? 'is-float-loop' : '',
        hoverOn ? 'is-float-hover' : '',
        isEditMode ? 'is-edit' : '',
      ]
        .filter(Boolean)
        .join(' ')}
      style={stickerFloatVars(config.id)}
    >
      <div className="sticker-widget__bob">
        <div className="sticker-widget__sway">
          <div className={`sticker-widget__frame${crop ? ' has-crop' : ''}`}>
            <img
              className="sticker-widget__image"
              src={src}
              alt={alt}
              draggable={false}
              style={style}
            />
          </div>
        </div>
      </div>
    </div>
  )
})

export default StickerWidget
