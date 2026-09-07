import type {
  ClothingStyle,
  OutfitVisual,
  UpperBodyVisualIdentity,
} from '../../components/agent/onboarding/onboardingTypes'
import {
  CLOTHING_STYLE_OPTIONS,
  clothingStyleFromProfile,
  parseUpperBodyVisualIdentity,
} from '../../components/agent/onboarding/onboardingTypes'

export const MAX_WARDROBE_ITEMS = 8
export const MAX_WARDROBE_NAME_CHARS = 40
export const DEFAULT_WARDROBE_ID = 'default'

export interface WardrobeItem {
  id: string
  clothingStyle: ClothingStyle
  outfit: OutfitVisual
  portraitAssetId?: string
  rigAssetId?: string
  generationFingerprint?: string
  name?: string
}

const LIVE_ID = 'live'

export function isDefaultWardrobeItem(
  item: Pick<WardrobeItem, 'id'> | string,
): boolean {
  return (typeof item === 'string' ? item : item.id) === DEFAULT_WARDROBE_ID
}

export function newWardrobeId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return `w-${crypto.randomUUID()}`
  }
  return `w-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`
}

function isClothingStyle(value: unknown): value is ClothingStyle {
  return (
    typeof value === 'string' &&
    (CLOTHING_STYLE_OPTIONS as string[]).includes(value)
  )
}

function parseOutfit(value: unknown): OutfitVisual | null {
  const parsed = parseUpperBodyVisualIdentity({
    character: {
      faceDesign: 'x',
      eyeDesign: 'x',
      hairShape: 'x',
      hairLayerPlan: 'x',
    },
    outfit: value,
  })
  return parsed?.outfit ?? null
}

export function parseWardrobeItem(value: unknown): WardrobeItem | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const source = value as Record<string, unknown>
  const id = typeof source.id === 'string' ? source.id.trim() : ''
  if (!id || id.length > 64) return null
  if (!isClothingStyle(source.clothingStyle)) return null
  const outfit = parseOutfit(source.outfit)
  if (!outfit) return null
  const portraitAssetId = parsePortraitAssetId(source.portraitAssetId)
  const rigAssetId = parseHexId(source.rigAssetId)
  const generationFingerprint = parseHexId(source.generationFingerprint)
  const name =
    id === DEFAULT_WARDROBE_ID ? undefined : parseWardrobeName(source.name)
  return {
    id,
    clothingStyle: source.clothingStyle,
    outfit,
    ...(portraitAssetId ? { portraitAssetId } : {}),
    ...(rigAssetId ? { rigAssetId } : {}),
    ...(generationFingerprint ? { generationFingerprint } : {}),
    ...(name ? { name } : {}),
  }
}

export function parseWardrobeName(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined
  const cleaned = [...value]
    .filter((char) => {
      const code = char.charCodeAt(0)
      return code >= 32 && code !== 127
    })
    .join('')
    .trim()
  if (!cleaned) return undefined
  return [...cleaned].slice(0, MAX_WARDROBE_NAME_CHARS).join('').trim() || undefined
}

export function wardrobeItemLabel(
  item: Pick<WardrobeItem, 'name' | 'clothingStyle'> &
    Partial<Pick<WardrobeItem, 'id'>>,
  styleNames: Record<ClothingStyle, string>,
  defaultLabel?: string,
): string {
  if (item.id === DEFAULT_WARDROBE_ID) {
    return defaultLabel?.trim() || styleNames[item.clothingStyle]
  }
  return item.name?.trim() || styleNames[item.clothingStyle]
}

function parseHexId(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined
  const id = value.trim().toLowerCase()
  if (id.length !== 64 || /[^0-9a-f]/.test(id)) return undefined
  return id
}

function parsePortraitAssetId(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined
  const portrait = value.trim()
  if (
    !portrait ||
    portrait.length > 512 ||
    portrait.includes(':') ||
    portrait.includes('..') ||
    portrait.startsWith('//') ||
    /\s/.test(portrait)
  ) {
    return undefined
  }
  return portrait
}

