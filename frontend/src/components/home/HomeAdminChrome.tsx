import type { ReactNode } from 'react'
import type { HomeDashboardLayouts, HomeLayoutMode } from '../../utils/homeLayout'
import type { HomeLayoutImportPayload } from './HomeLayoutTransfer'
import { LuSparkles } from '@lib/chromeStrokeIcons'
import { FaCog, FaCompress, FaEdit, FaExpand } from '@lib/faChromeIcons'
import { motionShim as motion } from '@lib/motionShim'
import { lazy, Suspense } from 'react'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'

const TitleFontSelector = lazy(() => import('../TitleFontSelector'))
const HomeLayoutTransferButtons = lazy(() =>
  import('./HomeLayoutTransfer').then((m) => ({
    default: m.HomeLayoutTransferButtons,
  })),
)

function HomeStatusBarSlot({
  open,
  side,
  children,
}: {
  open: boolean
  side: 'before' | 'after'
  children: ReactNode
}) {
  return (
    <div
      className={`home-status-bar__slot${open ? ' is-open' : ''}`}
      data-side={side}
      inert={!open ? true : undefined}
      aria-hidden={!open || undefined}
    >
      <div className="home-status-bar__slot-inner">
        <div className="home-status-bar__tools">{children}</div>
      </div>
    </div>
  )
}

export interface HomeAdminChromeProps {
  isEditMode: boolean
  toggleEditMode: () => void
  handleLayoutModeToggle: () => void
  layouts: HomeDashboardLayouts
  resolvedLayoutMode: HomeLayoutMode
  layoutFade: 'out' | 'in' | null
  applyImportedHomeLayout: (payload: HomeLayoutImportPayload) => void | Promise<void>
  isFreeLayout?: boolean
  stickerPicking?: boolean
  startStickerPick?: () => void
}

export function HomeStatusBarActions({
  isEditMode,
  toggleEditMode,
  handleLayoutModeToggle,
  layouts,
  resolvedLayoutMode,
  layoutFade,
  applyImportedHomeLayout,
}: HomeAdminChromeProps) {
  const { t } = useI18n()
  const navigate = useNavigate()

  return (
    <div className="home-status-bar__actions">
      <div className="home-status-bar__sep" />

      <HomeStatusBarSlot open={isEditMode} side="before">
        {isEditMode ? (
          <Suspense fallback={null}>
            <TitleFontSelector />
          </Suspense>
        ) : null}
      </HomeStatusBarSlot>

      <button
        type="button"
        data-tour="home-edit"
        onClick={toggleEditMode}
        className={`flex px-4 py-1.5 rounded-lg text-xs font-bold items-center gap-2 transition-all ${
          isEditMode
            ? 'text-white shadow-md hover:opacity-90'
            : 'bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10'
        }`}
        style={{
          backgroundColor: isEditMode ? 'var(--color-primary)' : undefined,
          color: isEditMode ? '#fff' : 'var(--color-primary)',
        }}
        aria-label={isEditMode ? t.common.done : t.common.edit}
      >
        <FaEdit size={12} aria-hidden />
        <span className="home-status-bar__mode" aria-hidden>
          <span data-on={!isEditMode || undefined}>{t.common.edit}</span>
          <span data-on={isEditMode || undefined}>{t.common.done}</span>
        </span>
      </button>

      <HomeStatusBarSlot open={isEditMode} side="after">
        <button
          type="button"
          data-tour="home-free-layout"
          onClick={handleLayoutModeToggle}
          className="flex px-4 py-1.5 rounded-lg text-xs font-bold items-center gap-2 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
          style={{ color: 'var(--color-primary)' }}
          aria-pressed={false}
          aria-label={t.home.switchToFreeLayout}
          title={t.home.switchToFreeLayout}
        >
          <FaExpand size={12} />
          {t.home.freeLayout}
        </button>
        <Suspense fallback={null}>
          <HomeLayoutTransferButtons
            buttonClassName="flex px-4 py-1.5 rounded-lg text-xs font-bold items-center gap-2 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10 disabled:opacity-50"
            buttonStyle={{ color: 'var(--color-primary)' }}
            layouts={layouts}
            mode={resolvedLayoutMode}
            disabled={layoutFade !== null}
            onImport={applyImportedHomeLayout}
          />
        </Suspense>
      </HomeStatusBarSlot>

      <button
        type="button"
        onClick={() => navigate('/config')}
        className="flex px-4 py-1.5 rounded-lg text-xs font-bold items-center gap-2 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
        style={{ color: 'var(--color-primary)' }}
        title={t.nav.config}
        aria-label={t.nav.config}
      >
        <FaCog size={12} />
        {t.nav.config}
      </button>
    </div>
  )
}

