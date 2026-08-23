import type {
  ClothingStyle,
  LifeGender,
  OnboardingHeaderChrome,
  UpperBodyVisualIdentity,
  UpperBodyVisualIdentityKey,
} from '../onboardingTypes'
import { LuCheck, LuEdit3, LuX } from '@lib/icons'
import { useCallback, useLayoutEffect, useRef, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { agentService } from '../../../../services/agent'
import { generationFailureMessage } from '../generationError'
import {
  CHARACTER_VISUAL_KEYS,
  CLOTHING_STYLE_OPTIONS,
  clothingStylePreview,
  OUTFIT_VISUAL_KEYS,
  parseUpperBodyVisualIdentity,
  UPPER_BODY_VISUAL_IDENTITY_LIMITS,
  visualField,
  withVisualField,
} from '../onboardingTypes'
import { ActionBar, PrimaryButton, StepBody } from '../ui/Chrome'
import { ErrorNote } from '../ui/Feedback'
import { Field, FieldGroup, TextArea } from '../ui/Field'

type VisualPhase = 'setup' | 'draft'

interface Props {
  identity: UpperBodyVisualIdentity | null
  gender: LifeGender | null
  language: string
  clothingStyle: ClothingStyle | null
  requirements: string
  busy: boolean
  onClothingStyle: (style: ClothingStyle) => void
  onIdentity: (identity: UpperBodyVisualIdentity | null) => void
  onRequirements: (requirements: string) => void
  onBusyChange: (busy: boolean) => void
  onHeaderChange: (chrome: OnboardingHeaderChrome) => void
  onConfirm: () => Promise<void>
}

export default function CharacterVisualDesignStep({
  identity,
  gender,
  language,
  clothingStyle,
  requirements,
  busy,
  onClothingStyle,
  onIdentity,
  onRequirements,
  onBusyChange,
  onHeaderChange,
  onConfirm,
}: Props) {
  const { t } = useI18n()
  const o = t.life.onboarding
  const [phase, setPhase] = useState<VisualPhase>(() =>
    identity ? 'draft' : 'setup',
  )
  const [generating, setGenerating] = useState(false)
  const [error, setError] = useState('')
  const [editingField, setEditingField] =
    useState<UpperBodyVisualIdentityKey | null>(null)
  const [draft, setDraft] = useState('')
  const generatingRef = useRef(false)
  const identityRef = useRef(identity)
  identityRef.current = identity

  const labels: Record<UpperBodyVisualIdentityKey, string> = {
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

  const generate = useCallback(
    async (
      regenerate: boolean,
      style = clothingStyle,
      keepCharacter = false,
    ) => {
      if (busy || generatingRef.current) return false
      if (!style) {
        setError(o.clothingStyleRequired)
        return false
      }
      if (!gender) {
        setError(o.genderRequired)
        return false
      }
      generatingRef.current = true
      setError('')
      setGenerating(true)
      onBusyChange(true)
      try {
        const response = await agentService.suggestPersonaVisualDesign({
          gender,
          language,
          clothingStyle: style,
          visualRequirements: requirements.trim() || undefined,
          keepCharacter,
          regenerate,
          existingVisualIdentity:
            (keepCharacter || regenerate) && identityRef.current
              ? identityRef.current
              : undefined,
        })
        const generated = parseUpperBodyVisualIdentity(response.visualIdentity)
        if (!generated) throw new Error(o.visualDesignFailed)
        onIdentity(generated)
        setPhase('draft')
        return true
      } catch (reason) {
        setError(
          generationFailureMessage(
            reason,
            o.visualDesignFailed,
            o.generationTimeout,
            {
              pro_unavailable: o.proUnavailable,
              visual_design_language: o.visualDesignLanguageFailed,
              visual_language_required: o.visualDesignLanguageFailed,
              visual_design_failed: o.visualDesignFailed,
              visual_design_unusable: o.visualDesignUnusable,
              visual_identity_invalid: o.visualDesignFailed,
              clothing_style_required: o.clothingStyleRequired,
              gender_required: o.genderRequired,
            },
          ),
        )
        return false
      } finally {
        generatingRef.current = false
        setGenerating(false)
        onBusyChange(false)
      }
    },
    [
      busy,
      clothingStyle,
      o.clothingStyleRequired,
      o.genderRequired,
      gender,
      language,
      o.generationTimeout,
      o.proUnavailable,
      o.visualDesignFailed,
      o.visualDesignLanguageFailed,
      onBusyChange,
      onIdentity,
      requirements,
    ],
  )

  const pickStyle = (style: ClothingStyle) => {
    if (blocked) return
    if (style === clothingStyle) return
    onClothingStyle(style)
  }

  const startEdit = (key: UpperBodyVisualIdentityKey) => {
    if (!identity) return
    setEditingField(key)
    setDraft(visualField(identity, key))
  }
  const cancelEdit = () => {
    setEditingField(null)
    setDraft('')
  }
  const commitEdit = () => {
    if (!identity || !editingField) return
    const next = draft.trim()
    if (!next) {
      cancelEdit()
      return
    }
    onIdentity(withVisualField(identity, editingField, next))
    setEditingField(null)
    setDraft('')
  }

  const blocked = busy || generating || editingField !== null

  useLayoutEffect(() => {
    onHeaderChange({
      description:
        generating && !identity
          ? o.visualDesignGenerating
          : phase === 'setup'
            ? o.visualStyleAsk
            : o.visualDesignPreview,
      action:
        phase === 'draft'
          ? {
              label: generating
                ? o.visualDesignGenerating
                : o.visualDesignRegenerate,
              busy: generating,
              disabled: blocked || !clothingStyle || !gender,
              onClick: () => void generate(true),
            }
          : undefined,
      onBack:
        phase === 'draft'
          ? () => {
              if (blocked) return true
              setError('')
              setPhase('setup')
              return true
            }
          : undefined,
    })
  }, [
    blocked,
    clothingStyle,
    generate,
    generating,
    identity,
    o.visualDesignGenerating,
    o.visualDesignPreview,
    o.visualDesignRegenerate,
    o.visualStyleAsk,
    onHeaderChange,
    phase,
  ])

  return (
    <section className="life-ob-visual" aria-label={o.step4Title}>
      <StepBody>
        {phase === 'setup' ? (
          <>
            <FieldGroup label={o.clothingStyleLabel}>
              <div
                className="life-ob-styles"
                role="radiogroup"
                aria-label={o.clothingStyleLabel}
              >
                {CLOTHING_STYLE_OPTIONS.map((style) => {
                  const selected = clothingStyle === style
                  return (
                    <button
                      key={style}
                      type="button"
                      role="radio"
                      aria-checked={selected}
                      className={`life-ob-styles__card${selected ? ' is-on' : ''}`}
                      disabled={blocked}
                      onClick={() => pickStyle(style)}
                    >
                      <img
                        src={clothingStylePreview(style)}
                        alt=""
                        draggable={false}
                      />
                      <span>{o.clothingStyle[style]}</span>
                    </button>
                  )
                })}
              </div>
            </FieldGroup>
            <Field
              label={o.visualRequirements}
              optional
              optionalLabel={o.optional}
            >
              <TextArea
                value={requirements}
                rows={2}
                maxLength={500}
                disabled={blocked}
                placeholder={o.visualRequirementsPlaceholder}
                onChange={(event) => {
                  onRequirements(event.target.value)
                }}
              />
            </Field>
          </>
        ) : identity ? (
          <div className="life-ob-persona-groups">
            {(
              [
                [o.visualGroupCharacter, CHARACTER_VISUAL_KEYS],
                [o.visualGroupOutfit, OUTFIT_VISUAL_KEYS],
              ] as const
            ).map(([title, keys]) => (
              <section
                key={title}
                className="life-ob-persona-group"
                aria-label={title}
              >
                <h2 className="life-ob-persona-group__title">{title}</h2>
                <dl className="life-ob-persona-view">
                  {keys.map((key) => {
                    const isEditing = editingField === key
                    return (
                      <div
                        key={key}
                        className={`life-ob-persona-view__row${isEditing ? ' is-editing' : ''}`}
                      >
                        {isEditing ? (
                          <div className="life-ob-persona-view__editor">
                            <dt>{labels[key]}</dt>
                            <TextArea
                              rows={4}
                              maxLength={UPPER_BODY_VISUAL_IDENTITY_LIMITS[key]}
                              value={draft}
                              autoFocus
                              onChange={(event) => setDraft(event.target.value)}
                              onKeyDown={(event) => {
                                if (
                                  (event.metaKey || event.ctrlKey) &&
                                  event.key === 'Enter'
                                ) {
                                  event.preventDefault()
                                  commitEdit()
                                }
                                if (event.key === 'Escape') {
                                  event.preventDefault()
                                  cancelEdit()
                                }
                              }}
                            />
                            <div className="life-ob-persona-view__actions">
                              <button
                                type="button"
                                className="life-ob-persona-view__action is-cancel"
                                disabled={busy}
                                title={o.cancelEdit}
                                aria-label={o.cancelEdit}
                                onClick={cancelEdit}
                              >
                                <LuX aria-hidden />
                              </button>
                              <button
                                type="button"
                                className="life-ob-persona-view__action is-save"
                                disabled={busy}
                                title={o.doneEditing}
                                aria-label={o.doneEditing}
                                onClick={commitEdit}
                              >
                                <LuCheck aria-hidden />
                              </button>
                            </div>
                          </div>
                        ) : (
                          <>
                            <div className="life-ob-persona-view__copy">
                              <dt>{labels[key]}</dt>
                              <dd>{visualField(identity, key)}</dd>
                            </div>
                            <button
                              type="button"
                              className="life-ob-persona-view__edit"
                              disabled={busy || generating}
                              title={o.editVisual}
                              aria-label={`${o.editVisual} · ${labels[key]}`}
                              onClick={() => startEdit(key)}
                            >
                              <LuEdit3 aria-hidden />
                            </button>
                          </>
                        )}
                      </div>
                    )
                  })}
                </dl>
              </section>
            ))}
          </div>
        ) : generating ? (
          <div className="life-ob-visual__pending">
            <span className="life-loading__orb" aria-hidden />
            <span>{o.visualDesignGenerating}</span>
          </div>
        ) : null}
        {error ? <ErrorNote>{error}</ErrorNote> : null}
      </StepBody>
      <ActionBar>
        <PrimaryButton
          label={
            generating
              ? o.visualDesignGenerating
              : phase === 'setup'
                ? o.visualDesignGenerate
                : busy
                  ? o.saving
                  : o.next
          }
          busy={generating || (busy && phase === 'draft')}
          disabled={
            editingField !== null ||
            !clothingStyle ||
            !gender ||
            (phase === 'draft' && (!identity || generating))
          }
          onClick={() => {
            if (blocked) return
            if (phase === 'setup' || !identity) {
              setPhase('draft')
              void generate(
                false,
                clothingStyle,
                Boolean(identityRef.current),
              ).then((ok) => {
                if (!ok) setPhase('setup')
              })
              return
            }
            setError('')
            void onConfirm().catch((reason) => {
              setError(
                generationFailureMessage(
                  reason,
                  o.visualDesignSaveFailed,
                  o.generationTimeout,
                  {
                    persona_contract_invalid: o.saveFailed,
                    visual_profile_invalid: o.visualDesignSaveFailed,
                  },
                ),
              )
            })
          }}
        />
      </ActionBar>
    </section>
  )
}
