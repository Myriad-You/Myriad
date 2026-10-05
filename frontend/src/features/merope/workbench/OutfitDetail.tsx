import type {
  UpperBodyVisualIdentity,
  UpperBodyVisualIdentityKey,
} from '../../../components/agent/onboarding/onboardingTypes'
import type { WardrobeItem } from '../persona/wardrobe'
import { LuChevronLeft } from 'react-icons/lu'
import { Field, TextInput } from '../../../components/agent/onboarding/ui/Field'
import PortraitImportButton from '../../../components/agent/onboarding/ui/PortraitImportButton'
import VisualIdentityView from '../../../components/agent/onboarding/ui/VisualIdentityView'
import { SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { siteMediaUrl } from '../../../utils/siteMediaUrl'
import { userFacingError } from '../../../utils/userFacingError'
import {
  isDefaultWardrobeItem,
  isFullBodyItem,
  MAX_WARDROBE_NAME_CHARS,
  parseWardrobeName,
  wardrobeItemLabel,
} from '../persona/wardrobe'
import { FullBodyPanel } from './FullBodyPanel'
import { reportMeropeError } from './workbenchShared'

interface Props {
  outfit: WardrobeItem
  /** She is wearing this outfit now. */
  wearing: boolean
  /** What a full-body set is drawn from, in words. */
  reference?: string
  /** Her character with this outfit on. */
  identity: UpperBodyVisualIdentity | null
  /** The outfit's own picture, or the master portrait while she wears it. */
  picture: string | null
  generating: boolean
  canDress: boolean
  visualLabels: Record<UpperBodyVisualIdentityKey, string>
  onBack: () => void
  onRename: (id: string, rawName: string) => Promise<void>
  onWear: () => Promise<void>
  onGenerate: () => void
  onDownload: (url: string) => void
  /** A full-body set's picture or figure was redrawn, replaced or saved. */
  onFullBodyChanged?: () => void
  seeThroughTokenConfigured: boolean
  onSaveSeeThroughToken: (token: string) => Promise<void>
  onUploaded: (url: string) => Promise<void>
  onDesign: (next: UpperBodyVisualIdentity) => void
}

/** One outfit's page in the wardrobe: its picture, name, and design. */
export function OutfitDetail({
  outfit,
  wearing,
  reference,
  identity,
  picture,
  generating,
  canDress,
  visualLabels,
  onBack,
  onRename,
  onWear,
  onGenerate,
  onDownload,
  onFullBodyChanged,
  seeThroughTokenConfigured,
  onSaveSeeThroughToken,
  onUploaded,
  onDesign,
}: Props) {
  const { t } = useI18n()
  const o = t.agentPersona.onboarding
  // A full-body set is never worn; its own panel draws and splits it.
  const standing = isFullBodyItem(outfit)
  const showWear = !standing && !wearing && Boolean(picture)
  const showGenerate = !standing && (wearing || !picture)
  // A full-body set downloads its picture from its own panel.
  const showDownload = !standing && Boolean(picture)
  const hasActions = showWear || showGenerate || showDownload || wearing
  const design = identity ? (
    <VisualIdentityView
      identity={identity}
      labels={visualLabels}
      characterTitle={t.merope.visualFixedTitle}
      outfitTitle={t.merope.visualOutfitTitle}
      show="outfit"
      editLabel={o.editVisual}
      cancelLabel={o.cancelEdit}
      saveLabel={o.doneEditing}
      busy={generating}
      onIdentity={onDesign}
    />
  ) : null
  const kindRow = (
    <p className="merope-wardrobe__caption merope-wardrobe__caption--kind">
      <span className="merope-wardrobe__chip">
        {standing ? t.merope.fullBody.badge : t.merope.fullBody.kindBust}
      </span>
      {standing && wearing ? (
        <span className="merope-wardrobe__chip is-worn">
          {t.merope.wardrobeWearing}
        </span>
      ) : null}
      {standing && reference ? <span>{reference}</span> : null}
    </p>
  )
  const nameField = (
    <>
      {isDefaultWardrobeItem(outfit) ? (
        <p className="merope-wardrobe__caption">{t.merope.wardrobeDefault}</p>
      ) : (
      <Field
        label={t.merope.wardrobeName}
        optional
        optionalLabel={o.optional}
      >
        <TextInput
          key={outfit.id}
          defaultValue={outfit.name ?? ''}
          maxLength={MAX_WARDROBE_NAME_CHARS}
          disabled={generating}
          placeholder={wardrobeItemLabel(
            { clothingStyle: outfit.clothingStyle },
            o.clothingStyle,
          )}
          onBlur={(event) => {
            const next = parseWardrobeName(event.currentTarget.value)
            if ((next ?? '') === (outfit.name ?? '')) return
            void onRename(outfit.id, event.currentTarget.value).catch(
              (reason) => {
                reportMeropeError(userFacingError(reason, t.merope.wardrobeRenameFailed))
              },
            )
          }}
        />
      </Field>
      )}
    </>
  )
  return (
    <div className="merope-wardrobe-page">
      <header className="merope-wardrobe-page__head">
        <button
          type="button"
          className="section-header-back"
          onClick={onBack}
          aria-label={t.common.back}
        >
          <LuChevronLeft size={18} aria-hidden />
          <span>{t.common.back}</span>
        </button>
      </header>
      {standing ? (
        <>
          <FullBodyPanel
            outfitId={outfit.id}
            seeThroughTokenConfigured={seeThroughTokenConfigured}
            onSaveSeeThroughToken={onSaveSeeThroughToken}
            onChanged={onFullBodyChanged}
            onDownload={onDownload}
            wearing={wearing}
            onWear={onWear}
            trailing={
              <>
                {design}
                <p className="merope-wardrobe__caption">
                  {t.merope.fullBody.description}
                </p>
              </>
            }
          >
            {kindRow}
            {nameField}
          </FullBodyPanel>
        </>
      ) : (
        <>
          {picture ? (
            <div className="merope-wardrobe-page__portrait">
              <img src={siteMediaUrl(picture)} alt="" draggable={false} />
            </div>
          ) : (
            <p className="merope-wardrobe__caption">{t.merope.assetEmpty}</p>
          )}
          {kindRow}
          <div className="merope-motion-asset__make">
            {nameField}
      {hasActions ? (
      <div className="merope-motion-asset__actions">
        {showWear ? (
          <SettingsButton
            type="button"
            size="sm"
            disabled={generating || !canDress}
            loading={generating}
            onClick={() => {
              void onWear().catch((reason) => {
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
            disabled={generating || !canDress}
            loading={generating}
            onClick={onGenerate}
          >
            {generating
              ? t.merope.visualGenerating
              : picture
                ? t.merope.visualRegenerate
                : t.merope.visualGenerate}
          </SettingsButton>
        ) : null}
        {showDownload && picture ? (
          <SettingsButton
            type="button"
            size="sm"
            variant="secondary"
            disabled={generating}
            onClick={() => onDownload(picture)}
          >
            {t.merope.visualDownload}
          </SettingsButton>
        ) : null}
        {wearing ? (
          <PortraitImportButton
            appearance="settings"
            disabled={generating}
            onError={reportMeropeError}
            onUploaded={onUploaded}
          />
        ) : null}
      </div>
      ) : null}
      {design}
          </div>
        </>
      )}
    </div>
  )
}