export function bindPortrait(
  items: WardrobeItem[],
  activeId: string | null,
  portraitAssetId: string | null | undefined,
): WardrobeItem[] {
  const portrait = parsePortraitAssetId(portraitAssetId)
  if (!portrait || !activeId) return items
  const active = items.find((item) => item.id === activeId)
  if (!active || active.portraitAssetId) return items
  if (items.some((item) => item.id !== activeId && item.portraitAssetId === portrait)) {
    return items
  }
  if (items.length > 1) return items
  return items.map((item) =>
    item.id === activeId ? { ...item, portraitAssetId: portrait } : item,
  )
}

export function stampPortrait(
  items: WardrobeItem[],
  activeId: string | null,
  portraitAssetId: string | null | undefined,
  generationFingerprint?: string | null,
): WardrobeItem[] {
  const portrait = parsePortraitAssetId(portraitAssetId)
  if (!portrait || !activeId) return items
  const fingerprint = parseHexId(generationFingerprint)
  return items.map((item) => {
    if (item.id !== activeId) return item
    if (item.portraitAssetId === portrait) {
      if (!fingerprint || item.generationFingerprint === fingerprint) return item
      return { ...item, generationFingerprint: fingerprint }
    }
    return {
      id: item.id,
      clothingStyle: item.clothingStyle,
      outfit: item.outfit,
      portraitAssetId: portrait,
      ...(fingerprint ? { generationFingerprint: fingerprint } : {}),
      ...(item.name ? { name: item.name } : {}),
    }
  })
}

export function sortWardrobe(items: WardrobeItem[]): WardrobeItem[] {
  return [...items].sort((left, right) => {
    if (left.id === DEFAULT_WARDROBE_ID) return -1
    if (right.id === DEFAULT_WARDROBE_ID) return 1
    const byStyle =
      CLOTHING_STYLE_OPTIONS.indexOf(left.clothingStyle) -
      CLOTHING_STYLE_OPTIONS.indexOf(right.clothingStyle)
    if (byStyle !== 0) return byStyle
    return left.id.localeCompare(right.id)
  })
}

export function parseWardrobe(value: unknown): WardrobeItem[] {
  if (!Array.isArray(value)) return []
  const items: WardrobeItem[] = []
  const seen = new Set<string>()
  for (const raw of value) {
    const item = parseWardrobeItem(raw)
    if (!item || seen.has(item.id)) continue
    seen.add(item.id)
    items.push(item)
    if (items.length >= MAX_WARDROBE_ITEMS) break
  }
  return items
}

export function applyOutfit(
  identity: UpperBodyVisualIdentity,
  item: WardrobeItem,
): UpperBodyVisualIdentity {
  return {
    character: identity.character,
    outfit: item.outfit,
  }
}

export function withCharacter(
  identity: UpperBodyVisualIdentity,
  character: UpperBodyVisualIdentity['character'],
): UpperBodyVisualIdentity {
  return {
    character,
    outfit: identity.outfit,
  }
}

export function writeOutfit(
  items: WardrobeItem[],
  id: string,
  outfit: OutfitVisual,
): WardrobeItem[] {
  return items.map((item) => (item.id === id ? { ...item, outfit } : item))
}

export function withPersistentIds(items: WardrobeItem[]): WardrobeItem[] {
  return items.map((item) =>
    item.id === LIVE_ID ? { ...item, id: newWardrobeId() } : item,
  )
}

export function persistWardrobeState(
  items: WardrobeItem[],
  activeId: string | null,
): { items: WardrobeItem[]; activeId: string | null } {
  const next = withPersistentIds(items)
  if (activeId !== LIVE_ID) return { items: next, activeId }
  const index = items.findIndex((item) => item.id === LIVE_ID)
  return {
    items: next,
    activeId: index >= 0 ? next[index].id : (next[0]?.id ?? null),
  }
}

function defaultOutfit(
  identity: UpperBodyVisualIdentity,
  clothingStyle: ClothingStyle,
  portraitAssetId?: string | null,
): WardrobeItem {
  const portrait = parsePortraitAssetId(portraitAssetId)
  return {
    id: DEFAULT_WARDROBE_ID,
    clothingStyle,
    outfit: identity.outfit,
    ...(portrait ? { portraitAssetId: portrait } : {}),
  }
}

