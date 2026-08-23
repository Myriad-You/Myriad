import type { OnboardingHeaderChrome } from '../onboardingTypes'
import { LuImage } from '@lib/icons'
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import {
  generateSitePortrait,
  getSiteFace,
} from '../../../../features/digital-life-companion/api'
import { notifyFaceUpdated } from '../../../../features/digital-life-companion/events'
import {
  generationFailureMessage,
  isGenerationTimeout,
  isPortraitInProgress,
} from '../generationError'
import { ActionBar, PrimaryButton, StepBody } from '../ui/Chrome'
import { ErrorNote } from '../ui/Feedback'
import { TextArea } from '../ui/Field'

interface Props {
  characterName: string
  busy: boolean
  onBusyChange: (busy: boolean) => void
  onHeaderChange: (chrome: OnboardingHeaderChrome) => void
  onFinished: () => void
}

export default function MasterPortraitStep({
  characterName,
  busy,
  onBusyChange,
  onHeaderChange,
  onFinished,
}: Props) {
  const { t } = useI18n()
  const o = t.life.onboarding
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const [composer, setComposer] = useState('')
  const [turns, setTurns] = useState<string[]>([])
  const [loading, setLoading] = useState(true)
  const [generating, setGenerating] = useState(false)
  const [editing, setEditing] = useState(false)
  const [error, setError] = useState('')
  const generatingRef = useRef(false)
  const claimedGenerate = useRef(false)
  const portraitUrlRef = useRef(portraitUrl)
  portraitUrlRef.current = portraitUrl

  const portraitErrors = {
    pro_unavailable: o.proUnavailable,
    visual_design_required: o.visualDesignRequired,
    visual_gender_required: o.genderRequired,
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
    portrait_edit_failed: o.portraitEditFailed,
  }

  const recoverPortrait = useCallback(
    async (previousUrl: string | null) => {
      for (const waitMs of [0, 2000, 3000, 4000, 5000]) {
        if (waitMs) {
          await new Promise((resolve) => window.setTimeout(resolve, waitMs))
        }
        try {
          const face = await getSiteFace()
          if (face.portraitUrl && face.portraitUrl !== previousUrl) {
            setPortraitUrl(face.portraitUrl)
            notifyFaceUpdated()
            return true
          }
        } catch {
          /* keep polling the public face */
        }
      }
      return false
    },
    [],
  )

  const generate = useCallback(
    async (edit = false, notes = '') => {
      if (busy || generatingRef.current) return false
      if (edit && !notes.trim()) {
        setError(o.portraitEditNeedsNotes)
        return false
      }
      generatingRef.current = true
      const previousUrl = portraitUrlRef.current
      setError('')
      setEditing(edit)
      setGenerating(true)
      onBusyChange(true)
      try {
        const result = await generateSitePortrait(
          edit ? notes.trim() : undefined,
          { edit },
        )
        if (!result.portraitUrl) {
          throw new Error(edit ? o.portraitEditFailed : o.portraitGenerateFailed)
        }
        setPortraitUrl(result.portraitUrl)
        notifyFaceUpdated()
        return true
      } catch (reason) {
        if (
          (isPortraitInProgress(reason) || isGenerationTimeout(reason)) &&
          (await recoverPortrait(previousUrl))
        ) {
          return true
        }
        setError(
          generationFailureMessage(
            reason,
            edit ? o.portraitEditFailed : o.portraitGenerateFailed,
            o.generationTimeout,
            portraitErrors,
          ),
        )
        return false
      } finally {
        generatingRef.current = false
        setGenerating(false)
        setEditing(false)
        onBusyChange(false)
      }
    },
    [
      busy,
      o.generationTimeout,
      o.imageProviderUnconfigured,
      o.imageProviderCredits,
      o.imageProviderUnauthorized,
      o.imageProviderRateLimited,
      o.imageProviderRejected,
      o.imageProviderInvalidResponse,
      o.portraitEditFailed,
      o.portraitEditNeedsNotes,
      o.portraitEmpty,
      o.portraitGenerateFailed,
      o.portraitInProgress,
      o.portraitInputsChanged,
      o.proUnavailable,
      o.visualDesignRequired,
      onBusyChange,
      recoverPortrait,
    ],
  )

  useEffect(() => {
    let cancelled = false
    void getSiteFace()
      .then((face) => {
        if (cancelled) return
        setPortraitUrl(face.portraitUrl)
      })
      .catch((reason) => {
        if (!cancelled) {
          setError(
            reason instanceof Error ? reason.message : o.portraitLoadFailed,
          )
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [o.portraitLoadFailed])

  useEffect(() => {
    if (loading || claimedGenerate.current) return
    claimedGenerate.current = true
    if (portraitUrlRef.current) return
    void generate(false)
  }, [generate, loading])

  const sendEdit = () => {
    if (blocked) return
    const note = composer.trim()
    if (!note) {
      setError(o.portraitEditNeedsNotes)
      return
    }
    void generate(true, note).then((ok) => {
      if (!ok) return
      setTurns((current) => [...current, note])
      setComposer('')
    })
  }

  const blocked = busy || generating || loading

  useLayoutEffect(() => {
    onHeaderChange({
      description:
        generating && !portraitUrl
          ? editing
            ? o.portraitEditing
            : o.portraitGenerating
          : portraitUrl
            ? o.portraitTalkLead
            : o.step5Lead,
      action: {
        label: generating
          ? editing
            ? o.portraitEditing
            : o.portraitGenerating
          : portraitUrl
            ? o.portraitRegenerate
            : o.portraitGenerate,
        busy: generating,
        disabled: blocked,
        onClick: () => void generate(false),
      },
    })
  }, [
    blocked,
    editing,
    generate,
    generating,
    o.portraitEditing,
    o.portraitGenerate,
    o.portraitGenerating,
    o.portraitRegenerate,
    o.portraitTalkLead,
    o.step5Lead,
    onHeaderChange,
    portraitUrl,
  ])

  return (
    <section className="life-ob-master" aria-label={o.step5Title}>
      <StepBody>
        <div
          className={`life-ob-master__layout${portraitUrl ? ' has-talk' : ''}`}
        >
          <div
            className={`life-ob-master__preview${portraitUrl ? '' : ' is-empty'}`}
          >
            {portraitUrl ? (
              <img src={portraitUrl} alt={characterName} decoding="async" />
            ) : loading || generating ? (
              <div className="life-ob-master__placeholder">
                <span className="life-loading__orb" aria-hidden />
                <span>
                  {generating
                    ? editing
                      ? o.portraitEditing
                      : o.portraitGenerating
                    : o.portraitLoading}
                </span>
              </div>
            ) : (
              <div className="life-ob-master__placeholder">
                <LuImage aria-hidden />
                <span>{o.portraitEmpty}</span>
              </div>
            )}
          </div>

          {portraitUrl ? (
            <div className="life-ob-talk">
              {turns.length > 0 ? (
                <ol className="life-ob-talk__thread" aria-label={o.portraitTalkLead}>
                  {turns.map((turn, index) => (
                    <li key={`${index}-${turn}`} className="life-ob-talk__turn">
                      {turn}
                    </li>
                  ))}
                </ol>
              ) : null}
              <label className="life-ob-talk__composer">
                <span className="life-ob-field__label">
                  {o.portraitRequirements}
                </span>
                <TextArea
                  value={composer}
                  rows={3}
                  maxLength={2_000}
                  disabled={blocked}
                  placeholder={o.portraitRequirementsPlaceholder}
                  onChange={(event) => setComposer(event.target.value)}
                  onKeyDown={(event) => {
                    if (
                      (event.metaKey || event.ctrlKey) &&
                      event.key === 'Enter'
                    ) {
                      event.preventDefault()
                      sendEdit()
                    }
                  }}
                />
                <button
                  type="button"
                  className="life-ghost-button life-ob-talk__send"
                  disabled={blocked || !composer.trim()}
                  onClick={sendEdit}
                >
                  {editing ? o.portraitEditing : o.portraitEdit}
                </button>
              </label>
            </div>
          ) : null}
        </div>
        {error ? <ErrorNote>{error}</ErrorNote> : null}
      </StepBody>
      <ActionBar>
        <PrimaryButton
          label={
            generating
              ? editing
                ? o.portraitEditing
                : o.portraitGenerating
              : loading
                ? o.portraitLoading
                : portraitUrl
                  ? o.portraitFinish
                  : o.portraitGenerate
          }
          busy={generating || loading}
          disabled={loading || (Boolean(portraitUrl) && generating)}
          onClick={() => {
            if (blocked) return
            if (portraitUrl) onFinished()
            else void generate(false)
          }}
        />
      </ActionBar>
    </section>
  )
}
