import type { RigCharacterHandle } from './rig/RigCharacter'
import type { CompanionRigManifest } from './rig/types'
import type { CompanionActivity } from './types'
import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import {
  flattenPersona,
  parseFlattenedPersona,
  personaFromApi,
  visualIdentityFromProfile,
  type StructuredPersona,
  type UpperBodyVisualIdentity,
  type UpperBodyVisualIdentityKey,
} from '../../components/agent/onboarding/onboardingTypes'
import PersonaIdentityView from '../../components/agent/onboarding/ui/PersonaIdentityView'
import VisualIdentityView from '../../components/agent/onboarding/ui/VisualIdentityView'
import {
  ADDRESSEE_UPDATED_EVENT,
  activityKey,
  moodBand,
} from '../../components/agent/lifeVitals'
import { SettingsButton, ToggleSwitch } from '../../components/settings'
import { useI18n } from '../../contexts/I18nContext'
import { userFacingError } from '../../utils/userFacingError'
import { agentService } from '../../services/agent'
import type { AgentPersona } from '../../services/agent/agentApi'
import Anime25DWorkbench from './anime25drig/Anime25DWorkbench'
import { generationFailureMessage } from '../../components/agent/onboarding/generationError'
import {
  decomposeSitePortraitWithSeeThrough,
  generateSitePortrait,
  getSeeThroughStatus,
  getSiteFace,
  updateSeeThroughToken,
} from './api'
import {
  commitRigPsdAsset,
  preflightRigPsdAsset,
} from './assets/pipeline'
import { notifyFaceUpdated } from './events'
import { isAnime25DPlayback } from './anime25drig/types'
import RigCharacter from './rig/RigCharacter'
import './companion.css'
import './life-motion-home.css'

function toCompanionActivity(raw: string): CompanionActivity {
  if (raw === 'talking' || raw === 'thinking') return raw
  return 'idle'
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
  activity: string
}

