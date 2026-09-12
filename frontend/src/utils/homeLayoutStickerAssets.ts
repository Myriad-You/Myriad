/** Same-site image-cache PNG/JPEG/WebP only. */

import type { HomeDashboardLayouts } from './homeLayout'
import type { HomeLayoutAsset, HomeLayoutAssetMap } from './homeLayoutTransfer'
import { API_URL } from '../config'
import {
  canonicalStickerImageUrl,
  encodeBase64,

  isHomeLayoutStickerTile,
  rewriteStickerImageUrls,
  sniffStickerImage,
  stickerAssetDataUrl,
  stickerAssetFitsBudget,
} from './homeLayoutTransfer'

export type StickerAssetFetch = (
  input: string,
  init?: RequestInit,
) => Promise<{
  ok: boolean
  arrayBuffer: () => Promise<ArrayBuffer>
}>

export async function fetchStickerAssets(
  layouts: HomeDashboardLayouts,
  fetchImpl: StickerAssetFetch = fetch,
): Promise<{ assets: HomeLayoutAssetMap; missing: string[] }> {
  const assets: HomeLayoutAssetMap = {}
  const missing: string[] = []
  const seen = new Set<string>()
  let occupied = 0
  for (const tile of layouts.free) {
    if (!isHomeLayoutStickerTile(tile)) continue
    const raw =
      typeof tile.config?.imageUrl === 'string' ? tile.config.imageUrl.trim() : ''
    if (!raw || raw.startsWith('data:')) continue
    const path = canonicalStickerImageUrl(raw)
    if (!path || path.startsWith('inline:')) {
      missing.push(raw)
      continue
    }
    if (seen.has(path)) continue
    seen.add(path)
    try {
      const res = await fetchImpl(`${API_URL}${path}`, {
        credentials: 'include',
      })
      if (!res.ok) {
        missing.push(raw)
        continue
      }
      const bytes = new Uint8Array(await res.arrayBuffer())
      if (!stickerAssetFitsBudget(occupied, bytes.byteLength)) {
        missing.push(raw)
        continue
      }
      const mime = sniffStickerImage(bytes)
      if (!mime) {
        missing.push(raw)
        continue
      }
      occupied += bytes.byteLength
      assets[path] = { mime, data: encodeBase64(bytes) }
    } catch {
      missing.push(raw)
    }
  }
  return { assets, missing }
}

export async function restoreStickerAssets(
  layouts: HomeDashboardLayouts,
  assets: HomeLayoutAssetMap,
  upload: (dataUrl: string) => Promise<string>,
): Promise<{ layouts: HomeDashboardLayouts; restored: number; failed: string[] }> {
  const replacements = new Map<string, string>()
  const failed: string[] = []
  const needed = new Set<string>()
  for (const tile of layouts.free) {
    if (!isHomeLayoutStickerTile(tile)) continue
    const raw =
      typeof tile.config?.imageUrl === 'string' ? tile.config.imageUrl.trim() : ''
    if (!raw) continue
    const key = canonicalStickerImageUrl(raw) ?? raw
    if (assetForUrl(key, assets) || assetForUrl(raw, assets)) needed.add(key)
  }

  for (const key of needed) {
    const asset = assetForUrl(key, assets)
    if (!asset) {
      failed.push(key)
      continue
    }
    try {
      const stored = (await upload(stickerAssetDataUrl(asset))).trim()
      if (!stored) {
        failed.push(key)
        continue
      }
      replacements.set(key, stored)
    } catch {
      failed.push(key)
    }
  }

  const next = rewriteStickerImageUrls(layouts, (url) => {
    const key = canonicalStickerImageUrl(url) ?? url
    const stored = replacements.get(key)
    if (stored) return stored
    if (url.startsWith('inline:') || url.startsWith('data:')) return null
    return url
  })
  return { layouts: next, restored: replacements.size, failed }
}

function assetForUrl(
  url: string,
  assets: HomeLayoutAssetMap,
): HomeLayoutAsset | undefined {
  if (assets[url]) return assets[url]
  const key = canonicalStickerImageUrl(url)
  if (key && assets[key]) return assets[key]
  return undefined
}
