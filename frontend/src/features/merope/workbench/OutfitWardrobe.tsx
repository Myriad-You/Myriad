import type {
  ClothingStyle,
  PersonaGender,
  UpperBodyVisualIdentity,
} from '../../../components/agent/onboarding/onboardingTypes'
import type { WardrobeItem } from '../persona/wardrobe'
import { useState } from 'react'
import { generationFailureMessage } from '../../../components/agent/onboarding/generationError'
import {
  CLOTHING_STYLE_OPTIONS,
  clothingStylePreview,
  parseUpperBodyVisualIdentity,
  VISUAL_NOTES_LIMIT,
} from '../../../components/agent/onboarding/onboardingTypes'
import { Field, TextArea, TextInput } from '../../../components/agent/onboarding/ui/Field'
import { SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { siteMediaUrl } from '../../../utils/siteMediaUrl'
import { showStickyToast } from '../../../utils/toastManager'
import {
  applyOutfit,
  isDefaultWardrobeItem,
  isFullBodyItem,
  MAX_WARDROBE_ITEMS,
  MAX_WARDROBE_NAME_CHARS,
  newWardrobeId,
  parseWardrobeName,
  sortWardrobe,
  wardrobeItemLabel,
} from '../persona/wardrobe'
import '../../../components/agent/PersonaOnboarding.css'

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
  const [fullBody, setFullBody] = useState(false)
  const [referenceId, setReferenceId] = useState<string | null>(null)
  const [itemName, setItemName] = useState('')
  const [requirements, setRequirements] = useState('')
  const [generating, setGenerating] = useState(false)
  const blocked = busy || generating
  const full = items.length >= MAX_WARDROBE_ITEMS
  const rack = sortWardrobe(items)
  // A full-body set can be redrawn from any bust set with a picture.
  const references = rack.filter(
    (item) =>
      !isFullBodyItem(item) &&
      (item.portraitAssetId || (item.id === activeId && portraitUrl)),
  )
  const reference = fullBody
    ? (references.find((item) => item.id === referenceId) ?? null)
    : null
  const canGenerate =
    Boolean(identity && gender && (reference || style)) && !full
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
    void onManage(item).catch((reason) => {
      showStickyToast({
        message: generationFailureMessage(
          reason,
          labels.wardrobeApplyFailed,
          o.generationTimeout,
        ),
        type: 'error',
        replaceKey: 'merope-wardrobe',
      })
    })
  }

  const removeItem = (id: string) => {
    void onDelete(id).catch((reason) => {
      showStickyToast({
        message: generationFailureMessage(
          reason,
          labels.wardrobeDeleteFailed,
          o.generationTimeout,
        ),
        type: 'error',
        replaceKey: 'merope-wardrobe',
      })
    })
  }

  const resetDrawer = () => {
    setComposing(false)
    setStyle(null)
    setFullBody(false)
    setReferenceId(null)
    setItemName('')
    setRequirements('')
  }

  const generate = async () => {
    if (!identity || !gender || blocked || full) return
    const name = parseWardrobeName(itemName)
    if (reference) {
      // Redrawn from that bust set, so it wears that set's design and, unless
      // renamed, its name.
      const named = name ?? parseWardrobeName(labelOf(reference))
      const item: WardrobeItem = {
        id: newWardrobeId(),
        clothingStyle: reference.clothingStyle,
        outfit: reference.outfit,
        profile: 'fullBody',
        referenceOutfitId: reference.id,
        ...(named ? { name: named } : {}),
      }
      setGenerating(true)
      try {
        await onCreated(item, applyOutfit(identity, item))
        resetDrawer()
      } catch (reason) {
        showStickyToast({
          message: generationFailureMessage(
            reason,
            o.visualDesignFailed,
            o.generationTimeout,
          ),
          type: 'error',
          replaceKey: 'merope-wardrobe',
        })
      } finally {
        setGenerating(false)
      }
      return
    }
    if (!style) return
    setGenerating(true)
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
      const item: WardrobeItem = {
        id: newWardrobeId(),
        clothingStyle: style,
        outfit: generated.outfit,
        ...(fullBody ? { profile: 'fullBody' as const } : {}),
        ...(name ? { name } : {}),
      }
      await onCreated(item, nextIdentity)
      resetDrawer()
    } catch (reason) {
      showStickyToast({
        message: generationFailureMessage(
          reason,
          o.visualDesignFailed,
          o.generationTimeout,
          {
            visual_identity_invalid: o.visualDesignFailed,
            portrait_generation_failed: o.portraitGenerateFailed,
          },
        ),
        type: 'error',
        replaceKey: 'merope-wardrobe',
      })
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
                      <img src={siteMediaUrl(currentPicture)} alt="" draggable={false} />
                    ) : (
                      <span className="merope-wardrobe__fold">
                        {labelOf(current)}
                      </span>
                    )}
                  </button>
                ) : currentPicture ? (
                  <div className="merope-wardrobe__garment">
                    <img src={siteMediaUrl(currentPicture)} alt="" draggable={false} />
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
              const standing = isFullBodyItem(item)
              return (
                <div key={item.id} className="merope-wardrobe__set" role="listitem">
                  <button
                    type="button"
                    className={`merope-wardrobe__garment${picture ? '' : ' merope-wardrobe__garment--fold'}${standing ? ' merope-wardrobe__garment--full-body' : ''}`}
                    disabled={blocked}
                    aria-pressed={false}
                    aria-label={labelOf(item)}
                    onClick={() => manageItem(item)}
                  >
                    {picture ? (
                      <img src={siteMediaUrl(picture)} alt="" draggable={false} />
                    ) : (
                      <span className="merope-wardrobe__fold">
                        {labelOf(item)}
                      </span>
                    )}
                  </button>
                  <div className="merope-wardrobe__tag">
                    <span>{labelOf(item)}</span>
                    {standing ? (
                      <i className="merope-wardrobe__kind">
                        {labels.fullBody.badge}
                      </i>
                    ) : null}
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
          <p className="merope-wardrobe__drawer-label">{labels.fullBody.kind}</p>
          <div
            className="merope-wardrobe__kinds"
            role="radiogroup"
            aria-label={labels.fullBody.kind}
          >
            {[false, true].map((standing) => (
              <button
                key={String(standing)}
                type="button"
                role="radio"
                aria-checked={fullBody === standing}
                className={`merope-wardrobe__family${fullBody === standing ? ' is-on' : ''}`}
                disabled={blocked}
                onClick={() => {
                  setFullBody(standing)
                  setReferenceId(null)
                }}
              >
                <span>
                  {standing
                    ? labels.fullBody.kindFullBody
                    : labels.fullBody.kindBust}
                </span>
              </button>
            ))}
          </div>
          {fullBody ? (
            <>
              <p className="merope-wardrobe__drawer-label">
                {labels.fullBody.reference}
              </p>
              <div
                className="merope-wardrobe__families"
                role="radiogroup"
                aria-label={labels.fullBody.reference}
              >
                <button
                  type="button"
                  role="radio"
                  aria-checked={!reference}
                  className={`merope-wardrobe__family merope-wardrobe__family--plain${reference ? '' : ' is-on'}`}
                  disabled={blocked}
                  onClick={() => setReferenceId(null)}
                >
                  <span>{labels.fullBody.referenceNone}</span>
                </button>
                {references.map((item) => {
                  const picture =
                    item.portraitAssetId ||
                    (item.id === activeId ? portraitUrl : null)
                  const selected = reference?.id === item.id
                  return (
                    <button
                      key={item.id}
                      type="button"
                      role="radio"
                      aria-checked={selected}
                      className={`merope-wardrobe__family${selected ? ' is-on' : ''}`}
                      disabled={blocked}
                      onClick={() => setReferenceId(item.id)}
                    >
                      {picture ? (
                        <img src={siteMediaUrl(picture)} alt="" draggable={false} />
                      ) : null}
                      <span>{labelOf(item)}</span>
                    </button>
                  )
                })}
              </div>
              <p className="merope-motion-home__help">
                {labels.fullBody.referenceHint}
              </p>
            </>
          ) : null}
          {reference ? null : (
          <>
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
          </>
          )}
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
                reference
                  ? labelOf(reference)
                  : style
                    ? styleNames[style]
                    : labels.wardrobeNamePlaceholder
              }
              onChange={(event) => setItemName(event.target.value)}
            />
          </Field>
          {reference ? null : (
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
          )}
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
              onClick={resetDrawer}
            >
              {t.common.cancel}
            </SettingsButton>
          </div>
        </div>
      ) : full ? (
        <p className="merope-motion-home__help">{labels.wardrobeFull}</p>
      ) : null}
    </section>
  )
}
