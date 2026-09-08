import type { RefObject } from 'react'
import type { StickerFloatMode } from '../widgets/StickerWidget'
import { useI18n } from '../../contexts/I18nContext'
import {
  WidgetSettingsAction,
  WidgetSettingsChoice,
  WidgetSettingsChoices,
  WidgetSettingsSection,
  WidgetSettingsTip,
} from '../widgets/shared/WidgetSettingsTip'

export interface HomeStickerCropTipProps {
  open: boolean
  anchor: DOMRect | null
  src?: string
  mode: StickerFloatMode
  onMode: (mode: StickerFloatMode) => void
  onClose: () => void
  ignoreRef?: RefObject<HTMLElement | null>
}

function stickerDownloadName(blob: Blob, src: string): string {
  const type = blob.type.toLowerCase()
  const fromUrl = src.match(/\.(png|jpe?g|webp|gif)(?:$|\?)/i)?.[1]
  const ext = type.includes('jpeg')
    ? 'jpg'
    : type.includes('webp')
      ? 'webp'
      : type.includes('gif')
        ? 'gif'
        : fromUrl
          ? fromUrl.toLowerCase().replace('jpeg', 'jpg')
          : 'png'
  return `sticker.${ext}`
}

async function downloadStickerImage(src: string): Promise<void> {
  const response = await fetch(src, { credentials: 'include' })
  if (!response.ok) throw new Error('download failed')
  const blob = await response.blob()
  const objectUrl = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = objectUrl
  link.download = stickerDownloadName(blob, src)
  document.body.appendChild(link)
  link.click()
  link.remove()
  window.setTimeout(() => URL.revokeObjectURL(objectUrl), 1_000)
}

export function HomeStickerCropTip({
  open,
  anchor,
  src,
  mode,
  onMode,
  onClose,
  ignoreRef,
}: HomeStickerCropTipProps) {
  const { t } = useI18n()

  const choices: { id: StickerFloatMode; label: string; hint: string }[] = [
    {
      id: 'loop',
      label: t.home.stickerFloatLoop,
      hint: t.home.stickerFloatLoopHint,
    },
    {
      id: 'hover',
      label: t.home.stickerFloatHover,
      hint: t.home.stickerFloatHoverHint,
    },
    {
      id: 'off',
      label: t.home.stickerFloatOff,
      hint: t.home.stickerFloatOffHint,
    },
  ]

  return (
    <WidgetSettingsTip
      open={open}
      anchor={anchor}
      title={t.home.stickerSettings}
      subtitle={t.home.stickerCropHint}
      width={300}
      height={392}
      onClose={onClose}
      ignoreRef={ignoreRef}
    >
      <WidgetSettingsSection label={t.home.stickerFloat}>
        <WidgetSettingsChoices label={t.home.stickerFloat}>
          {choices.map((choice) => (
            <WidgetSettingsChoice
              key={choice.id}
              selected={mode === choice.id}
              label={choice.label}
              hint={choice.hint}
              onClick={() => onMode(choice.id)}
            />
          ))}
        </WidgetSettingsChoices>
      </WidgetSettingsSection>
      {src ? (
        <WidgetSettingsSection label={t.home.stickerFile}>
          <WidgetSettingsAction
            label={t.home.stickerDownload}
            hint={t.home.stickerDownloadHint}
            onClick={() => {
              void downloadStickerImage(src).catch(() => {
                window.open(src, '_blank', 'noopener,noreferrer')
              })
            }}
          />
        </WidgetSettingsSection>
      ) : null}
    </WidgetSettingsTip>
  )
}
