/**
 * Catalog merchandising locale overlays (long_description / preview).
 * Distinct from manifest.locales, which only cover install-time name/description.
 */

import type { StorePreviewDescriptor } from './storePreview'
import {
  nonEmptyText,
  pickLocaleEntry,
  resolveManifestText,
} from './manifestLocale'
import { parseStorePreview } from './storePreview'

export interface RemoteStoreLocaleEntry {
  name?: string
  description?: string
  long_description?: string
  preview?: StorePreviewDescriptor
}

export type RemoteStoreLocales = Record<string, RemoteStoreLocaleEntry>

export interface LocalizedStoreApp {
  name: string
  description?: string
  longDescription?: string
  preview?: StorePreviewDescriptor
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** Ingest untrusted catalog `locales`; invalid previews are dropped per entry. */
export function parseStoreLocales(value: unknown): RemoteStoreLocales | undefined {
  if (!isRecord(value)) return undefined
  const locales: RemoteStoreLocales = {}
  for (const [tag, raw] of Object.entries(value)) {
    if (!tag.trim() || !isRecord(raw)) continue
    locales[tag] = {
      name: typeof raw.name === 'string' ? raw.name : undefined,
      description: typeof raw.description === 'string' ? raw.description : undefined,
      long_description:
        typeof raw.long_description === 'string' ? raw.long_description : undefined,
      preview: parseStorePreview(raw.preview),
    }
  }
  return Object.keys(locales).length > 0 ? locales : undefined
}

/**
 * Resolve store-facing name, short/long copy, and preview for the host locale.
 * Per-field fallback: locale entry → catalog default → localized short description.
 */
export function resolveStoreMerchandising(
  app: {
    name: string
    description?: string
    long_description?: string
    preview?: StorePreviewDescriptor
    locales?: RemoteStoreLocales
  },
  locale: string | undefined,
): LocalizedStoreApp {
  const text = resolveManifestText(app, locale)
  const entry = pickLocaleEntry(app.locales, locale)
  return {
    name: text.name,
    description: text.description,
    longDescription:
      nonEmptyText(entry?.long_description) ??
      nonEmptyText(app.long_description) ??
      text.description,
    preview: entry?.preview ?? app.preview,
  }
}
