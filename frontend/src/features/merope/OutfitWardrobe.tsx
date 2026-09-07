import type {
  ClothingStyle,
  PersonaGender,
  UpperBodyVisualIdentity,
} from '../../components/agent/onboarding/onboardingTypes'
import type { WardrobeItem } from './wardrobe'
import { useState } from 'react'
import { generationFailureMessage } from '../../components/agent/onboarding/generationError'
import {
  CLOTHING_STYLE_OPTIONS,
  clothingStylePreview,
  parseUpperBodyVisualIdentity,
  VISUAL_NOTES_LIMIT,
} from '../../components/agent/onboarding/onboardingTypes'
import { Field, TextArea, TextInput } from '../../components/agent/onboarding/ui/Field'
import { SettingsButton } from '../../components/settings'
import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import {
  applyOutfit,
  isDefaultWardrobeItem,
  MAX_WARDROBE_ITEMS,
  MAX_WARDROBE_NAME_CHARS,
  newWardrobeId,
  parseWardrobeName,
  sortWardrobe,
  wardrobeItemLabel,
} from './wardrobe'
import '../../components/agent/PersonaOnboarding.css'

interface Props {
  identity: UpperBodyVisualIdentity | null
  gender: PersonaGender | null
  language: string
  items: WardrobeItem[]
  activeId: string | null
  portraitUrl?: string | null
  busy: boolean
  filling?: boolean
  hasPortrait?: boolean
  onFillFromPortrait?: () => void
  onManage: (item: WardrobeItem) => Promise<void>
  onDelete: (id: string) => Promise<void>
  onCreated: (item: WardrobeItem, identity: UpperBodyVisualIdentity) => Promise<void>
}

