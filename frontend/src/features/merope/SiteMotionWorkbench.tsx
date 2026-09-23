import type {
  ClothingStyle,
  StructuredPersona,
  UpperBodyVisualIdentity,
  UpperBodyVisualIdentityKey,
} from '../../components/agent/onboarding/onboardingTypes'
import type { AgentPersona } from '../../services/agent/agentApi'
import type { PoseCorrection } from './anime25drig/poseCorrections'
import type { RigCharacterHandle } from './rig/RigCharacter'
import type { MeropeRigManifest } from './rig/types'
import type { MeropeActivity } from './types'
import type { WardrobeItem } from './wardrobe'
import { LuRefreshCw, LuSparkles } from '@lib/icons'
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { createPortal } from 'react-dom'
import { LuChevronLeft } from 'react-icons/lu'
import {
  activityKey,
  ADDRESSEE_UPDATED_EVENT,
  moodBand,
} from '../../components/agent/meropeVitals'
import { generationFailureMessage } from '../../components/agent/onboarding/generationError'
import {
  CLOTHING_STYLE_OPTIONS,
  flattenPersona,
  genderFromProfile,
  parseFlattenedPersona,
  parseUpperBodyVisualIdentity,
  personaFromApi,
  visualIdentityFromProfile,
} from '../../components/agent/onboarding/onboardingTypes'
import { Field, TextInput } from '../../components/agent/onboarding/ui/Field'
import PersonaIdentityView from '../../components/agent/onboarding/ui/PersonaIdentityView'
import PersonaImportPanel from '../../components/agent/onboarding/ui/PersonaImportPanel'
import PortraitImportButton from '../../components/agent/onboarding/ui/PortraitImportButton'
import VisualIdentityView from '../../components/agent/onboarding/ui/VisualIdentityView'
import { SettingsButton, ToggleSwitch } from '../../components/settings'
import {
  getTourSnapshot,
  subscribeTour,
} from '../../components/tour/tourEngine'
import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { notifyAvatarChanged } from '../../services/avatarSourceApi'
import { emitAppEvent } from '../../utils/appEvents'
import { invalidatePublicConfigCache } from '../../utils/requestDedup'
import { siteMediaUrl } from '../../utils/siteMediaUrl'
import { showStickyToast } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import Anime25DWorkbench from './anime25drig/Anime25DWorkbench'
import { isAnime25DPlayback } from './anime25drig/types'
import {
  decomposeSitePortraitWithSeeThrough,
  generateSitePortrait,
  generateStickerAvatar,
  getSeeThroughStatus,
  getSiteFace,
  saveRigPoseCorrections,
  updateSeeThroughToken,
} from './api'
import { commitRigPsdAsset, preflightRigPsdAsset } from './assets/pipeline'
import { notifyFaceUpdated } from './events'
import { useRigPreviewMotionLifecycle } from './motion/useRigMotionLifecycle'
import OutfitWardrobe from './OutfitWardrobe'
import { refreshPersonaStickerAvatar } from './personaAvatar'
import RigCharacter from './rig/RigCharacter'
import {
  applyOutfit,
  bindPortrait,
  hydrateWardrobe,
  isDefaultWardrobeItem,
  MAX_WARDROBE_NAME_CHARS,
  parseWardrobe,
  parseWardrobeName,
  persistWardrobeState,
  seedWardrobeFromIdentity,
  sortWardrobe,
  stampPortrait,
  wardrobeItemLabel,
  withCharacter,
  writeOutfit,
} from './wardrobe'
import './merope.css'
import './merope-motion-home.css'

function reportMeropeError(message: string) {
  if (!message.trim()) return
  showStickyToast({
    message,
    type: 'error',
    replaceKey: 'merope-workbench',
  })
}

function toMeropeActivity(raw: string): MeropeActivity {
  if (raw === 'talking' || raw === 'thinking') return raw
  return 'idle'
}

function joinOverviewSentences(locale: string, parts: string[]): string {
  const cleaned = parts
    .map((part) => part.replaceAll(/[。．.]+$/gu, '').trim())
    .filter(Boolean)
  if (cleaned.length === 0) return ''
  if (locale.startsWith('en')) return `${cleaned.join('. ')}.`
  return `${cleaned.join('。')}。`
}

