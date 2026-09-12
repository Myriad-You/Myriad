import { FaCompress, FaExpand, FaTh } from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useCallback, useMemo, useState } from 'react'
import { Navigate, useNavigate, useSearchParams } from 'react-router-dom'

import { useI18n } from '../../contexts/I18nContext'
import { isExlight, useAnimationLevel } from '../../hooks/useAnimationLevel'
import { usePageSeo } from '../../hooks/usePageSeo'
import { useBreakpoints } from '../../hooks/useSharedEventListener'
import {
  canAccessModuleVisibility,
  useModuleVisibilityPreferences,
} from '../../utils/moduleVisibility'
import { isWebKit } from '../../utils/platformDetect'
import { TappAppShell } from '../components/TappAppShell'
import { TappIcon } from '../components/TappIcon'
import { TappStore } from '../components/TappStore'
import { HOST_PANEL_STORE_ID } from '../constants/hostPanels'
import { TAPP_ICON_TOKENS } from '../constants/icons'
import { useTappFullscreenChrome } from '../hooks/useTappFullscreenChrome'
import { useTappShellClose } from '../hooks/useTappShellClose'
import { useTappShellPresence } from '../hooks/useTappShellPresence'
import { buildTappStorePageSeo } from '../utils/tappPageSeo'
import { TAPP_LIST_PATH, tappRunPath } from '../utils/tappPaths'

export function TappStorePage() {
  const [searchParams] = useSearchParams()
  const { isMobile } = useBreakpoints()
  const wantsMulti =
    searchParams.get('multi') === 'true' && !isWebKit && !isMobile
  if (wantsMulti) {
    return (
      <Navigate
        to={tappRunPath(HOST_PANEL_STORE_ID, { multi: true })}
        replace
      />
    )
  }

  return <TappStorePageStandard isMobile={isMobile} />
}

