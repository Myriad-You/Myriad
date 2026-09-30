import type {
  ClothingStyle,
  StructuredPersona,
  UpperBodyVisualIdentity,
} from '../../../components/agent/onboarding/onboardingTypes'
import type { AgentPersona } from '../../../services/agent/agentApi'
import type { WardrobeItem } from '../persona/wardrobe'
import type { SiteFace } from './useSiteFace'
import { useCallback, useEffect, useRef, useState } from 'react'
import { generationFailureMessage } from '../../../components/agent/onboarding/generationError'
import {
  CLOTHING_STYLE_OPTIONS,
  flattenPersona,
  genderFromProfile,
  parseUpperBodyVisualIdentity,
  visualIdentityFromProfile,
} from '../../../components/agent/onboarding/onboardingTypes'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { emitAppEvent } from '../../../utils/appEvents'
import { userFacingError } from '../../../utils/userFacingError'
import { generateSitePortrait } from '../api'
import { notifyFaceUpdated } from '../events/updates'
import {
  applyOutfit,
  bindPortrait,
  hydrateWardrobe,
  isDefaultWardrobeItem,
  parseWardrobe,
  parseWardrobeName,
  persistWardrobeState,
  seedWardrobeFromIdentity,
  stampPortrait,
  withCharacter,
  writeOutfit,
} from '../persona/wardrobe'
import { reportMeropeError, structuredFromSnapshot } from './workbenchShared'

interface Options {
  face: SiteFace
  /** A new master portrait replaced the old one. */
  onPortraitReplaced: () => void
}

/**
 * Her persona and wardrobe as the workbench edits them. Every write goes
 * through `saveVisualProfile`, which persists and then reloads the face.
 */