export default function OutfitWardrobe({
  identity,
  gender,
  language,
  items,
  activeId,
  portraitUrl = null,
  busy,
  filling = false,
  hasPortrait = false,
  onFillFromPortrait,
  onManage,
  onDelete,
  onCreated,
}: Props) {
  const { t } = useI18n()
  const labels = t.merope
  const styleNames = t.agentPersona.onboarding.clothingStyle
  const o = t.agentPersona.onboarding
  const [composing, setComposing] = useState(false)
  const [style, setStyle] = useState<ClothingStyle | null>(null)
  const [itemName, setItemName] = useState('')
  const [requirements, setRequirements] = useState('')
  const [generating, setGenerating] = useState(false)
  const [error, setError] = useState('')
  const blocked = busy || generating
  const full = items.length >= MAX_WARDROBE_ITEMS
  const canGenerate = Boolean(identity && gender && style) && !full
  const rack = sortWardrobe(items)
  const current = rack.find((item) => item.id === activeId) ?? rack[0] ?? null
  const others = current
    ? rack.filter((item) => item.id !== current.id)
    : rack
  const currentPicture = current
    ? current.portraitAssetId ||
      (isDefaultWardrobeItem(current) ? portraitUrl : null)
    : portraitUrl
  const canCompose = Boolean(identity && gender) && !composing && !full
  const labelOf = (item: WardrobeItem) =>
    wardrobeItemLabel(item, styleNames, labels.wardrobeDefault)

  const manageItem = (item: WardrobeItem) => {
    setError('')
    void onManage(item).catch((reason) => {
      setError(
        generationFailureMessage(
          reason,
          labels.wardrobeApplyFailed,
          o.generationTimeout,
        ),
      )
    })
  }

  const removeItem = (id: string) => {
    setError('')
    void onDelete(id).catch((reason) => {
      setError(
        generationFailureMessage(
          reason,
          labels.wardrobeDeleteFailed,
          o.generationTimeout,
        ),
      )
    })
  }

  const generate = async () => {
    if (!identity || !gender || !style || blocked || full) return
    setGenerating(true)
    setError('')
    try {
      const response = await agentService.suggestPersonaVisualDesign({
        gender,
        language,
        clothingStyle: style,
        visualRequirements: requirements.trim() || undefined,
        keepCharacter: true,
        existingVisualIdentity: identity,
      })
      const generated = parseUpperBodyVisualIdentity(response.visualIdentity)
      if (!generated) throw new Error(o.visualDesignFailed)
      const nextIdentity = applyOutfit(identity, {
        id: 'next',
        clothingStyle: style,
        outfit: generated.outfit,
      })
      const name = parseWardrobeName(itemName)
      const item: WardrobeItem = {
        id: newWardrobeId(),
        clothingStyle: style,
        outfit: generated.outfit,
        ...(name ? { name } : {}),
      }
      await onCreated(item, nextIdentity)
      setComposing(false)
      setStyle(null)
      setItemName('')
      setRequirements('')
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
            visual_design_unusable: o.visualDesignUnusable,
            visual_identity_invalid: o.visualDesignFailed,
            clothing_style_required: o.clothingStyleRequired,
            gender_required: o.genderRequired,
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
            visual_design_required: o.visualDesignRequired,
            visual_gender_mismatch: o.visualGenderMismatch,
            visual_identity_unusable: o.visualIdentityUnusableForPortrait,
          },
        ),
      )
    } finally {
      setGenerating(false)
    }
  }

  return (
    <section className="merope-wardrobe" aria-label={labels.wardrobeTitle}>
      {current || currentPicture || others.length > 0 || canCompose ? (
        <div className="merope-wardrobe__case">
          <div
            className="merope-wardrobe__rack"
            role="list"
            aria-label={labels.wardrobeTitle}
          >
            {current || currentPicture ? (
              <div className="merope-wardrobe__set is-on" role="listitem">
                {current ? (
                  <button
                    type="button"
                    className={`merope-wardrobe__garment${currentPicture ? '' : ' merope-wardrobe__garment--fold'}`}
                    disabled={blocked}
                    aria-pressed
                    aria-label={labelOf(current)}
                    onClick={() => manageItem(current)}
                  >
                    {currentPicture ? (
                      <img src={currentPicture} alt="" draggable={false} />
                    ) : (
                      <span className="merope-wardrobe__fold">
                        {labelOf(current)}
                      </span>
                    )}
                  </button>
                ) : currentPicture ? (
                  <div className="merope-wardrobe__garment">
                    <img src={currentPicture} alt="" draggable={false} />
                  </div>
                ) : null}
                {current ? (
                  <div className="merope-wardrobe__tag">
                    <span>{labelOf(current)}</span>
                    <i className="merope-wardrobe__wearing">
                      {labels.wardrobeWearing}
                    </i>
                    {others.length > 0 && !isDefaultWardrobeItem(current) ? (
                      <button
                        type="button"
                        className="merope-wardrobe__remove"
                        disabled={blocked}
                        aria-label={t.common.delete}
                        onClick={() => removeItem(current.id)}
                      >
                        {t.common.delete}
                      </button>
                    ) : null}
                  </div>
                ) : null}
              </div>
            ) : null}
            {others.map((item) => {
              const picture = item.portraitAssetId || null
              return (
                <div key={item.id} className="merope-wardrobe__set" role="listitem">
                  <button
                    type="button"
                    className={`merope-wardrobe__garment${picture ? '' : ' merope-wardrobe__garment--fold'}`}
                    disabled={blocked}
                    aria-pressed={false}
                    aria-label={labelOf(item)}
                    onClick={() => manageItem(item)}
                  >
                    {picture ? (
                      <img src={picture} alt="" draggable={false} />
                    ) : (
                      <span className="merope-wardrobe__fold">
                        {labelOf(item)}
                      </span>
                    )}
                  </button>
                  <div className="merope-wardrobe__tag">
                    <span>{labelOf(item)}</span>
                    {isDefaultWardrobeItem(item) ? null : (
                      <button
                        type="button"
                        className="merope-wardrobe__remove"
                        disabled={blocked}
                        aria-label={t.common.delete}
                        onClick={() => removeItem(item.id)}
                      >
                        {t.common.delete}
                      </button>
                    )}
                  </div>
                </div>
              )
            })}
            {canCompose ? (
              <div className="merope-wardrobe__set is-add" role="listitem">
                <button
                  type="button"
                  className="merope-wardrobe__garment merope-wardrobe__garment--empty"
                  disabled={blocked}
                  onClick={() => {
                    setError('')
                    setComposing(true)
                  }}
                >
                  <span className="merope-wardrobe__add-mark" aria-hidden>
                    +
                  </span>
                  <span>{labels.wardrobeNew}</span>
                </button>
              </div>
            ) : null}
          </div>
        </div>
      ) : null}
      {filling ? (
        <p className="merope-wardrobe__caption">{labels.wardrobeReading}</p>
      ) : !identity && hasPortrait && onFillFromPortrait ? (
        <div className="merope-wardrobe__caption-row">
          <p className="merope-wardrobe__caption">{labels.wardrobeNeedCharacter}</p>
          <button
            type="button"
            className="merope-wardrobe__fill"
            disabled={blocked}
            onClick={() => onFillFromPortrait()}
          >
            {labels.wardrobeFillFromPortrait}
          </button>
        </div>
      ) : !identity ? (
        <p className="merope-wardrobe__caption">{labels.wardrobeNeedCharacter}</p>
      ) : rack.length === 0 && !composing ? (
        <p className="merope-wardrobe__caption">{labels.wardrobeEmpty}</p>
      ) : null}
      {composing ? (
        <div className="merope-wardrobe__drawer">
          <p className="merope-wardrobe__drawer-label">{o.clothingStyleLabel}</p>
          <div
            className="merope-wardrobe__families"
            role="radiogroup"
            aria-label={o.clothingStyleLabel}
          >
            {CLOTHING_STYLE_OPTIONS.map((option) => {
              const selected = style === option
              return (
                <button
                  key={option}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  className={`merope-wardrobe__family${selected ? ' is-on' : ''}`}
                  disabled={blocked}
                  onClick={() => setStyle(option)}
                >
                  <img
                    src={clothingStylePreview(option)}
                    alt=""
                    draggable={false}
                  />
                  <span>{styleNames[option]}</span>
                </button>
              )
            })}
          </div>
          <Field
            label={labels.wardrobeName}
            optional
            optionalLabel={o.optional}
            value={itemName}
            max={MAX_WARDROBE_NAME_CHARS}
          >
            <TextInput
              value={itemName}
              maxLength={MAX_WARDROBE_NAME_CHARS}
              disabled={blocked}
              placeholder={
                style
                  ? styleNames[style]
                  : labels.wardrobeNamePlaceholder
              }
              onChange={(event) => setItemName(event.target.value)}
            />
          </Field>
          <Field
            label={labels.wardrobeRequirements}
            optional
            optionalLabel={o.optional}
            hint={labels.wardrobeRequirementsHint}
            value={requirements}
            max={VISUAL_NOTES_LIMIT}
          >
            <TextArea
              value={requirements}
              rows={2}
              maxLength={VISUAL_NOTES_LIMIT}
              disabled={blocked}
              placeholder={labels.wardrobeRequirementsPlaceholder}
              onChange={(event) => setRequirements(event.target.value)}
            />
          </Field>
          <div className="merope-wardrobe__actions">
            <SettingsButton
              type="button"
              size="sm"
              disabled={!canGenerate}
              loading={generating}
              onClick={() => void generate()}
            >
              {generating ? o.visualDesignGenerating : labels.wardrobeGenerate}
            </SettingsButton>
            <SettingsButton
              type="button"
              size="sm"
              variant="secondary"
              disabled={generating}
              onClick={() => {
                setComposing(false)
                setItemName('')
                setError('')
              }}
            >
              {t.common.cancel}
            </SettingsButton>
          </div>
        </div>
      ) : full ? (
        <p className="merope-motion-home__help">{labels.wardrobeFull}</p>
      ) : null}
      {error ? (
        <p className="merope-motion-home__help" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  )
}
