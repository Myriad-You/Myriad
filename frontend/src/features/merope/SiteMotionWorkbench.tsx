import type {
  StructuredPersona,
  UpperBodyVisualIdentity,
  UpperBodyVisualIdentityKey,
} from '../../components/agent/onboarding/onboardingTypes'
import type { AgentPersona } from '../../services/agent/agentApi'
import type { RigCharacterHandle } from './rig/RigCharacter'
import type { MeropeRigManifest } from './rig/types'
import type { MeropeActivity } from './types'
import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import {
  activityKey,
  ADDRESSEE_UPDATED_EVENT,
  moodBand,
} from '../../components/agent/meropeVitals'
import { generationFailureMessage } from '../../components/agent/onboarding/generationError'
import {
  flattenPersona,
  parseFlattenedPersona,
  personaFromApi,
  visualIdentityFromProfile,
} from '../../components/agent/onboarding/onboardingTypes'
import PersonaIdentityView from '../../components/agent/onboarding/ui/PersonaIdentityView'
import PersonaImportPanel from '../../components/agent/onboarding/ui/PersonaImportPanel'
import PortraitImportButton from '../../components/agent/onboarding/ui/PortraitImportButton'
import VisualIdentityView from '../../components/agent/onboarding/ui/VisualIdentityView'
import { SettingsButton, ToggleSwitch } from '../../components/settings'
import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { userFacingError } from '../../utils/userFacingError'
import Anime25DWorkbench from './anime25drig/Anime25DWorkbench'
import { isAnime25DPlayback } from './anime25drig/types'
import {
  decomposeSitePortraitWithSeeThrough,
  generateSitePortrait,
  getSeeThroughStatus,
  getSiteFace,
  updateSeeThroughToken,
} from './api'
import { commitRigPsdAsset, preflightRigPsdAsset } from './assets/pipeline'
import { notifyFaceUpdated } from './events'
import { useRigPreviewMotionLifecycle } from './motion/useRigMotionLifecycle'
import RigCharacter from './rig/RigCharacter'
import './merope.css'
import './merope-motion-home.css'

function toMeropeActivity(raw: string): MeropeActivity {
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
  arousal?: number
  activity: string
}