export function usePersonaWardrobe({ face, onPortraitReplaced }: Options) {
  const { t, locale } = useI18n()
  const o = t.agentPersona.onboarding
  const { clearPortrait, loadFace, portraitUrl } = face
  const [wardrobeItems, setWardrobeItems] = useState<WardrobeItem[]>([])
  const [activeOutfitId, setActiveOutfitId] = useState<string | null>(null)
  const [managingOutfitId, setManagingOutfitId] = useState<string | null>(null)
  const [generating, setGenerating] = useState(false)
  const [visualIdentity, setVisualIdentity] =
    useState<UpperBodyVisualIdentity | null>(null)
  const [personaSnapshot, setPersonaSnapshot] = useState<AgentPersona | null>(
    null,
  )
  const [structuredPersona, setStructuredPersona] =
    useState<StructuredPersona | null>(null)
  const fillAttempted = useRef(false)

  /** Take the persona as stored: identity and wardrobe follow it. */
  const hydrate = useCallback((persona: AgentPersona | null) => {
    setPersonaSnapshot(persona)
    const identity = visualIdentityFromProfile(persona?.visualProfile)
    setVisualIdentity(identity)
    const hydrated = hydrateWardrobe(
      persona?.visualProfile,
      identity,
      persona?.portraitAssetId,
    )
    setWardrobeItems(hydrated.items)
    setActiveOutfitId(hydrated.activeId)
  }, [])

  /** The persona as first loaded. */
  const adopt = useCallback(
    (persona: AgentPersona | null) => {
      hydrate(persona)
      setStructuredPersona(structuredFromSnapshot(persona))
    },
    [hydrate],
  )

  /** The persona could not be read: show nothing rather than stale state. */
  const forget = useCallback(() => {
    setPersonaSnapshot(null)
    setStructuredPersona(null)
    setVisualIdentity(null)
    setWardrobeItems([])
    setActiveOutfitId(null)
  }, [])

  /** Re-read the persona after the server changed it. */
  const reload = useCallback(async () => {
    hydrate(await agentService.getPersona())
  }, [hydrate])

  const saveVisualProfile = useCallback(
    async (patch: {
      identity: UpperBodyVisualIdentity
      clothingStyle?: string | null
      items: WardrobeItem[]
      activeId: string | null
      portraitAssetId?: string | null
    }) => {
      if (!personaSnapshot) return
      const persisted = persistWardrobeState(patch.items, patch.activeId)
      const portraitSpecified = Object.hasOwn(patch, 'portraitAssetId')
      const requestedPortrait = patch.portraitAssetId?.trim() || ''
      const items = requestedPortrait
        ? bindPortrait(persisted.items, persisted.activeId, requestedPortrait)
        : persisted.items
      const saved = await agentService.putPersona({
        name: personaSnapshot.name,
        personality: personaSnapshot.personality ?? '',
        persona: personaSnapshot.persona,
        ...(portraitSpecified
          ? { portraitAssetId: requestedPortrait || null }
          : {}),
        visualProfile: {
          ...(personaSnapshot.visualProfile ?? {}),
          visualIdentity: patch.identity,
          ...(patch.clothingStyle !== undefined
            ? { clothingStyle: patch.clothingStyle }
            : {}),
          wardrobe: items,
          activeOutfitId: persisted.activeId,
        },
      })
      setPersonaSnapshot(saved)
      setVisualIdentity(patch.identity)
      setWardrobeItems(items)
      setActiveOutfitId(persisted.activeId)
      if (portraitSpecified && !requestedPortrait) clearPortrait()
      await loadFace()
    },
    [clearPortrait, loadFace, personaSnapshot],
  )

  const seededDefault = useRef(false)
  useEffect(() => {
    if (seededDefault.current || !personaSnapshot || !visualIdentity) return
    if (!wardrobeItems.some(isDefaultWardrobeItem)) return
    const stored = parseWardrobe(
      personaSnapshot.visualProfile &&
        typeof personaSnapshot.visualProfile === 'object'
        ? (personaSnapshot.visualProfile as { wardrobe?: unknown }).wardrobe
        : undefined,
    )
    if (stored.some(isDefaultWardrobeItem)) {
      seededDefault.current = true
      return
    }
    seededDefault.current = true
    void saveVisualProfile({
      identity: visualIdentity,
      items: wardrobeItems,
      activeId: activeOutfitId,
    }).catch(() => {
      seededDefault.current = false
    })
  }, [
    activeOutfitId,
    personaSnapshot,
    saveVisualProfile,
    visualIdentity,
    wardrobeItems,
  ])

  const applyVisualFromPortrait = useCallback(
    async (nextPortraitUrl?: string | null) => {
      if (!personaSnapshot) return
      setGenerating(true)
      reportMeropeError('')
      try {
        const observed = await agentService.observeVisualFromPortrait({
          gender:
            genderFromProfile(personaSnapshot.visualProfile) ?? 'unspecified',
          language:
            typeof personaSnapshot.visualProfile?.language === 'string'
              ? personaSnapshot.visualProfile.language
              : locale,
        })
        const observedIdentity = parseUpperBodyVisualIdentity(
          observed.visualIdentity,
        )
        if (!observedIdentity) throw new Error(t.merope.wardrobeFillFailed)
        const identity = visualIdentity
          ? {
              character: visualIdentity.character,
              outfit: observedIdentity.outfit,
            }
          : observedIdentity
        const style =
          typeof observed.clothingStyle === 'string' &&
          (CLOTHING_STYLE_OPTIONS as string[]).includes(observed.clothingStyle)
            ? (observed.clothingStyle as ClothingStyle)
            : 'everyday'
        const portrait = nextPortraitUrl?.trim() || portraitUrl
        if (wardrobeItems.length > 0 && activeOutfitId) {
          await saveVisualProfile({
            identity,
            clothingStyle: style,
            items: wardrobeItems.map((item) =>
              item.id === activeOutfitId
                ? {
                    id: item.id,
                    clothingStyle: style,
                    outfit: identity.outfit,
                    ...(portrait
                      ? { portraitAssetId: portrait }
                      : item.portraitAssetId
                        ? { portraitAssetId: item.portraitAssetId }
                        : {}),
                    ...(item.name ? { name: item.name } : {}),
                    ...(portrait &&
                    portrait === item.portraitAssetId &&
                    item.rigAssetId
                      ? { rigAssetId: item.rigAssetId }
                      : {}),
                  }
                : item,
            ),
            activeId: activeOutfitId,
            portraitAssetId: portrait,
          })
        } else {
          const seeded = seedWardrobeFromIdentity(identity, style, portrait)
          await saveVisualProfile({
            identity,
            clothingStyle: style,
            items: seeded.items,
            activeId: seeded.activeId,
            portraitAssetId: portrait,
          })
        }
      } catch (reason) {
        reportMeropeError(
          generationFailureMessage(
            reason,
            t.merope.wardrobeFillFailed,
            o.generationTimeout,
            {
              portrait_required: o.importPortraitHint,
              visual_design_unusable: o.importVisualFailed,
            },
          ),
        )
        throw reason
      } finally {
        setGenerating(false)
      }
    },
    [
      activeOutfitId,
      locale,
      o.generationTimeout,
      o.importPortraitHint,
      o.importVisualFailed,
      personaSnapshot,
      portraitUrl,
      saveVisualProfile,
      t.merope.wardrobeFillFailed,
      visualIdentity,
      wardrobeItems,
    ],
  )

  const fillVisualFromPortrait = useCallback(async () => {
    if (visualIdentity) return
    try {
      await applyVisualFromPortrait()
    } catch {
    }
  }, [applyVisualFromPortrait, visualIdentity])

  useEffect(() => {
    if (fillAttempted.current) return
    if (!personaSnapshot || visualIdentity || !portraitUrl) return
    fillAttempted.current = true
    void fillVisualFromPortrait()
  }, [fillVisualFromPortrait, personaSnapshot, portraitUrl, visualIdentity])

  const saveCharacter = useCallback(
    async (next: UpperBodyVisualIdentity) => {
      if (!visualIdentity) return
      const identity = withCharacter(visualIdentity, next.character)
      setVisualIdentity(identity)
      try {
        await saveVisualProfile({
          identity,
          items: wardrobeItems,
          activeId: activeOutfitId,
        })
      } catch (reason) {
        reportMeropeError(
          generationFailureMessage(
            reason,
            o.visualDesignSaveFailed,
            o.generationTimeout,
          ),
        )
      }
    },
    [
      activeOutfitId,
      o.generationTimeout,
      o.visualDesignSaveFailed,
      saveVisualProfile,
      visualIdentity,
      wardrobeItems,
    ],
  )

  const saveOutfitDesign = useCallback(
    async (itemId: string, next: UpperBodyVisualIdentity) => {
      if (!visualIdentity) return
      const items = writeOutfit(wardrobeItems, itemId, next.outfit)
      const identity =
        itemId === activeOutfitId
          ? { character: visualIdentity.character, outfit: next.outfit }
          : visualIdentity
      if (itemId === activeOutfitId) setVisualIdentity(identity)
      try {
        await saveVisualProfile({
          identity,
          items,
          activeId: activeOutfitId,
        })
      } catch (reason) {
        reportMeropeError(
          generationFailureMessage(
            reason,
            o.visualDesignSaveFailed,
            o.generationTimeout,
          ),
        )
      }
    },
    [
      activeOutfitId,
      o.generationTimeout,
      o.visualDesignSaveFailed,
      saveVisualProfile,
      visualIdentity,
      wardrobeItems,
    ],
  )

  const saveStructuredPersona = useCallback(
    async (next: StructuredPersona, options?: { resetVisual?: boolean }) => {
      setStructuredPersona(next)
      const name = personaSnapshot?.name.trim() || 'Arael'
      const visualProfile = options?.resetVisual
        ? {
            ...(personaSnapshot?.visualProfile ?? {}),
            visualIdentity: null,
            clothingStyle: null,
            wardrobe: [],
            activeOutfitId: null,
          }
        : personaSnapshot?.visualProfile
      try {
        const saved = await agentService.putPersona({
          name,
          personality: flattenPersona(next),
          persona: {
            ...(personaSnapshot?.persona ?? {}),
            displayName: name,
            ...next,
          },
          visualProfile,
        })
        if (options?.resetVisual) {
          fillAttempted.current = false
          setVisualIdentity(null)
          setWardrobeItems([])
          setActiveOutfitId(null)
          setManagingOutfitId(null)
        }
        setPersonaSnapshot(saved)
        emitAppEvent('arael-persona-updated')
      } catch (reason) {
        reportMeropeError(userFacingError(reason, o.saveFailed))
      }
    },
    [o.saveFailed, personaSnapshot],
  )

  const generatePortrait = useCallback(async (item: WardrobeItem) => {
    if (generating || !visualIdentity) return
    if (!window.confirm(t.merope.visualConfirm)) return
    const nextIdentity = applyOutfit(visualIdentity, item)
    setGenerating(true)
    reportMeropeError('')
    try {
      await saveVisualProfile({
        identity: nextIdentity,
        clothingStyle: item.clothingStyle,
        items: wardrobeItems,
        activeId: item.id,
      })
      const generated = await generateSitePortrait()
      await loadFace()
      if (generated.portraitUrl) {
        await saveVisualProfile({
          identity: nextIdentity,
          clothingStyle: item.clothingStyle,
          items: stampPortrait(
            wardrobeItems,
            item.id,
            generated.portraitUrl,
            generated.generationFingerprint,
          ),
          activeId: item.id,
          portraitAssetId: generated.portraitUrl,
        })
        // 同一次写入已作废旧贴纸。
        onPortraitReplaced()
      }
      notifyFaceUpdated()
    } catch (reason) {
      const o = t.agentPersona.onboarding
      reportMeropeError(
        generationFailureMessage(
          reason,
          t.merope.visualFailed,
          o.generationTimeout,
          {
            portrait_generation_failed: o.portraitGenerateFailed,
          },
        ),
      )
    } finally {
      setGenerating(false)
    }
  }, [
    generating,
    loadFace,
    onPortraitReplaced,
    saveVisualProfile,
    t.merope.visualConfirm,
    t.merope.visualFailed,
    t.agentPersona.onboarding,
    visualIdentity,
    wardrobeItems,
  ])

  const wearOutfit = useCallback(
    async (item: WardrobeItem) => {
      if (!visualIdentity) return
      const nextIdentity = applyOutfit(visualIdentity, item)
      if (item.portraitAssetId) {
        await saveVisualProfile({
          identity: nextIdentity,
          clothingStyle: item.clothingStyle,
          items: wardrobeItems,
          activeId: item.id,
          portraitAssetId: item.portraitAssetId,
        })
        notifyFaceUpdated()
        return
      }
      setGenerating(true)
      try {
        await saveVisualProfile({
          identity: nextIdentity,
          clothingStyle: item.clothingStyle,
          items: wardrobeItems,
          activeId: item.id,
        })
        const generated = await generateSitePortrait()
        if (!generated.portraitUrl) throw new Error(t.merope.visualFailed)
        await saveVisualProfile({
          identity: nextIdentity,
          clothingStyle: item.clothingStyle,
          items: stampPortrait(
            wardrobeItems,
            item.id,
            generated.portraitUrl,
            generated.generationFingerprint,
          ),
          activeId: item.id,
          portraitAssetId: generated.portraitUrl,
        })
        notifyFaceUpdated()
      } finally {
        setGenerating(false)
      }
    },
    [
      saveVisualProfile,
      t.merope.visualFailed,
      visualIdentity,
      wardrobeItems,
    ],
  )

  const renameOutfit = useCallback(
    async (id: string, rawName: string) => {
      if (!visualIdentity || isDefaultWardrobeItem(id)) return
      const name = parseWardrobeName(rawName)
      await saveVisualProfile({
        identity: visualIdentity,
        items: wardrobeItems.map((item) =>
          item.id === id
            ? name
              ? { ...item, name }
              : { ...item, name: undefined }
            : item,
        ),
        activeId: activeOutfitId,
      })
    },
    [activeOutfitId, saveVisualProfile, visualIdentity, wardrobeItems],
  )

  const deleteOutfit = async (id: string) => {
    if (isDefaultWardrobeItem(id)) return
    const remaining = wardrobeItems.filter((item) => item.id !== id)
    if (remaining.length === 0 || !visualIdentity) return
    const nextItem = id === activeOutfitId ? remaining[0] : null
    await saveVisualProfile({
      identity: nextItem
        ? applyOutfit(visualIdentity, nextItem)
        : visualIdentity,
      clothingStyle: nextItem?.clothingStyle,
      items: remaining,
      activeId: nextItem?.id ?? activeOutfitId,
      ...(nextItem
        ? { portraitAssetId: nextItem.portraitAssetId ?? null }
        : {}),
    })
    if (managingOutfitId === id) {
      setManagingOutfitId(nextItem?.id ?? null)
    }
    notifyFaceUpdated()
  }

  const createOutfit = async (item: WardrobeItem) => {
    if (!visualIdentity) return
    await saveVisualProfile({
      identity: visualIdentity,
      items: [...wardrobeItems, item],
      activeId: activeOutfitId,
    })
    setManagingOutfitId(item.id)
  }

  return {
    personaSnapshot,
    setPersonaSnapshot,
    structuredPersona,
    visualIdentity,
    wardrobeItems,
    setWardrobeItems,
    activeOutfitId,
    managingOutfitId,
    setManagingOutfitId,
    generating,
    adopt,
    forget,
    reload,
    applyVisualFromPortrait,
    fillVisualFromPortrait,
    saveCharacter,
    saveOutfitDesign,
    saveStructuredPersona,
    generatePortrait,
    wearOutfit,
    renameOutfit,
    deleteOutfit,
    createOutfit,
  }
}
