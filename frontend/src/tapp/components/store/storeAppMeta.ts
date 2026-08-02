/** Language tags, icon style, permission level helpers for store UI. */

import type { ReactElement } from 'react'
import type { TappPermission } from '../../types'
import type { IconStyle } from '../../utils/tappColors'
import type { UnifiedAppItem } from './types'
import { createElement } from 'react'
import { PERMISSION_LEVELS } from '../../runtime/permissionConfig'
import { getTappIconStyle } from '../../utils/tappColors'
import { hasStandaloneTappIcon } from '../TappIcon'

/** Official store source / verified catalog — certification mark after title. */
export function isOfficialStoreApp(app: UnifiedAppItem): boolean {
  if (
    app.fromOfficialSource ||
    app.remoteApp?.sourceOfficial ||
    app.verified
  ) {
    return true
  }
  // Built-in examples (e.g. Hello World) tagged official
  if (app.tags?.some((tag) => tag.toLowerCase() === 'official')) {
    return true
  }
  if (
    app.localTapp?.tags?.some((tag) => tag.toLowerCase() === 'official')
  ) {
    return true
  }
  return false
}

/**
 * Soft scalloped certification seal: rounder rim (low-amplitude 12-lobe wave).
 * Polar: r = 9.35 + 0.55·cos(12θ)
 */
function OfficialVerifiedSealIcon() {
  return createElement(
    'svg',
    {
      className: 'as-store-verified-mark__icon',
      viewBox: '0 0 24 24',
      width: '1em',
      height: '1em',
      fill: 'currentColor',
      'aria-hidden': true,
      focusable: false,
    },
    createElement('path', {
      d: 'M12 2.1 12.43 2.18 12.84 2.41 13.22 2.73 13.58 3.06 13.92 3.34 14.28 3.5 14.67 3.54 15.1 3.47 15.58 3.36 16.07 3.28 16.54 3.28 16.95 3.43 17.28 3.71 17.52 4.12 17.69 4.58 17.83 5.05 17.99 5.46 18.22 5.78 18.54 6.01 18.95 6.17 19.42 6.31 19.88 6.48 20.29 6.72 20.57 7.05 20.72 7.46 20.72 7.93 20.64 8.42 20.53 8.9 20.46 9.33 20.5 9.72 20.66 10.08 20.94 10.42 21.27 10.78 21.59 11.16 21.82 11.57 21.9 12 21.82 12.43 21.59 12.84 21.27 13.22 20.94 13.58 20.66 13.92 20.5 14.28 20.46 14.67 20.53 15.1 20.64 15.58 20.72 16.07 20.72 16.54 20.57 16.95 20.29 17.28 19.88 17.52 19.42 17.69 18.95 17.83 18.54 17.99 18.22 18.22 17.99 18.54 17.83 18.95 17.69 19.42 17.52 19.88 17.28 20.29 16.95 20.57 16.54 20.72 16.07 20.72 15.58 20.64 15.1 20.53 14.67 20.46 14.28 20.5 13.92 20.66 13.58 20.94 13.22 21.27 12.84 21.59 12.43 21.82 12 21.9 11.57 21.82 11.16 21.59 10.78 21.27 10.42 20.94 10.08 20.66 9.72 20.5 9.33 20.46 8.9 20.53 8.42 20.64 7.93 20.72 7.46 20.72 7.05 20.57 6.72 20.29 6.48 19.88 6.31 19.42 6.17 18.95 6.01 18.54 5.78 18.22 5.46 17.99 5.05 17.83 4.58 17.69 4.12 17.52 3.71 17.28 3.43 16.95 3.28 16.54 3.28 16.07 3.36 15.58 3.47 15.1 3.54 14.67 3.5 14.28 3.34 13.92 3.06 13.58 2.73 13.22 2.41 12.84 2.18 12.43 2.1 12 2.18 11.57 2.41 11.16 2.73 10.78 3.06 10.42 3.34 10.08 3.5 9.72 3.54 9.33 3.47 8.9 3.36 8.42 3.28 7.93 3.28 7.46 3.43 7.05 3.71 6.72 4.12 6.48 4.58 6.31 5.05 6.17 5.46 6.01 5.78 5.78 6.01 5.46 6.17 5.05 6.31 4.58 6.48 4.12 6.72 3.71 7.05 3.43 7.46 3.28 7.93 3.28 8.42 3.36 8.9 3.47 9.33 3.54 9.72 3.5 10.08 3.34 10.42 3.06 10.78 2.73 11.16 2.41 11.57 2.18Z',
    }),
    createElement('path', {
      fill: 'var(--as-store-verified-check, #fff)',
      d: 'M10.28 14.72 7.78 12.22a.85.85 0 0 1 1.2-1.2l1.85 1.85 4.2-4.78a.85.85 0 1 1 1.28 1.12l-4.78 5.44a.85.85 0 0 1-1.25.07z',
    }),
  )
}