function structuredFromSnapshot(
  persona: AgentPersona | null,
): StructuredPersona | null {
  if (!persona) return null
  if (persona.persona) {
    const parsed = personaFromApi(persona.persona)
    if (
      parsed.summary ||
      parsed.temperament.length ||
      parsed.likes.length ||
      parsed.drives.length ||
      parsed.socialStyle ||
      parsed.speechStyle
    ) {
      return parsed
    }
  }
  if (persona.personality?.trim()) {
    return parseFlattenedPersona(persona.personality)
  }
  return null
}

interface Props {
  mood: number
  arousal?: number
  activity: string
}

export default function SiteMotionWorkbench({
  mood,
  arousal,
  activity,
}: Props) {
  const { t, locale, format } = useI18n()
  const [wardrobeItems, setWardrobeItems] = useState<WardrobeItem[]>([])
  const [activeOutfitId, setActiveOutfitId] = useState<string | null>(null)
  const [managingOutfitId, setManagingOutfitId] = useState<string | null>(null)
  const personaTouring = useSyncExternalStore(
    subscribeTour,
    () =>
      getTourSnapshot().active &&
      getTourSnapshot().tourId === 'config-persona-owner',
    () => false,
  )
  const managingId = personaTouring ? null : managingOutfitId
  const [rigManifest, setRigManifest] = useState<MeropeRigManifest | null>(null)
  const [rigAssetId, setRigAssetId] = useState<string | null>(null)
  const manifestRef = useRef(rigManifest)
  manifestRef.current = rigManifest
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const [generationFingerprint, setGenerationFingerprint] = useState<
    string | null
  >(null)
  const [seeThroughTokenConfigured, setSeeThroughTokenConfigured] =
    useState(false)
  const [generating, setGenerating] = useState(false)
  const [visualIdentity, setVisualIdentity] =
    useState<UpperBodyVisualIdentity | null>(null)
  const [personaSnapshot, setPersonaSnapshot] = useState<AgentPersona | null>(
    null,
  )
  const [doNotDisturb, setDoNotDisturb] = useState(false)
  const [stickerAvatarUrl, setStickerAvatarUrl] = useState<string | null>(null)
  const [avatarBusy, setAvatarBusy] = useState(false)
  const [dndStart, setDndStart] = useState('')
  const [dndEnd, setDndEnd] = useState('')
  const [dndBusy, setDndBusy] = useState(false)
  const [structuredPersona, setStructuredPersona] =
    useState<StructuredPersona | null>(null)
  const [studioHost, setStudioHost] = useState<HTMLDivElement | null>(null)
  const fillAttempted = useRef(false)
  const rigCharacterRef = useRef<RigCharacterHandle>(null)
  useRigPreviewMotionLifecycle(rigCharacterRef, {
    mood,
    arousal,
    activity: toMeropeActivity(activity),
  })
  const o = t.agentPersona.onboarding
  const visualLabels: Record<UpperBodyVisualIdentityKey, string> = {
    faceDesign: o.visualFaceDesign,
    eyeDesign: o.visualEyeDesign,
    hairShape: o.visualHairShape,
    hairLayerPlan: o.visualHairLayers,
    upperBodySilhouette: o.visualUpperBodySilhouette,
    outfitConstruction: o.visualOutfitConstruction,
    sleeveArmDesign: o.visualSleeveArmDesign,
    materialPlan: o.visualMaterialPlan,
    heroAccessory: o.visualHeroAccessory,
    paletteHint: o.visualPalette,
    motif: o.visualMotif,
  }

  const loadFace = useCallback(async () => {
    const face = await getSiteFace()
    setRigManifest(face.manifest)
    setRigAssetId(face.assetId)
    setPortraitUrl(face.portraitUrl)
    setGenerationFingerprint(face.generationFingerprint)
    return face
  }, [])

  useEffect(() => {
    let cancelled = false
    void loadFace().catch((reason) => {
      if (!cancelled) {
        setRigManifest(null)
        setPortraitUrl(null)
        setGenerationFingerprint(null)
        reportMeropeError(userFacingError(reason, t.merope.loadFailed))
      }
    })
    return () => {
      cancelled = true
    }
  }, [loadFace, t.merope.loadFailed])

  useEffect(() => {
    let cancelled = false
    void agentService
      .getPersona()
      .then((persona) => {
        if (!cancelled) {
          setPersonaSnapshot(persona)
          setStructuredPersona(structuredFromSnapshot(persona))
          const identity = visualIdentityFromProfile(persona?.visualProfile)
          setVisualIdentity(identity)
          const hydrated = hydrateWardrobe(
            persona?.visualProfile,
            identity,
            persona?.portraitAssetId,
          )
          setWardrobeItems(hydrated.items)
          setActiveOutfitId(hydrated.activeId)
          setDoNotDisturb(persona?.doNotDisturb === true)
          setStickerAvatarUrl(persona?.avatarAssetId ?? null)
          setDndStart(persona?.dndStart?.trim() || '')
          setDndEnd(persona?.dndEnd?.trim() || '')
        }
      })
      .catch(() => {
        if (!cancelled) {
          setPersonaSnapshot(null)
          setStructuredPersona(null)
          setVisualIdentity(null)
          setWardrobeItems([])
          setActiveOutfitId(null)
          setDoNotDisturb(false)
          setStickerAvatarUrl(null)
          setDndStart('')
          setDndEnd('')
        }
      })
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    void getSeeThroughStatus()
      .then((status) => {
        if (!cancelled) setSeeThroughTokenConfigured(status.tokenConfigured)
      })
      .catch((reason) => {
        if (!cancelled) {
          setSeeThroughTokenConfigured(false)
          reportMeropeError(userFacingError(reason, t.merope.seeThroughStatusFailed))
        }
      })
    return () => {
      cancelled = true
    }
  }, [t.merope.seeThroughStatusFailed])

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
      if (portraitSpecified && !requestedPortrait) {
        setPortraitUrl(null)
        setGenerationFingerprint(null)
        setRigManifest(null)
      }
      await loadFace()
    },
    [loadFace, personaSnapshot],
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
              pro_unavailable: o.proUnavailable,
              portrait_required: o.importPortraitHint,
              visual_design_unusable: o.importVisualFailed,
              gender_required: o.genderRequired,
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
      o.genderRequired,
      o.importPortraitHint,
      o.importVisualFailed,
      o.proUnavailable,
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

  const applyAddressee = useCallback(
    (saved: {
      mood: number
      activity: string
      doNotDisturb: boolean
      doNotDisturbActive?: boolean
      dndStart?: string | null
      dndEnd?: string | null
    }) => {
      setDoNotDisturb(saved.doNotDisturb)
      setDndStart(saved.dndStart?.trim() || '')
      setDndEnd(saved.dndEnd?.trim() || '')
      setPersonaSnapshot((current) =>
        current
          ? {
              ...current,
              doNotDisturb: saved.doNotDisturb,
              doNotDisturbActive: saved.doNotDisturbActive,
              dndStart: saved.dndStart ?? null,
              dndEnd: saved.dndEnd ?? null,
              mood: saved.mood,
              activity: saved.activity,
            }
          : current,
      )
      window.dispatchEvent(new CustomEvent(ADDRESSEE_UPDATED_EVENT))
    },
    [],
  )

  const saveDoNotDisturb = useCallback(
    async (next: boolean) => {
      const previous = doNotDisturb
      setDoNotDisturb(next)
      setDndBusy(true)
      try {
        applyAddressee(await agentService.putAddressee({ doNotDisturb: next }))
      } catch (reason) {
        setDoNotDisturb(previous)
        reportMeropeError(userFacingError(reason, t.errors.addresseeSaveFailed))
      } finally {
        setDndBusy(false)
      }
    },
    [applyAddressee, doNotDisturb, t.errors.addresseeSaveFailed],
  )

  const saveDndSchedule = useCallback(
    async (start: string, end: string) => {
      setDndBusy(true)
      try {
        applyAddressee(
          await agentService.putAddressee({
            dndStart: start,
            dndEnd: end,
          }),
        )
      } catch (reason) {
        reportMeropeError(userFacingError(reason, t.errors.addresseeSaveFailed))
      } finally {
        setDndBusy(false)
      }
    },
    [applyAddressee, t.errors.addresseeSaveFailed],
  )

  /** 主立绘可能在本页打开后被清掉。 */
  const makeStickerAvatar = useCallback(async () => {
    if (avatarBusy || !portraitUrl) return
    setAvatarBusy(true)
    reportMeropeError('')
    try {
      const result = await generateStickerAvatar()
      setStickerAvatarUrl(result.avatarUrl)
      // 公开配置缓存 30s，必须作废才能换通知图标。
      invalidatePublicConfigCache()
      void refreshPersonaStickerAvatar()
      // 其它头像位可能仍戴上一张贴纸。
      notifyAvatarChanged()
    } catch (reason) {
      reportMeropeError(userFacingError(reason, t.merope.avatarFailed))
    } finally {
      setAvatarBusy(false)
    }
  }, [avatarBusy, portraitUrl, t.merope.avatarFailed])

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
        setStickerAvatarUrl(null)
        invalidatePublicConfigCache()
        void refreshPersonaStickerAvatar()
        notifyAvatarChanged()
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
            image_provider_unconfigured: o.imageProviderUnconfigured,
            image_provider_credits: o.imageProviderCredits,
            image_provider_unauthorized: o.imageProviderUnauthorized,
            image_provider_rate_limited: o.imageProviderRateLimited,
            image_provider_rejected: o.imageProviderRejected,
            image_provider_invalid_response: o.imageProviderInvalidResponse,
            image_provider_unsupported: o.imageProviderUnconfigured,
            portrait_generation_in_progress: o.portraitInProgress,
            character_visual_inputs_changed: o.portraitInputsChanged,
            portrait_generation_failed: o.portraitGenerateFailed,
            portrait_edit_notes_required: o.portraitEditNeedsNotes,
            portrait_adjustment_invalid: o.portraitAdjustmentOutOfScope,
            portrait_adjustment_out_of_scope: o.portraitAdjustmentOutOfScope,
            portrait_required_for_edit: o.portraitEmpty,
            visual_design_required: o.visualDesignRequired,
            visual_gender_mismatch: o.visualGenderMismatch,
            visual_identity_unusable: o.visualIdentityUnusableForPortrait,
            visual_gender_required: o.genderRequired,
          },
        ),
      )
    } finally {
      setGenerating(false)
    }
  }, [
    generating,
    loadFace,
    saveVisualProfile,
    t.merope.visualConfirm,
    t.merope.visualFailed,
    t.agentPersona.onboarding,
    visualIdentity,
    wardrobeItems,
  ])

  const preflightRigPsd = useCallback(
    async (
      file: File,
      onStage: NonNullable<Parameters<typeof preflightRigPsdAsset>[2]>,
      signal?: AbortSignal,
    ) => {
      if (!portraitUrl) throw new Error(t.merope.assetNeedsPortrait)
      return preflightRigPsdAsset(
        file,
        portraitUrl,
        onStage,
        generationFingerprint || undefined,
        signal,
      )
    },
    [generationFingerprint, portraitUrl, t.merope.assetNeedsPortrait],
  )

  const saveSeeThroughToken = useCallback(async (token: string) => {
    const status = await updateSeeThroughToken(token)
    setSeeThroughTokenConfigured(status.tokenConfigured)
  }, [])

  const decomposeRigPsd = useCallback(async () => {
    if (!portraitUrl) throw new Error(t.merope.assetNeedsPortrait)
    return decomposeSitePortraitWithSeeThrough({
      sourceMasterAssetId: portraitUrl,
      sourceGenerationFingerprint: generationFingerprint || undefined,
      resolution: 1280,
      seed: 42,
      splitArmsAndLegs: true,
    })
  }, [generationFingerprint, portraitUrl, t.merope.assetNeedsPortrait])

  const commitRigPsd = useCallback(
    async (
      preflight: Parameters<typeof commitRigPsdAsset>[0],
      onStage: NonNullable<Parameters<typeof commitRigPsdAsset>[1]>,
    ) => {
      const imported = await commitRigPsdAsset(preflight, onStage)
      setRigManifest(imported.manifest)
      const persona = await agentService.getPersona()
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
      await loadFace()
      notifyFaceUpdated()
      return {
        partCount: imported.partCount,
        score: imported.report.score,
      }
    },
    [loadFace],
  )

  const savePoseCorrections = useCallback(async (corrections: PoseCorrection[]) => {
    if (!rigAssetId || !rigManifest) throw new Error(t.merope.poseCorrection.failed)
    const saved = await saveRigPoseCorrections(rigAssetId, corrections)
    notifyFaceUpdated()
    // A late save must not replace a newer portrait/outfit displayed meanwhile.
    if (manifestRef.current !== rigManifest) return
    setRigManifest(saved.manifest)
    setRigAssetId(saved.assetId)
    setWardrobeItems(items => items.map(item => item.id === activeOutfitId ? { ...item, rigAssetId: saved.assetId } : item))
  }, [rigAssetId, rigManifest, activeOutfitId, t.merope.poseCorrection.failed])

  const downloadPortrait = useCallback(
    async (url?: string | null) => {
      const source = siteMediaUrl(url?.trim() || portraitUrl || '')
      if (!source) return
      try {
        const response = await fetch(source)
        if (!response.ok) throw new Error(t.merope.portraitDownloadFailed)
        const blob = await response.blob()
        const objectUrl = URL.createObjectURL(blob)
        const link = document.createElement('a')
        link.href = objectUrl
        link.download = 'portrait.png'
        document.body.appendChild(link)
        link.click()
        link.remove()
        window.setTimeout(() => URL.revokeObjectURL(objectUrl), 1_000)
      } catch {
        window.open(source, '_blank', 'noopener,noreferrer')
      }
    },
    [portraitUrl, t.merope.portraitDownloadFailed],
  )

  const motionEnabled = Boolean(
    rigManifest?.anime25dPlayback &&
    isAnime25DPlayback(rigManifest.anime25dPlayback) &&
    rigManifest.textures[0]?.url,
  )

  const portraitStage = (
    <div className="merope-motion-asset__preview">
      {portraitUrl ? (
        <img
          className={`merope-motion-asset__still${motionEnabled ? ' is-behind' : ''}`}
          src={siteMediaUrl(portraitUrl)}
          alt={t.merope.visualTitle}
        />
      ) : (
        <p className="merope-motion-asset__empty">{t.merope.assetEmpty}</p>
      )}
      <div ref={setStudioHost} className="merope-motion-asset__live" />
    </div>
  )

  const visualIdentityView = (
    show: 'character' | 'outfit',
  ) =>
    visualIdentity ? (
      <VisualIdentityView
        identity={visualIdentity}
        labels={visualLabels}
        characterTitle={t.merope.visualFixedTitle}
        outfitTitle={t.merope.visualOutfitTitle}
        show={show}
        editLabel={o.editVisual}
        cancelLabel={o.cancelEdit}
        saveLabel={o.doneEditing}
        busy={generating}
        onIdentity={(next) => void saveCharacter(next)}
      />
    ) : null

  const statusName = personaSnapshot?.name.trim() || '—'
  const statusMood = o.mood[moodBand(mood, arousal)]
  const statusActivity = o.activity[activityKey(activity)]
  const styleNames = o.clothingStyle
  const rack = sortWardrobe(wardrobeItems)
  const wardrobeCount = rack.length
  const wearingItem = rack.find((item) => item.id === activeOutfitId)
  const wearingName = wearingItem
    ? wardrobeItemLabel(wearingItem, styleNames, t.merope.wardrobeDefault)
    : null
  const wardrobeSentences: string[] = []
  if (wearingName) {
    wardrobeSentences.push(
      format(t.merope.overviewWardrobeWearing, { name: wearingName }),
    )
  }
  if (wardrobeCount > 1) {
    wardrobeSentences.push(
      format(t.merope.overviewWardrobeCount, { n: wardrobeCount }),
    )
  }
  if (wardrobeCount > 0) {
    const portraitReady = rack.filter((item) => item.portraitAssetId).length
    const rigReady = rack.filter((item) => item.rigAssetId).length
    const allReady =
      portraitReady === wardrobeCount && rigReady === wardrobeCount
    if (!allReady) {
      if (wardrobeCount === 1) {
        if (portraitReady) {
          wardrobeSentences.push(t.merope.overviewWardrobeOnePortrait)
        } else if (rigReady) {
          wardrobeSentences.push(t.merope.overviewWardrobeOneRig)
        } else {
          wardrobeSentences.push(t.merope.overviewWardrobeNoneReady)
        }
      } else if (portraitReady === 0 && rigReady === 0) {
        wardrobeSentences.push(t.merope.overviewWardrobeNoneReady)
      } else if (portraitReady === wardrobeCount && rigReady === 0) {
        wardrobeSentences.push(t.merope.overviewWardrobeAllPortraits)
      } else if (rigReady === wardrobeCount && portraitReady === 0) {
        wardrobeSentences.push(t.merope.overviewWardrobeAllRigs)
      } else {
        wardrobeSentences.push(
          format(t.merope.overviewWardrobeMixed, {
            portrait: portraitReady,
            rig: rigReady,
          }),
        )
      }
    }
  }
  const overviewRows: Array<{ key: string; label: string; value: string }> = [
    {
      key: 'wardrobe',
      label: t.merope.wardrobeTitle,
      value:
        wardrobeCount === 0
          ? t.merope.wardrobeEmpty
          : joinOverviewSentences(locale, wardrobeSentences),
    },
    ...(structuredPersona?.summary.trim()
      ? [
          {
            key: 'summary',
            label: t.merope.overviewSummary,
            value: structuredPersona.summary.trim(),
          },
        ]
      : []),
  ]

  const commitDndHours = () => {
    if ((dndStart && dndEnd) || (!dndStart && !dndEnd)) {
      void saveDndSchedule(dndStart, dndEnd)
    }
  }

  const overviewCard = personaSnapshot ? (
    <div className="merope-ob-persona-groups">
      <div className="merope-motion-avatar">
        <section
          className="merope-motion-avatar__pane"
          aria-label={t.merope.avatarTitle}
        >
          <div className="merope-motion-avatar__frame">
            {stickerAvatarUrl ? (
              <img
                className="merope-motion-avatar__preview"
                src={siteMediaUrl(stickerAvatarUrl)}
                alt={t.merope.avatarTitle}
                width={96}
                height={96}
                decoding="async"
              />
            ) : (
              <span className="merope-motion-avatar__preview is-empty" aria-hidden />
            )}
          </div>
          <div className="merope-motion-asset__actions">
            <SettingsButton
              type="button"
              size="sm"
              icon={stickerAvatarUrl ? <LuRefreshCw /> : <LuSparkles />}
              disabled={avatarBusy || !portraitUrl}
              loading={avatarBusy}
              confirm={t.merope.avatarConfirm}
              title={portraitUrl ? undefined : t.merope.avatarNeedsPortrait}
              onClick={() => void makeStickerAvatar()}
            >
              {avatarBusy
                ? t.merope.avatarGenerating
                : stickerAvatarUrl
                  ? t.merope.avatarRegenerate
                  : t.merope.avatarGenerate}
            </SettingsButton>
          </div>
        </section>
        <section
          className="merope-motion-avatar__pane"
          aria-label={t.merope.statusGroup}
        >
          <div className="merope-motion-avatar__field">
            <h2 className="merope-ob-persona-group__title">
              {t.merope.overviewName}
            </h2>
            <p className="merope-motion-avatar__value">{statusName}</p>
          </div>
          <div className="merope-motion-avatar__field">
            <h2 className="merope-ob-persona-group__title">
              {t.merope.statusGroup}
            </h2>
            <p className="merope-motion-avatar__value">
              {statusMood}
              <span aria-hidden> · </span>
              {statusActivity}
            </p>
          </div>
        </section>
      </div>
      <section
        className="merope-ob-persona-group"
        aria-label={t.merope.overviewGroup}
      >
        <h2 className="merope-ob-persona-group__title">{t.merope.overviewGroup}</h2>
        <dl className="merope-ob-persona-view">
          {overviewRows.map((row) => (
            <div key={row.key} className="merope-ob-persona-view__row">
              <div className="merope-ob-persona-view__copy">
                <dt>{row.label}</dt>
                <dd>{row.value}</dd>
              </div>
            </div>
          ))}
          <div className="merope-ob-persona-view__row">
            <div className="merope-ob-persona-view__copy">
              <dt>{t.merope.overviewDoNotDisturb}</dt>
              <dd>
                {doNotDisturb
                  ? t.merope.overviewOn
                  : personaSnapshot.doNotDisturbActive
                    ? t.merope.overviewDndScheduled
                    : t.merope.overviewOff}
              </dd>
            </div>
            <ToggleSwitch
              checked={doNotDisturb}
              disabled={dndBusy}
              aria-label={t.merope.overviewDoNotDisturb}
              onChange={(next) => void saveDoNotDisturb(next)}
            />
          </div>
          <div className="merope-ob-persona-view__row merope-motion-overview__hours-row">
            <div className="merope-ob-persona-view__copy">
              <dt>{t.merope.overviewDndWindow}</dt>
              <dd className="merope-motion-overview__hours">
                <input
                  type="time"
                  className="merope-motion-overview__clock"
                  value={dndStart}
                  disabled={dndBusy}
                  aria-label={t.merope.overviewDndStart}
                  onChange={(event) => setDndStart(event.target.value)}
                  onBlur={commitDndHours}
                />
                <span aria-hidden>–</span>
                <input
                  type="time"
                  className="merope-motion-overview__clock"
                  value={dndEnd}
                  disabled={dndBusy}
                  aria-label={t.merope.overviewDndEnd}
                  onChange={(event) => setDndEnd(event.target.value)}
                  onBlur={commitDndHours}
                />
              </dd>
            </div>
          </div>
        </dl>
      </section>
    </div>
  ) : (
    <p className="merope-motion-home__help">{t.merope.overviewEmpty}</p>
  )

  const personaCard = (
    <div className="merope-motion-persona">
      <PersonaImportPanel
        appearance="settings"
        name={personaSnapshot?.name.trim() || 'Arael'}
        disabled={generating}
        onImported={(next) =>
          void saveStructuredPersona(next, { resetVisual: true })
        }
      />
      {structuredPersona ? (
        <PersonaIdentityView
          persona={structuredPersona}
          labels={{
            temperament: o.fieldTemperament,
            likes: o.fieldLikes,
            drives: o.fieldDrives,
            socialStyle: o.fieldSocial,
            speechStyle: o.fieldVoice,
            summary: o.fieldSummary,
          }}
          editLabel={o.editPersona}
          cancelLabel={o.cancelEdit}
          saveLabel={o.doneEditing}
          groupLabel={t.merope.personaGroup}
          onPersona={(next) => void saveStructuredPersona(next)}
        />
      ) : (
        <p className="merope-motion-home__help">{t.merope.personaEmpty}</p>
      )}
    </div>
  )

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

  const closetCard = (
    <div className="merope-wardrobe__visual">
    <OutfitWardrobe
      identity={visualIdentity}
      gender={genderFromProfile(personaSnapshot?.visualProfile)}
      language={
        typeof personaSnapshot?.visualProfile?.language === 'string'
          ? (personaSnapshot.visualProfile.language as string)
          : locale
      }
      items={wardrobeItems}
      activeId={activeOutfitId}
      portraitUrl={portraitUrl}
      busy={generating}
      filling={generating && !visualIdentity}
      hasPortrait={Boolean(portraitUrl)}
      onFillFromPortrait={() => void fillVisualFromPortrait()}
      onManage={async (item) => {
        setManagingOutfitId(item.id)
      }}
      onDelete={async (id) => {
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
      }}
      onCreated={async (item) => {
        if (!visualIdentity) return
        await saveVisualProfile({
          identity: visualIdentity,
          items: [...wardrobeItems, item],
          activeId: activeOutfitId,
        })
        setManagingOutfitId(item.id)
      }}
    />
    {visualIdentityView('character')}
    </div>
  )

  const managingOutfit = wardrobeItems.find(
    (item) => item.id === managingId,
  )
  const wearingManaged = managingOutfit?.id === activeOutfitId
  const managingIdentity =
    visualIdentity && managingOutfit
      ? applyOutfit(visualIdentity, managingOutfit)
      : null
  const outfitPicture =
    managingOutfit?.portraitAssetId ||
    (wearingManaged ? portraitUrl : null)
  const showWear = Boolean(managingOutfit && !wearingManaged && outfitPicture)
  const showGenerate = Boolean(
    managingOutfit && (wearingManaged || !outfitPicture),
  )
  const outfitCard = managingOutfit ? (
    <div className="merope-wardrobe-page">
      <header className="merope-wardrobe-page__head">
        <button
          type="button"
          className="section-header-back"
          onClick={() => setManagingOutfitId(null)}
          aria-label={t.common.back}
        >
          <LuChevronLeft size={18} aria-hidden />
          <span>{t.common.back}</span>
        </button>
      </header>
      {outfitPicture ? (
        <div className="merope-wardrobe-page__portrait">
          <img src={siteMediaUrl(outfitPicture)} alt="" draggable={false} />
        </div>
      ) : (
        <p className="merope-wardrobe__caption">{t.merope.assetEmpty}</p>
      )}
      <div className="merope-motion-asset__make">
      {isDefaultWardrobeItem(managingOutfit) ? (
        <p className="merope-wardrobe__caption">{t.merope.wardrobeDefault}</p>
      ) : (
      <Field
        label={t.merope.wardrobeName}
        optional
        optionalLabel={o.optional}
      >
        <TextInput
          key={managingOutfit.id}
          defaultValue={managingOutfit.name ?? ''}
          maxLength={MAX_WARDROBE_NAME_CHARS}
          disabled={generating}
          placeholder={wardrobeItemLabel(
            { clothingStyle: managingOutfit.clothingStyle },
            t.agentPersona.onboarding.clothingStyle,
          )}
          onBlur={(event) => {
            const next = parseWardrobeName(event.currentTarget.value)
            if ((next ?? '') === (managingOutfit.name ?? '')) return
            void renameOutfit(managingOutfit.id, event.currentTarget.value).catch(
              (reason) => {
                reportMeropeError(userFacingError(reason, t.merope.wardrobeRenameFailed))
              },
            )
          }}
        />
      </Field>
      )}
      <div className="merope-motion-asset__actions">
        {showWear ? (
          <SettingsButton
            type="button"
            size="sm"
            disabled={generating || !visualIdentity}
            loading={generating}
            onClick={() => {
              void wearOutfit(managingOutfit).catch((reason) => {
                reportMeropeError(
                  userFacingError(reason, t.merope.wardrobeApplyFailed),
                )
              })
            }}
          >
            {generating
              ? t.merope.visualGenerating
              : t.merope.wardrobeWear}
          </SettingsButton>
        ) : null}
        {showGenerate ? (
          <SettingsButton
            type="button"
            size="sm"
            disabled={generating || !visualIdentity}
            loading={generating}
            onClick={() => void generatePortrait(managingOutfit)}
          >
            {generating
              ? t.merope.visualGenerating
              : outfitPicture
                ? t.merope.visualRegenerate
                : t.merope.visualGenerate}
          </SettingsButton>
        ) : null}
        {outfitPicture ? (
          <SettingsButton
            type="button"
            size="sm"
            variant="secondary"
            disabled={generating}
            onClick={() => void downloadPortrait(outfitPicture)}
          >
            {t.merope.visualDownload}
          </SettingsButton>
        ) : null}
        {wearingManaged ? (
          <PortraitImportButton
            appearance="settings"
            disabled={generating}
            onError={reportMeropeError}
            onUploaded={async (url) => {
              reportMeropeError('')
              await loadFace()
              const saved = await agentService.getPersona()
              setPersonaSnapshot(saved)
              const identity = visualIdentityFromProfile(saved?.visualProfile)
              const wardrobe = hydrateWardrobe(saved?.visualProfile, identity, saved?.portraitAssetId)
              setWardrobeItems(wardrobe.items)
              setActiveOutfitId(wardrobe.activeId)
              setStickerAvatarUrl(null)
              invalidatePublicConfigCache()
              void refreshPersonaStickerAvatar()
              notifyAvatarChanged()
              try {
                await applyVisualFromPortrait(url)
              } catch {
                await loadFace()
              }
              notifyFaceUpdated()
            }}
          />
        ) : null}
      </div>
      {managingIdentity ? (
        <VisualIdentityView
          identity={managingIdentity}
          labels={visualLabels}
          characterTitle={t.merope.visualFixedTitle}
          outfitTitle={t.merope.visualOutfitTitle}
          show="outfit"
          editLabel={o.editVisual}
          cancelLabel={o.cancelEdit}
          saveLabel={o.doneEditing}
          busy={generating}
          onIdentity={(next) => {
            void saveOutfitDesign(managingOutfit.id, next)
          }}
        />
      ) : null}
      </div>
    </div>
  ) : null

  const studio = (
    <section
      className="merope-motion-home merope-motion-home--settings"
      aria-label={t.merope.portraitGroup}
    >
      <div className="merope-motion-home__studio">
        <div className="merope-motion-home__stage">
          <RigCharacter
            ref={rigCharacterRef}
            activity={toMeropeActivity(activity)}
            fallbackUrl={portraitUrl}
            manifest={rigManifest}
            mood={mood}
            manualControl
          />
        </div>
      </div>
    </section>
  )

  return (
    <div className="merope-motion-page">
      <aside
        className="merope-motion-page__stage"
        aria-label={t.merope.visualTitle}
        data-tour="config-persona-portrait"
      >
        {portraitStage}
      </aside>
      <div className="merope-motion-page__settings">
        <Anime25DWorkbench
          overviewLead={overviewCard}
          personaLead={personaCard}
          wardrobeLead={closetCard}
          outfitLead={outfitCard}
          outfitRig={wearingManaged}
          characterRef={rigCharacterRef}
          sourceMasterAssetId={portraitUrl || ''}
          sourceGenerationFingerprint={generationFingerprint || undefined}
          seeThroughTokenConfigured={seeThroughTokenConfigured}
          onSaveSeeThroughToken={saveSeeThroughToken}
          onDecomposeRigPsd={decomposeRigPsd}
          onPreflightRigPsd={preflightRigPsd}
          onCommitRigPsd={commitRigPsd}
          motionEnabled={motionEnabled}
          correctionPlayback={rigManifest?.anime25dPlayback ?? null}
          correctionAssetId={rigAssetId}
          onSavePoseCorrections={savePoseCorrections}
        />
      </div>
      {portraitUrl && motionEnabled && studioHost
        ? createPortal(studio, studioHost)
        : null}
    </div>
  )
}