export function seedWardrobeFromIdentity(
  identity: UpperBodyVisualIdentity,
  clothingStyle: ClothingStyle,
  portraitAssetId?: string | null,
): { items: WardrobeItem[]; activeId: string | null } {
  return {
    items: [defaultOutfit(identity, clothingStyle, portraitAssetId)],
    activeId: DEFAULT_WARDROBE_ID,
  }
}

export function ensureDefaultWardrobe(
  items: WardrobeItem[],
  identity: UpperBodyVisualIdentity | null,
  clothingStyle: ClothingStyle | null,
  portraitAssetId?: string | null,
  activeId?: string | null,
): { items: WardrobeItem[]; activeId: string | null } {
  if (!identity || !clothingStyle) {
    return { items, activeId: activeId ?? null }
  }
  const portrait = parsePortraitAssetId(portraitAssetId)
  const bindPortraitOnDefault = items.length <= 1
  const locked = items.find((item) => item.id === DEFAULT_WARDROBE_ID)
  if (locked) {
    return {
      items: items.map((item) => {
        if (item.id !== DEFAULT_WARDROBE_ID) return item
        return {
          id: item.id,
          clothingStyle: item.clothingStyle,
          outfit: item.outfit,
          ...(item.portraitAssetId
            ? { portraitAssetId: item.portraitAssetId }
            : bindPortraitOnDefault && portrait
              ? { portraitAssetId: portrait }
              : {}),
          ...(item.rigAssetId ? { rigAssetId: item.rigAssetId } : {}),
          ...(item.generationFingerprint
            ? { generationFingerprint: item.generationFingerprint }
            : {}),
        }
      }),
      activeId: activeId ?? DEFAULT_WARDROBE_ID,
    }
  }
  if (items.length === 0) {
    return {
      items: [defaultOutfit(identity, clothingStyle, portrait)],
      activeId: DEFAULT_WARDROBE_ID,
    }
  }
  const candidate = items[0]
  return {
    items: items.map((item) => {
      if (item.id !== candidate.id) return item
      return {
        id: DEFAULT_WARDROBE_ID,
        clothingStyle: item.clothingStyle,
        outfit: item.outfit,
        ...(item.portraitAssetId ? { portraitAssetId: item.portraitAssetId } : {}),
        ...(item.rigAssetId ? { rigAssetId: item.rigAssetId } : {}),
        ...(item.generationFingerprint
          ? { generationFingerprint: item.generationFingerprint }
          : {}),
      }
    }),
    activeId:
      !activeId || activeId === candidate.id
        ? DEFAULT_WARDROBE_ID
        : activeId,
  }
}

export function syncActiveOutfit(
  items: WardrobeItem[],
  activeId: string | null,
  identity: UpperBodyVisualIdentity,
): WardrobeItem[] {
  if (!activeId) return items
  return items.map((item) =>
    item.id === activeId ? { ...item, outfit: identity.outfit } : item,
  )
}

function outfitsMatch(left: OutfitVisual, right: OutfitVisual): boolean {
  return (Object.keys(left) as Array<keyof OutfitVisual>).every(
    (key) => left[key] === right[key],
  )
}

export function hydrateWardrobe(
  profile: unknown,
  identity: UpperBodyVisualIdentity | null,
  portraitAssetId?: string | null,
): { items: WardrobeItem[]; activeId: string | null } {
  const source =
    profile && typeof profile === 'object' && !Array.isArray(profile)
      ? (profile as Record<string, unknown>)
      : null
  const parsed = parseWardrobe(source?.wardrobe)
  const style = clothingStyleFromProfile(profile)
  const saved =
    typeof source?.activeOutfitId === 'string' ? source.activeOutfitId : null
  const matched =
    parsed.find((item) => item.id === saved) ??
    (identity && style
      ? parsed.find(
          (item) =>
            item.clothingStyle === style &&
            outfitsMatch(item.outfit, identity.outfit),
        )
      : undefined) ??
    parsed[0]
  return ensureDefaultWardrobe(
    parsed,
    identity,
    style,
    portraitAssetId,
    matched?.id ?? saved,
  )
}