/** Official / verified certification seal with wavy rim. */
export function OfficialVerifiedDot({
  label,
}: {
  label: string
}): ReactElement {
  return createElement(
    'span',
    {
      className: 'as-store-verified-mark',
      title: label,
      'aria-label': label,
      role: 'img',
    },
    createElement(OfficialVerifiedSealIcon),
  )
}

/**
 * Infer catalog primary language from top-level name/description.
 * Myriad packages keep Chinese (etc.) as top-level fallback and only put
 * host overrides (en-US / ja-JP) under `locales` — so zh never appears as a key.
 */
export function inferPrimaryCatalogLocale(text: string): string | null {
  const sample = text.trim()
  if (!sample) return null
  if (/[\u3040-\u30FF]/.test(sample)) return 'ja-JP'
  if (/[\u3400-\u9FFF\uF900-\uFAFF]/.test(sample)) return 'zh-CN'
  return null
}

/** Collect declared BCP-47 tags from manifest locales + package i18n. */
export function collectAppLanguageTags(app: UnifiedAppItem): string[] {
  const tags = new Set<string>()
  const addKeys = (record?: Record<string, unknown> | null) => {
    if (!record) return
    for (const key of Object.keys(record)) {
      const tag = key.trim()
      if (tag) tags.add(tag)
    }
  }
  addKeys(app.remoteApp?.locales)
  addKeys(app.remoteApp?.download?.i18n)
  addKeys(app.localTapp?.manifest?.locales)
  addKeys(app.localTapp?.code?.i18n as Record<string, unknown> | undefined)

  const primaryText = [
    app.remoteApp?.name,
    app.remoteApp?.description,
    app.localTapp?.manifest?.name,
    app.localTapp?.manifest?.description,
  ]
    .filter((v): v is string => typeof v === 'string' && v.trim().length > 0)
    .join('\n')
  const inferred = inferPrimaryCatalogLocale(primaryText)
  if (inferred) {
    const primary = inferred.split(/[-_]/)[0]!.toLowerCase()
    const already = Array.from(tags).some(
      (tag) => tag.split(/[-_]/)[0]!.toLowerCase() === primary,
    )
    if (!already) tags.add(inferred)
  }

  return Array.from(tags).sort((a, b) =>
    a.localeCompare(b, undefined, { sensitivity: 'base' }),
  )
}

/** Format language tags for the current UI locale (e.g. zh-CN → 中文). */
export function formatAppLanguages(tags: string[], uiLocale: string): string {
  if (tags.length === 0) return '—'
  let displayNames: Intl.DisplayNames | null = null
  try {
    displayNames = new Intl.DisplayNames([uiLocale], { type: 'language' })
  } catch {
    displayNames = null
  }
  const labels: string[] = []
  const seen = new Set<string>()
  for (const tag of tags) {
    const primary = tag.split(/[-_]/)[0] || tag
    let label = primary
    if (displayNames) {
      try {
        label = displayNames.of(primary) || displayNames.of(tag) || primary
      } catch {
        try {
          label = displayNames.of(tag) || primary
        } catch {
          label = primary
        }
      }
    }
    if (seen.has(label)) continue
    seen.add(label)
    labels.push(label)
  }
  return labels.join(' · ')
}

/** @see hasStandaloneTappIcon — store alias */
export function hasStandaloneAppIcon(
  app: Pick<UnifiedAppItem, 'icon' | 'iconSvg'>,
): boolean {
  return hasStandaloneTappIcon(app)
}

/**
 * Accent surface for icons / feature cards (theme or category gradient).
 * `standalone` means the app ships a full icon — callers should not paint this
 * accent as a shell behind the icon (feature cards still use the accent fill).
 */
export function getAppIconStyle(app: UnifiedAppItem): IconStyle {
  return getTappIconStyle({
    icon: app.icon,
    iconSvg: app.iconSvg,
    themeColor: app.themeColor,
    category: app.category,
    id: app.id,
    permissions: app.permissions,
  })
}

/** 权限级别（与后端一致，未知权限按基础处理） */
export function getPermissionLevel(
  permission: string,
): 'basic' | 'elevated' | 'privileged' {
  return PERMISSION_LEVELS[permission as TappPermission] ?? 'basic'
}