export function HomeLayoutRail({
  isEditMode,
  toggleEditMode,
  handleLayoutModeToggle,
  layouts,
  resolvedLayoutMode,
  layoutFade,
  applyImportedHomeLayout,
  isFreeLayout,
  stickerPicking,
  startStickerPick,
}: HomeAdminChromeProps) {
  const { t } = useI18n()
  const navigate = useNavigate()

  return (
    <div className="home-layout-rail" data-library-dock-chrome="">
      <motion.div
        className="home-layout-rail__island"
        initial={{ opacity: 0, y: 12, scale: 0.96 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ type: 'spring', damping: 36, stiffness: 240 }}
      >
        {isEditMode ? (
          <div className="home-layout-rail__cluster">
            <button
              type="button"
              data-tour="home-free-layout"
              className={`home-layout-rail__btn ${
                isFreeLayout ? 'is-active' : ''
              }`}
              onClick={handleLayoutModeToggle}
              aria-pressed={isFreeLayout}
              aria-label={
                isFreeLayout
                  ? t.home.switchToStandardLayout
                  : t.home.switchToFreeLayout
              }
              title={
                isFreeLayout
                  ? t.home.switchToStandardLayout
                  : t.home.switchToFreeLayout
              }
            >
              {isFreeLayout ? (
                <FaCompress size={13} />
              ) : (
                <FaExpand size={13} />
              )}
              {isFreeLayout ? t.home.standardLayout : t.home.freeLayout}
            </button>
            {isFreeLayout ? (
              <button
                type="button"
                data-tour="home-sticker"
                className={`home-layout-rail__btn ${
                  stickerPicking ? 'is-active' : ''
                }`}
                onClick={startStickerPick}
                aria-label={t.home.createSticker}
                aria-pressed={stickerPicking}
              >
                <LuSparkles size={13} />
                {t.home.createSticker}
              </button>
            ) : null}
            <Suspense fallback={null}>
              <HomeLayoutTransferButtons
                buttonClassName="home-layout-rail__btn"
                layouts={layouts}
                mode={resolvedLayoutMode}
                disabled={layoutFade !== null}
                onImport={applyImportedHomeLayout}
              />
            </Suspense>
          </div>
        ) : null}
        {isEditMode && isFreeLayout ? (
          <div className="home-layout-rail__rule" />
        ) : null}
        {isFreeLayout ? (
          <div className="home-layout-rail__cluster">
            {isEditMode ? (
              <Suspense fallback={null}>
                <TitleFontSelector
                  buttonClassName="home-layout-rail__btn"
                  showHeroOptions={false}
                />
              </Suspense>
            ) : null}
            <button
              type="button"
              data-tour="home-edit"
              className={`home-layout-rail__btn ${
                isEditMode ? 'is-active' : ''
              }`}
              onClick={toggleEditMode}
              aria-pressed={isEditMode}
              aria-label={isEditMode ? t.common.done : t.common.edit}
              title={isEditMode ? t.common.done : t.common.edit}
            >
              <FaEdit size={13} />
              {isEditMode ? t.common.done : t.common.edit}
            </button>
            <button
              type="button"
              className="home-layout-rail__btn"
              onClick={() => navigate('/config')}
              aria-label={t.nav.config}
              title={t.nav.config}
            >
              <FaCog size={13} />
              {t.nav.config}
            </button>
          </div>
        ) : null}
      </motion.div>
    </div>
  )
}