function TappStorePageStandard({ isMobile }: { isMobile: boolean }) {
  const navigate = useNavigate()
  const { t } = useI18n()
  const animConfig = useAnimationLevel()
  const noAnimation = isExlight(animConfig)
  const { preferences: moduleVisibility } = useModuleVisibilityPreferences()
  const moduleOpenToAll = canAccessModuleVisibility(
    moduleVisibility.modules.tapp,
    { isAuthenticated: false, isAdmin: false },
  )

  const [isFullscreen, setIsFullscreen] = useState(false)
  useTappFullscreenChrome(isFullscreen, setIsFullscreen, {
    enableEscape: true,
  })

  usePageSeo(
    useMemo(
      () =>
        buildTappStorePageSeo({
          storeLabel: t.tapp.storeTitle,
          storeDescription: t.tapp.listSubtitle,
          moduleOpenToAll,
        }),
      [t.tapp.storeTitle, t.tapp.listSubtitle, moduleOpenToAll],
    ),
  )

  const {
    requestClose,
    shellClassName,
    scrimClassName,
    shellStyle,
    onShellAnimationEnd,
    isExiting,
  } = useTappShellPresence({
    enabled: !isFullscreen,
    fade: true,
  })
  const presence = useMemo(
    () => ({
      shellClassName,
      scrimClassName,
      shellStyle,
      onShellAnimationEnd,
      isExiting,
    }),
    [
      shellClassName,
      scrimClassName,
      shellStyle,
      onShellAnimationEnd,
      isExiting,
    ],
  )

  const navigateHome = useCallback(() => {
    navigate(TAPP_LIST_PATH)
  }, [navigate])
  const goBack = useTappShellClose({
    isFullscreen,
    setIsFullscreen,
    requestClose,
    onClosed: navigateHome,
  })
  const openMulti = useCallback(() => {
    navigate(tappRunPath(HOST_PANEL_STORE_ID, { multi: true }))
  }, [navigate])
  const toggleFullscreen = useCallback(() => {
    setIsFullscreen((v) => !v)
  }, [])

  const transitions = useMemo(
    () => ({
      toolbar: animConfig.spring
        ? { type: 'spring' as const, stiffness: 320, damping: 28 }
        : {
            type: 'tween' as const,
            duration: 0.25 * animConfig.durationScale,
          },
    }),
    [animConfig.spring, animConfig.durationScale],
  )

  const fsToolbar = (
    <AnimatePresence>
      {isFullscreen && (
        <motion.div
          key="store-fullscreen-toolbar"
          initial={{ opacity: 0, x: -16, scale: 0.92 }}
          animate={{ opacity: 1, x: 0, scale: 1 }}
          exit={{ opacity: 0, x: -16, scale: 0.92 }}
          transition={transitions.toolbar}
          className={`fixed z-900 transition-opacity duration-300 ${
            isMobile
              ? 'opacity-100'
              : 'opacity-0 hover:opacity-100 focus-within:opacity-100'
          }`}
          style={{
            top: 'max(1rem, calc(env(safe-area-inset-top, 0px) + 0.5rem))',
            left: 'max(1rem, env(safe-area-inset-left, 0px))',
          }}
        >
          <div className="glass flex items-center gap-3 rounded-xl px-3 py-2 shadow-lg">
            <div className="flex items-center gap-2">
              <div className="flex shrink-0 items-center justify-center">
                <TappIcon
                  icon={TAPP_ICON_TOKENS.store}
                  name={t.tapp.storeTitle}
                  sizeClass="w-5 h-5"
                />
              </div>
              <div className="hidden sm:block">
                <h1 className="text-xs font-semibold leading-tight text-gray-800 dark:text-gray-100">
                  {t.tapp.storeTitle}
                </h1>
              </div>
            </div>
            <div className="h-6 w-px bg-gray-200 dark:bg-neutral-700" />
            <div className="flex items-center gap-1">
              <motion.button
                onClick={toggleFullscreen}
                className="rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-700 dark:hover:bg-neutral-700 dark:hover:text-gray-300"
                title={t.tapp.exitFullscreen}
                whileHover={noAnimation ? undefined : { scale: 1.1 }}
                whileTap={noAnimation ? undefined : { scale: 0.9 }}
              >
                <FaCompress className="h-3.5 w-3.5" />
              </motion.button>
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )

  return (
    <TappAppShell
      shellAttr="data-tapp-store-shell"
      isMobile={isMobile}
      isFullscreen={isFullscreen}
      presence={presence}
      fullscreenToolbar={fsToolbar}
      onBack={goBack}
      backTitle={t.tapp.back}
      backAriaLabel={t.tapp.backToAppList}
      contentClassName={
        isFullscreen ? undefined : 'tapp-store-standard-content shadow-sm'
      }
      headerLeading={
        <div className="flex min-w-0 items-center gap-2">
          <div className="flex shrink-0 items-center justify-center">
            <TappIcon
              icon={TAPP_ICON_TOKENS.store}
              name={t.tapp.storeTitle}
              sizeClass="w-5 h-5"
            />
          </div>
          <span className="truncate text-sm font-semibold text-gray-800 dark:text-gray-100">
            {t.tapp.storeTitle}
          </span>
        </div>
      }
      headerActions={
        <div className="flex shrink-0 items-center gap-1">
          {!isMobile && !isWebKit && (
            <motion.button
              onClick={openMulti}
              className="rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-indigo-50 hover:text-indigo-600 dark:hover:bg-indigo-900/20 dark:hover:text-indigo-400"
              title={t.tapp.multiWindow}
              whileHover={noAnimation ? undefined : { scale: 1.15 }}
              whileTap={noAnimation ? undefined : { scale: 0.9 }}
            >
              <FaTh className="h-3.5 w-3.5" />
            </motion.button>
          )}
          <motion.button
            onClick={toggleFullscreen}
            className="rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-700 dark:hover:bg-neutral-700 dark:hover:text-gray-300"
            title={t.tapp.fullscreen}
            whileHover={noAnimation ? undefined : { scale: 1.15 }}
            whileTap={noAnimation ? undefined : { scale: 0.9 }}
          >
            <FaExpand className="h-3.5 w-3.5" />
          </motion.button>
        </div>
      }
    >
      <div
        className={`h-full overflow-hidden ${
          isFullscreen
            ? 'bg-[var(--bg-primary)]'
            : 'tapp-store-frame--wallpaper'
        }`}
      >
        <TappStore
          className="h-full"
          embeddedChrome
          fullscreen={isFullscreen}
        />
      </div>
    </TappAppShell>
  )
}

export default TappStorePage