export default function SiteMotionWorkbench({ mood, activity }: Props) {
  const { t } = useI18n()
  const [rigManifest, setRigManifest] = useState<CompanionRigManifest | null>(
    null,
  )
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const [generationFingerprint, setGenerationFingerprint] = useState<
    string | null
  >(null)
  const [seeThroughTokenConfigured, setSeeThroughTokenConfigured] = useState(false)
  const [reviewMode, setReviewMode] = useState(false)
  const [error, setError] = useState('')
  const [generating, setGenerating] = useState(false)
  const [visualIdentity, setVisualIdentity] =
    useState<UpperBodyVisualIdentity | null>(null)
  const [personaSnapshot, setPersonaSnapshot] = useState<AgentPersona | null>(
    null,
  )
  const [doNotDisturb, setDoNotDisturb] = useState(false)
  const [dndStart, setDndStart] = useState('')
  const [dndEnd, setDndEnd] = useState('')
  const [dndBusy, setDndBusy] = useState(false)
  const [structuredPersona, setStructuredPersona] =
    useState<StructuredPersona | null>(null)
  const [reviewDock, setReviewDock] = useState<HTMLDivElement | null>(null)
  const [studioHost, setStudioHost] = useState<HTMLDivElement | null>(null)
  const rigCharacterRef = useRef<RigCharacterHandle>(null)
  const o = t.life.onboarding
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
    setPortraitUrl(face.portraitUrl)
    setGenerationFingerprint(face.generationFingerprint)
    return face
  }, [])

  useEffect(() => {
    let cancelled = false
    void loadFace()
      .catch((reason) => {
        if (!cancelled) {
          setRigManifest(null)
          setPortraitUrl(null)
          setGenerationFingerprint(null)
          setError(userFacingError(reason, t.companion.loadFailed))
        }
      })
    return () => {
      cancelled = true
    }
  }, [loadFace, t.companion.loadFailed])

  useEffect(() => {
    let cancelled = false
    void agentService
      .getPersona()
      .then((persona) => {
        if (!cancelled) {
          setPersonaSnapshot(persona)
          setStructuredPersona(structuredFromSnapshot(persona))
          setVisualIdentity(visualIdentityFromProfile(persona?.visualProfile))
          setDoNotDisturb(persona?.doNotDisturb === true)
          setDndStart(persona?.dndStart?.trim() || '')
          setDndEnd(persona?.dndEnd?.trim() || '')
        }
      })
      .catch(() => {
        if (!cancelled) {
          setPersonaSnapshot(null)
          setStructuredPersona(null)
          setVisualIdentity(null)
          setDoNotDisturb(false)
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
          setError(userFacingError(reason, t.companion.loadFailed))
        }
      })
    return () => {
      cancelled = true
    }
  }, [t.companion.loadFailed])

  const saveVisualIdentity = useCallback(
    async (next: UpperBodyVisualIdentity) => {
      setVisualIdentity(next)
      if (!personaSnapshot) return
      try {
        const saved = await agentService.putPersona({
          name: personaSnapshot.name,
          personality: personaSnapshot.personality ?? '',
          persona: personaSnapshot.persona,
          visualProfile: {
            ...(personaSnapshot.visualProfile ?? {}),
            visualIdentity: next,
          },
        })
        setPersonaSnapshot(saved)
      } catch (reason) {
        setError(userFacingError(reason, o.visualDesignSaveFailed))
      }
    },
    [o.visualDesignSaveFailed, personaSnapshot],
  )

  const saveStructuredPersona = useCallback(
    async (next: StructuredPersona) => {
      setStructuredPersona(next)
      if (!personaSnapshot) return
      try {
        const saved = await agentService.putPersona({
          name: personaSnapshot.name,
          personality: flattenPersona(next),
          persona: {
            ...(personaSnapshot.persona ?? {}),
            displayName: personaSnapshot.name,
            ...next,
          },
          visualProfile: personaSnapshot.visualProfile,
        })
        setPersonaSnapshot(saved)
        window.dispatchEvent(new CustomEvent('arael-persona-updated'))
      } catch (reason) {
        setError(userFacingError(reason, o.saveFailed))
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
        setError(userFacingError(reason, t.companion.loadFailed))
      } finally {
        setDndBusy(false)
      }
    },
    [applyAddressee, doNotDisturb, t.companion.loadFailed],
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
        setError(userFacingError(reason, t.companion.loadFailed))
      } finally {
        setDndBusy(false)
      }
    },
    [applyAddressee, t.companion.loadFailed],
  )

  const generatePortrait = useCallback(async () => {
    if (generating) return
    if (!window.confirm(t.companion.visualConfirm)) return
    setGenerating(true)
    setError('')
    try {
      await generateSitePortrait()
      await loadFace()
      notifyFaceUpdated()
    } catch (reason) {
      const o = t.life.onboarding
      setError(
        generationFailureMessage(
          reason,
          t.companion.visualFailed,
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
    t.companion.visualConfirm,
    t.companion.visualFailed,
    t.life.onboarding,
  ])

  const preflightRigPsd = useCallback(
    async (
      file: File,
      onStage: NonNullable<Parameters<typeof preflightRigPsdAsset>[2]>,
    ) => {
      if (!portraitUrl) throw new Error(t.companion.assetNeedsPortrait)
      return preflightRigPsdAsset(
        file,
        portraitUrl,
        onStage,
        generationFingerprint || undefined,
      )
    },
    [generationFingerprint, portraitUrl, t.companion.assetNeedsPortrait],
  )

  const saveSeeThroughToken = useCallback(async (token: string) => {
    const status = await updateSeeThroughToken(token)
    setSeeThroughTokenConfigured(status.tokenConfigured)
  }, [])

  const decomposeRigPsd = useCallback(async () => {
    if (!portraitUrl) throw new Error(t.companion.visualFailed)
    return decomposeSitePortraitWithSeeThrough({
      sourceMasterAssetId: portraitUrl,
      sourceGenerationFingerprint: generationFingerprint || undefined,
      resolution: 768,
      seed: 42,
      splitArmsAndLegs: true,
    })
  }, [generationFingerprint, portraitUrl, t.companion.visualFailed])

  const commitRigPsd = useCallback(
    async (
      preflight: Parameters<typeof commitRigPsdAsset>[0],
      onStage: NonNullable<Parameters<typeof commitRigPsdAsset>[1]>,
    ) => {
      const imported = await commitRigPsdAsset(preflight, onStage)
      setRigManifest(imported.manifest)
      await loadFace()
      notifyFaceUpdated()
      return {
        partCount: imported.partCount,
        score: imported.report.score,
      }
    },
    [loadFace],
  )

  const exitReview = useCallback(() => {
    rigCharacterRef.current?.stopMotionPlan()
    setReviewMode(false)
  }, [])

  useEffect(() => {
    if (!reviewMode) return undefined
    const previousOverflow = document.body.style.overflow
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') exitReview()
    }
    document.body.style.overflow = 'hidden'
    window.addEventListener('keydown', onKeyDown)
    return () => {
      document.body.style.overflow = previousOverflow
      window.removeEventListener('keydown', onKeyDown)
    }
  }, [exitReview, reviewMode])

  const downloadPortrait = useCallback(async () => {
    if (!portraitUrl) return
    try {
      const response = await fetch(portraitUrl)
      if (!response.ok) throw new Error(t.companion.visualFailed)
      const blob = await response.blob()
      const objectUrl = URL.createObjectURL(blob)
      const link = document.createElement('a')
      link.href = objectUrl
      link.download = 'arael-portrait.png'
      document.body.appendChild(link)
      link.click()
      link.remove()
      window.setTimeout(() => URL.revokeObjectURL(objectUrl), 1_000)
    } catch {
      window.open(portraitUrl, '_blank', 'noopener,noreferrer')
    }
  }, [portraitUrl, t.companion.visualFailed])

  const motionEnabled = Boolean(
    rigManifest?.anime25dPlayback &&
      isAnime25DPlayback(rigManifest.anime25dPlayback) &&
      rigManifest.textures[0]?.url,
  )

  const portraitStage = (
    <div className="life-motion-asset__preview">
      {portraitUrl ? (
        <img
          className={`life-motion-asset__still${motionEnabled ? ' is-behind' : ''}`}
          src={portraitUrl}
          alt={t.companion.visualTitle}
        />
      ) : (
        <p className="life-motion-asset__empty">{t.companion.assetEmpty}</p>
      )}
      <div ref={setStudioHost} className="life-motion-asset__live" />
    </div>
  )

  const visualSource = (
    <section aria-label={t.companion.visualSourceTitle}>
      <h3 className="life-motion-visual__heading">
        {t.companion.visualSourceTitle}
      </h3>
      {visualIdentity ? (
        <VisualIdentityView
          identity={visualIdentity}
          labels={visualLabels}
          characterTitle={o.visualGroupCharacter}
          outfitTitle={o.visualGroupOutfit}
          editLabel={o.editVisual}
          cancelLabel={o.cancelEdit}
          saveLabel={o.doneEditing}
          busy={generating}
          onIdentity={(next) => void saveVisualIdentity(next)}
        />
      ) : (
        <p className="life-motion-home__help">{t.companion.visualSourceEmpty}</p>
      )}
    </section>
  )

  const overviewRows: Array<{ key: string; label: string; value: string }> = [
    {
      key: 'name',
      label: t.companion.overviewName,
      value: personaSnapshot?.name.trim() || '—',
    },
    {
      key: 'mood',
      label: t.companion.overviewMood,
      value: o.mood[moodBand(mood)],
    },
    {
      key: 'activity',
      label: t.companion.overviewActivity,
      value: o.activity[activityKey(activity)],
    },
    {
      key: 'portrait',
      label: t.companion.overviewPortrait,
      value: portraitUrl
        ? t.companion.overviewPortraitReady
        : t.companion.overviewPortraitEmpty,
    },
    {
      key: 'rig',
      label: t.companion.overviewRig,
      value: motionEnabled
        ? t.companion.overviewRigReady
        : t.companion.overviewRigEmpty,
    },
    ...(structuredPersona?.summary.trim()
      ? [
          {
            key: 'summary',
            label: t.companion.overviewSummary,
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
    <div className="life-ob-persona-groups">
      <section
        className="life-ob-persona-group"
        aria-label={t.companion.overviewGroup}
      >
        <dl className="life-ob-persona-view">
          {overviewRows.map((row) => (
            <div key={row.key} className="life-ob-persona-view__row">
              <div className="life-ob-persona-view__copy">
                <dt>{row.label}</dt>
                <dd>{row.value}</dd>
              </div>
            </div>
          ))}
          <div className="life-ob-persona-view__row life-motion-overview__dnd">
            <div className="life-ob-persona-view__copy">
              <dt>{t.companion.overviewDoNotDisturb}</dt>
              <dd>
                {doNotDisturb
                  ? t.companion.overviewOn
                  : personaSnapshot.doNotDisturbActive
                    ? t.companion.overviewDndScheduled
                    : t.companion.overviewOff}
              </dd>
            </div>
            <ToggleSwitch
              checked={doNotDisturb}
              disabled={dndBusy}
              aria-label={t.companion.overviewDoNotDisturb}
              onChange={(next) => void saveDoNotDisturb(next)}
            />
          </div>
          <div className="life-ob-persona-view__row life-motion-overview__hours-row">
            <div className="life-ob-persona-view__copy">
              <dt>{t.companion.overviewDndWindow}</dt>
              <dd className="life-motion-overview__hours">
                <input
                  type="time"
                  className="life-motion-overview__clock"
                  value={dndStart}
                  disabled={dndBusy}
                  aria-label={t.companion.overviewDndStart}
                  onChange={(event) => setDndStart(event.target.value)}
                  onBlur={commitDndHours}
                />
                <span aria-hidden>–</span>
                <input
                  type="time"
                  className="life-motion-overview__clock"
                  value={dndEnd}
                  disabled={dndBusy}
                  aria-label={t.companion.overviewDndEnd}
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
    <p className="life-motion-home__help">{t.companion.overviewEmpty}</p>
  )

  const personaCard = structuredPersona ? (
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
      groupLabel={t.companion.personaGroup}
      onPersona={(next) => void saveStructuredPersona(next)}
    />
  ) : (
    <p className="life-motion-home__help">{t.companion.personaEmpty}</p>
  )

  const portraitCard = (
    <div className="life-motion-asset__make">
      <div className="life-motion-asset__actions">
        <SettingsButton
          type="button"
          size="sm"
          disabled={generating}
          loading={generating}
          onClick={() => void generatePortrait()}
        >
          {generating
            ? t.companion.visualGenerating
            : portraitUrl
              ? t.companion.visualRegenerate
              : t.companion.visualGenerate}
        </SettingsButton>
        {portraitUrl ? (
          <SettingsButton
            type="button"
            size="sm"
            variant="secondary"
            disabled={generating}
            onClick={() => void downloadPortrait()}
          >
            {t.companion.visualDownload}
          </SettingsButton>
        ) : null}
      </div>
      {error ? (
        <p className="life-motion-home__help" role="alert">
          {error}
        </p>
      ) : null}
      {visualSource}
    </div>
  )

  const studioTarget = reviewMode ? document.body : studioHost
  const studio = (
    <section
      className={`life-motion-home life-motion-home--settings${reviewMode ? ' is-reviewing' : ''}`}
      aria-label={
        reviewMode ? t.companion.motionReviewEnter : t.companion.portraitGroup
      }
      aria-modal={reviewMode || undefined}
      role={reviewMode ? 'dialog' : undefined}
    >
      <div className="life-motion-home__studio">
        <div className="life-motion-home__stage">
          <RigCharacter
            ref={rigCharacterRef}
            activity={toCompanionActivity(activity)}
            fallbackUrl={portraitUrl || ''}
            manifest={rigManifest}
            mood={mood}
            manualControl
          />
        </div>
        <div ref={setReviewDock} className="life-motion-home__dock" />
      </div>
    </section>
  )

  return (
    <div className="life-motion-page">
      <aside className="life-motion-page__stage" aria-label={t.companion.visualTitle}>
        {portraitStage}
      </aside>
      <div className="life-motion-page__settings">
        <Anime25DWorkbench
          overviewLead={overviewCard}
          personaLead={personaCard}
          essentialsLead={portraitCard}
          reviewMode={reviewMode}
          onReviewModeChange={(reviewing) => {
            if (reviewing) setReviewMode(true)
            else exitReview()
          }}
          characterRef={rigCharacterRef}
          sourceMasterAssetId={portraitUrl || ''}
          sourceGenerationFingerprint={generationFingerprint || undefined}
          seeThroughTokenConfigured={seeThroughTokenConfigured}
          onSaveSeeThroughToken={saveSeeThroughToken}
          onDecomposeRigPsd={decomposeRigPsd}
          onPreflightRigPsd={preflightRigPsd}
          onCommitRigPsd={commitRigPsd}
          reviewDock={reviewDock}
          motionEnabled={motionEnabled}
        />
      </div>
      {portraitUrl && motionEnabled && studioTarget
        ? createPortal(studio, studioTarget)
        : null}
    </div>
  )
}