export default function SiteMotionWorkbench({
  mood,
  arousal,
  activity,
}: Props) {
  const { t } = useI18n()
  const [rigManifest, setRigManifest] = useState<MeropeRigManifest | null>(null)
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const [generationFingerprint, setGenerationFingerprint] = useState<
    string | null
  >(null)
  const [seeThroughTokenConfigured, setSeeThroughTokenConfigured] =
    useState(false)
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
  const [studioHost, setStudioHost] = useState<HTMLDivElement | null>(null)
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
        setError(userFacingError(reason, t.merope.loadFailed))
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
          setError(userFacingError(reason, t.merope.seeThroughStatusFailed))
        }
      })
    return () => {
      cancelled = true
    }
  }, [t.merope.seeThroughStatusFailed])

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
    async (next: StructuredPersona, options?: { resetVisual?: boolean }) => {
      setStructuredPersona(next)
      const name = personaSnapshot?.name.trim() || 'Arael'
      const visualProfile = options?.resetVisual
        ? {
            ...(personaSnapshot?.visualProfile ?? {}),
            visualIdentity: null,
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
        if (options?.resetVisual) setVisualIdentity(null)
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
        setError(userFacingError(reason, t.errors.addresseeSaveFailed))
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
        setError(userFacingError(reason, t.errors.addresseeSaveFailed))
      } finally {
        setDndBusy(false)
      }
    },
    [applyAddressee, t.errors.addresseeSaveFailed],
  )

  const generatePortrait = useCallback(async () => {
    if (generating) return
    if (!window.confirm(t.merope.visualConfirm)) return
    setGenerating(true)
    setError('')
    try {
      await generateSitePortrait()
      await loadFace()
      notifyFaceUpdated()
    } catch (reason) {
      const o = t.agentPersona.onboarding
      setError(
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
    t.merope.visualConfirm,
    t.merope.visualFailed,
    t.agentPersona.onboarding,
  ])

  const preflightRigPsd = useCallback(
    async (
      file: File,
      onStage: NonNullable<Parameters<typeof preflightRigPsdAsset>[2]>,
    ) => {
      if (!portraitUrl) throw new Error(t.merope.assetNeedsPortrait)
      return preflightRigPsdAsset(
        file,
        portraitUrl,
        onStage,
        generationFingerprint || undefined,
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
      resolution: 768,
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
      await loadFace()
      notifyFaceUpdated()
      return {
        partCount: imported.partCount,
        score: imported.report.score,
      }
    },
    [loadFace],
  )

  const downloadPortrait = useCallback(async () => {
    if (!portraitUrl) return
    try {
      const response = await fetch(portraitUrl)
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
      window.open(portraitUrl, '_blank', 'noopener,noreferrer')
    }
  }, [portraitUrl, t.merope.portraitDownloadFailed])

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
          src={portraitUrl}
          alt={t.merope.visualTitle}
        />
      ) : (
        <p className="merope-motion-asset__empty">{t.merope.assetEmpty}</p>
      )}
      <div ref={setStudioHost} className="merope-motion-asset__live" />
    </div>
  )

  const visualSource = (
    <section aria-label={t.merope.visualSourceTitle}>
      <h3 className="merope-motion-visual__heading">
        {t.merope.visualSourceTitle}
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
        <p className="merope-motion-home__help">{t.merope.visualSourceEmpty}</p>
      )}
    </section>
  )

  const overviewRows: Array<{ key: string; label: string; value: string }> = [
    {
      key: 'name',
      label: t.merope.overviewName,
      value: personaSnapshot?.name.trim() || '—',
    },
    {
      key: 'mood',
      label: t.merope.overviewMood,
      value: o.mood[moodBand(mood, arousal)],
    },
    {
      key: 'activity',
      label: t.merope.overviewActivity,
      value: o.activity[activityKey(activity)],
    },
    {
      key: 'portrait',
      label: t.merope.overviewPortrait,
      value: portraitUrl
        ? t.merope.overviewPortraitReady
        : t.merope.overviewPortraitEmpty,
    },
    {
      key: 'rig',
      label: t.merope.overviewRig,
      value: motionEnabled
        ? t.merope.overviewRigReady
        : t.merope.overviewRigEmpty,
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
      <section
        className="merope-ob-persona-group"
        aria-label={t.merope.overviewGroup}
      >
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

  const portraitCard = (
    <div className="merope-motion-asset__make">
      <div className="merope-motion-asset__actions">
        <SettingsButton
          type="button"
          size="sm"
          disabled={generating}
          loading={generating}
          onClick={() => void generatePortrait()}
        >
          {generating
            ? t.merope.visualGenerating
            : portraitUrl
              ? t.merope.visualRegenerate
              : t.merope.visualGenerate}
        </SettingsButton>
        {portraitUrl ? (
          <SettingsButton
            type="button"
            size="sm"
            variant="secondary"
            disabled={generating}
            onClick={() => void downloadPortrait()}
          >
            {t.merope.visualDownload}
          </SettingsButton>
        ) : null}
        <PortraitImportButton
          appearance="settings"
          disabled={generating}
          onError={setError}
          onUploaded={async () => {
            setError('')
            await loadFace()
          }}
        />
      </div>
      {error ? (
        <p className="merope-motion-home__help" role="alert">
          {error}
        </p>
      ) : null}
      {visualSource}
    </div>
  )

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
      >
        {portraitStage}
      </aside>
      <div className="merope-motion-page__settings">
        <Anime25DWorkbench
          overviewLead={overviewCard}
          personaLead={personaCard}
          essentialsLead={portraitCard}
          characterRef={rigCharacterRef}
          sourceMasterAssetId={portraitUrl || ''}
          sourceGenerationFingerprint={generationFingerprint || undefined}
          seeThroughTokenConfigured={seeThroughTokenConfigured}
          onSaveSeeThroughToken={saveSeeThroughToken}
          onDecomposeRigPsd={decomposeRigPsd}
          onPreflightRigPsd={preflightRigPsd}
          onCommitRigPsd={commitRigPsd}
          motionEnabled={motionEnabled}
        />
      </div>
      {portraitUrl && motionEnabled && studioHost
        ? createPortal(studio, studioHost)
        : null}
    </div>
  )
}
