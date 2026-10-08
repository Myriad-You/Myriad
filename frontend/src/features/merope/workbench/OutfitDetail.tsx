import type {
  UpperBodyVisualIdentity,
  UpperBodyVisualIdentityKey,
} from '../../../components/agent/onboarding/onboardingTypes'
import type { WardrobeItem } from '../persona/wardrobe'
import { useRef, useState } from 'react'
import { LuChevronLeft, LuDownload, LuPencil } from 'react-icons/lu'
import PortraitImportButton from '../../../components/agent/onboarding/ui/PortraitImportButton'
import VisualIdentityView from '../../../components/agent/onboarding/ui/VisualIdentityView'
import { SettingsButton, SettingTitleTag } from '../../../components/settings'
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
  const [actionsHost, setActionsHost] = useState<HTMLDivElement | null>(null)
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
  // The name is the page's title; a named outfit renames in place.
  const renameable = !isDefaultWardrobeItem(outfit)
  const fallbackName = wardrobeItemLabel({ clothingStyle: outfit.clothingStyle }, o.clothingStyle)
  const title = renameable ? outfit.name?.trim() || fallbackName : t.merope.wardrobeDefault
  const [renaming, setRenaming] = useState(false)
  const cancelRename = useRef(false)
  const commitRename = (raw: string) => {
    setRenaming(false)
    if (cancelRename.current) {
      cancelRename.current = false
      return
    }
    if ((parseWardrobeName(raw) ?? '') === (outfit.name ?? '')) return
    void onRename(outfit.id, raw).catch((reason) => {
      reportMeropeError(userFacingError(reason, t.merope.wardrobeRenameFailed))
    })
  }
  const titleNode = renaming ? (
    <input
      key={outfit.id}
      className="merope-wardrobe-page__name-input"
      // Renaming starts from a click on the title: the field takes the focus it asked for.
      autoFocus
      defaultValue={outfit.name ?? ''}
      maxLength={MAX_WARDROBE_NAME_CHARS}
      placeholder={fallbackName}
      aria-label={t.merope.wardrobeName}
      onBlur={(event) => commitRename(event.currentTarget.value)}
      onKeyDown={(event) => {
        if (event.key === 'Enter') event.currentTarget.blur()
        if (event.key === 'Escape') {
          cancelRename.current = true
          event.currentTarget.blur()
        }
      }}
    />
  ) : renameable ? (
    <button
      type="button"
      className="merope-wardrobe-page__name"
      disabled={generating}
      title={t.merope.wardrobeName}
      onClick={() => setRenaming(true)}
    >
      <span>{title}</span>
      <LuPencil size={13} aria-hidden />
    </button>
  ) : (
    <span>{title}</span>
  )
  return (
    <div className="merope-wardrobe-page">
      <header className="section-header merope-wardrobe-page__head">
        <div className="section-header-left">
          <div className="section-header-leading">
            <button
              type="button"
              className="section-header-back"
              onClick={onBack}
              aria-label={t.common.back}
            >
              <LuChevronLeft size={18} aria-hidden />
              <span>{t.common.back}</span>
            </button>
          </div>
          <div className="section-header-text">
            <h2 className="section-title">
              {titleNode}
              <span className="section-title-extra">
                <SettingTitleTag variant="muted">
                  {standing ? t.merope.fullBody.badge : t.merope.fullBody.kindBust}
                </SettingTitleTag>
                {wearing ? <SettingTitleTag>{t.merope.wardrobeWearing}</SettingTitleTag> : null}
              </span>
            </h2>
            {standing && reference ? <div className="section-description">{reference}</div> : null}
          </div>
        </div>
        {/* The picture's actions: the bust's here, the full body's sent here by its panel. */}
        <div ref={setActionsHost} className="section-header-right merope-wardrobe-page__actions">
          {!standing && hasActions ? (
            <>
              {showWear ? (
                <SettingsButton
                  type="button"
                  size="sm"
                  variant="primary"
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
              {wearing ? (
                <PortraitImportButton
                  appearance="settings"
                  disabled={generating}
                  onError={reportMeropeError}
                  onUploaded={onUploaded}
                />
              ) : null}
              {showDownload && picture ? (
                <SettingsButton
                  type="button"
                  size="sm"
                  variant="icon"
                  icon={<LuDownload size={16} />}
                  aria-label={t.merope.visualDownload}
                  title={t.merope.visualDownload}
                  disabled={generating}
                  onClick={() => onDownload(picture)}
                />
              ) : null}
            </>
          ) : null}
        </div>
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
            actionsHost={actionsHost}
          />
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
          {design ? <div className="merope-motion-asset__make">{design}</div> : null}
        </>
      )}
    </div>
  )
}
