import type { RigCharacterHandle } from './rig/RigCharacter'
import type { CompanionRigManifest } from './rig/types'
import type { CompanionActivity } from './types'
import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { InfoActionCard, InputItem } from '../../components/settings'
import { useI18n } from '../../contexts/I18nContext'
import Anime25DWorkbench from './anime25drig/Anime25DWorkbench'
import { generationFailureMessage } from '../../components/agent/onboarding/generationError'
import {
  decomposeSitePortraitWithSeeThrough,
  generateSitePortrait,
  getSeeThroughStatus,
  getSiteFace,
  updateSeeThroughToken,
  uploadSitePortrait,
} from './api'
import { compositePsdToPng } from './rig/psdImporter'
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
  const [portraitRequirements, setPortraitRequirements] = useState('')
  const [reviewDock, setReviewDock] = useState<HTMLDivElement | null>(null)
  const [studioHost, setStudioHost] = useState<HTMLDivElement | null>(null)
  const rigCharacterRef = useRef<RigCharacterHandle>(null)

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
          setError(
            reason instanceof Error ? reason.message : t.companion.loadFailed,
          )
        }
      })
    return () => {
      cancelled = true
    }
  }, [loadFace, t.companion.loadFailed])

  useEffect(() => {
    let cancelled = false
    void getSeeThroughStatus()
      .then((status) => {
        if (!cancelled) setSeeThroughTokenConfigured(status.tokenConfigured)
      })
      .catch((reason) => {
        if (!cancelled) {
          setSeeThroughTokenConfigured(false)
          setError(
            reason instanceof Error ? reason.message : t.companion.loadFailed,
          )
        }
      })
    return () => {
      cancelled = true
    }
  }, [t.companion.loadFailed])

  const generatePortrait = useCallback(async () => {
    if (generating) return
    if (!window.confirm(t.companion.visualConfirm)) return
    setGenerating(true)
    setError('')
    try {
      await generateSitePortrait(portraitRequirements.trim() || undefined)
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
    portraitRequirements,
    t.companion.visualConfirm,
    t.companion.visualFailed,
    t.life.onboarding,
  ])

  const preflightRigPsd = useCallback(
    async (
      file: File,
      onStage: NonNullable<Parameters<typeof preflightRigPsdAsset>[2]>,
    ) => {
      let master = portraitUrl
      let fingerprint = generationFingerprint || undefined
      if (!master) {
        const png = await compositePsdToPng(file)
        const uploaded = await uploadSitePortrait(png)
        master = uploaded.portraitUrl
        fingerprint = undefined
        setPortraitUrl(master)
        notifyFaceUpdated()
      }
      return preflightRigPsdAsset(file, master, onStage, fingerprint)
    },
    [generationFingerprint, portraitUrl],
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

  const portraitCard = (
    <>
      <InputItem
        itemKey="portrait-requirements"
        label={t.companion.visualRequirements}
        description={t.companion.visualRequirementsDescription}
        value={portraitRequirements}
        onChange={setPortraitRequirements}
        placeholder={t.companion.visualRequirementsPlaceholder}
        multiline
        rows={3}
        disabled={generating}
      />
      <InfoActionCard
        className="life-motion-portrait-card"
        copyable={false}
        tone={portraitUrl ? 'default' : 'info'}
        title={t.companion.visualTitle}
        empty={!portraitUrl}
        emptyText={t.companion.visualDescription}
        preview={
          portraitUrl ? (
            <img src={portraitUrl} alt={t.companion.visualTitle} />
          ) : undefined
        }
        actions={[
          {
            key: 'portrait',
            label: generating
              ? t.companion.visualGenerating
              : portraitUrl
                ? t.companion.visualRegenerate
                : t.companion.visualGenerate,
            onClick: () => void generatePortrait(),
            disabled: generating,
            loading: generating,
          },
          ...(portraitUrl
            ? [
                {
                  key: 'download',
                  label: t.companion.visualDownload,
                  onClick: () => void downloadPortrait(),
                  disabled: generating,
                },
              ]
            : []),
        ]}
        footer={error || undefined}
      />
    </>
  )

  const motionEnabled = Boolean(
    rigManifest?.anime25dPlayback &&
      isAnime25DPlayback(rigManifest.anime25dPlayback) &&
      rigManifest.textures[0]?.url,
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
    <>
    <Anime25DWorkbench
      essentialsLead={
        <>
          {portraitCard}
          <div ref={setStudioHost} className="life-motion-home__host" />
        </>
      }
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
    {portraitUrl && studioTarget ? createPortal(studio, studioTarget) : null}
    </>
  )
}
