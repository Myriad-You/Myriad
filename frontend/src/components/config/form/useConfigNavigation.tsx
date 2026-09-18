import type { SectionSwitchDirection } from '../../settings'
import type { ConfigSectionCopy } from './configSections'
import type { QuickAccessItem } from './types'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { settingsAgentLabel } from '../../../features/merope/publicName'
import { usePersonaPublicName } from '../../../features/merope/usePersonaPublicName'
import { agentSettingsPath } from '../../agent/settings/agentSettingsPath'
import {
  scheduleScrollToSettingGuide,
  scrollToSettingGuide,
} from '../../settings/guides/guideAnchor'
import { refreshConfigTourSurface } from '../../tour/tourLogic'
import MyriadConfigIcon from '../MyriadConfigIcon'
import {
  CONFIG_NAV_DEFAULT_SECTION,
  loadConfigNavPersisted,
  federationSettingsVisible,
  resolveConfigSectionFromSearch,
  resolveInitialConfigSection,
  saveConfigNavPersisted,
  snapshotConfigNavScroll,
  syncConfigSectionToUrl,
} from './configNavPersistence'
import { configSectionCatalog } from './configSections'
import { LEGACY_CONFIG_SECTION_MAP, loadConfigFavorites } from './defaults'
import { useConfigDomain } from './useConfigDomain'

export function useConfigNavigation(
  isAdmin: boolean,
  t: ConfigSectionCopy,
  federationEnabled = true,
) {
  const navigate = useNavigate()
  const [activeSection, setActiveSection] = useState(() =>
    resolveInitialConfigSection(isAdmin, federationEnabled),
  )
  const [sectionDir, setSectionDir] =
    useState<SectionSwitchDirection>('forward')
  const [mobilePane, setMobilePane] = useState<'nav' | 'section'>(() => {
    if (typeof window === 'undefined') return 'nav'
    const stored = loadConfigNavPersisted()
    const initial = resolveInitialConfigSection(isAdmin, federationEnabled)
    if (stored?.mobilePane) return stored.mobilePane
    if (initial !== CONFIG_NAV_DEFAULT_SECTION || stored?.section) {
      return 'section'
    }
    try {
      if (new URLSearchParams(window.location.search).get('section')) {
        return 'section'
      }
    } catch {
      /* ignore */
    }
    return 'nav'
  })
  const [isMobileLayout, setIsMobileLayout] = useState(false)
  const [platformFocus, setPlatformFocus] = useState<string | null>(() => {
    const stored = loadConfigNavPersisted()
    return stored?.platformFocus ?? null
  })
  const pendingGuideScrollRef = React.useRef<string | null>(null)
  /** restore scroll once on first paint */
  const didRestoreScrollRef = useRef(false)
  const [initialFavorites] = useState(loadConfigFavorites)
  const favoritesDomain = useConfigDomain({
    id: 'favorites',
    initial: initialFavorites,
    ready: true,
    persist: async (draft) => {
      localStorage.setItem('config_favorites', JSON.stringify(draft))
      return draft
    },
  })
  const favorites = favoritesDomain.draft
  const setFavorites = favoritesDomain.setDraft
  const personaName = usePersonaPublicName()
  const agentLabel = settingsAgentLabel(t.config.agent, personaName)

  const sections = useMemo(
    () => configSectionCatalog(t, isAdmin, agentLabel, federationEnabled),
    [t, isAdmin, agentLabel, federationEnabled],
  )

  useEffect(() => {
    if (
      activeSection === 'federation' &&
      !federationSettingsVisible(isAdmin, federationEnabled)
    ) {
      setActiveSection(CONFIG_NAV_DEFAULT_SECTION)
    }
  }, [activeSection, federationEnabled, isAdmin])
  const quickAccessItems: QuickAccessItem[] = useMemo(
    () =>
      sections.map((section) => ({
        id: section.id,
        section: section.id,
        label: section.title,
        description: section.description,
        icon: <MyriadConfigIcon kind={section.id} />,
        href: section.href,
      })),
    [sections],
  )

  useEffect(() => {
    if (typeof window === 'undefined') return
    const mq = window.matchMedia('(max-width: 1023px)')
    const sync = () => setIsMobileLayout(mq.matches)
    sync()
    mq.addEventListener('change', sync)
    return () => mq.removeEventListener('change', sync)
  }, [])

  useEffect(() => {
    saveConfigNavPersisted({
      section: activeSection,
      mobilePane,
      platformFocus,
    })
    syncConfigSectionToUrl(activeSection)
    refreshConfigTourSurface()
  }, [activeSection, mobilePane, platformFocus])

  useEffect(() => {
    if (typeof window === 'undefined') return
    let ticking = false
    const onScroll = () => {
      if (ticking) return
      ticking = true
      window.requestAnimationFrame(() => {
        ticking = false
        snapshotConfigNavScroll()
      })
    }
    window.addEventListener('scroll', onScroll, { passive: true })
    return () => window.removeEventListener('scroll', onScroll)
  }, [])

  useEffect(() => {
    if (typeof window === 'undefined' || didRestoreScrollRef.current) return
    const stored = loadConfigNavPersisted()
    if (!stored || stored.section !== activeSection) return
    const y = stored.scrollY
    if (typeof y !== 'number' || y <= 0) {
      didRestoreScrollRef.current = true
      return
    }

    let cancelled = false
    const restore = () => {
      if (cancelled || didRestoreScrollRef.current) return
      window.scrollTo({ top: y, behavior: 'auto' })
    }
    const markDone = () => {
      didRestoreScrollRef.current = true
    }

    const t0 = window.setTimeout(restore, 0)
    const t1 = window.setTimeout(restore, 120)
    const t2 = window.setTimeout(restore, 450)
    let readyTimer: number | undefined
    const onLoaded = () => {
      restore()
      window.clearTimeout(readyTimer)
      readyTimer = window.setTimeout(() => {
        restore()
        markDone()
      }, 80)
    }
    window.addEventListener('config-loaded', onLoaded)

    return () => {
      cancelled = true
      window.clearTimeout(t0)
      window.clearTimeout(t1)
      window.clearTimeout(t2)
      window.clearTimeout(readyTimer)
      window.removeEventListener('config-loaded', onLoaded)
    }
  }, [activeSection])

  const handleSectionChange = useCallback(
    (section: string, options?: { guidePath?: string | null }) => {
      const next = LEGACY_CONFIG_SECTION_MAP[section] ?? section
      const portal = quickAccessItems.find((item) => item.section === next)
      if (portal?.href) {
        navigate(
          portal.href.startsWith('/agent/settings')
            ? agentSettingsPath({ guidePath: options?.guidePath })
            : portal.href,
        )
        return
      }
      if (
        next === 'federation' &&
        !federationSettingsVisible(isAdmin, federationEnabled)
      ) {
        return
      }
      const guidePath = options?.guidePath?.trim() || null
      pendingGuideScrollRef.current = guidePath

      const order = quickAccessItems.map((item) => item.section)
      const from = order.indexOf(activeSection)
      const to = order.indexOf(next)
      setSectionDir(from >= 0 && to >= 0 && to < from ? 'back' : 'forward')

      const sameSection = next === activeSection
      setActiveSection(next)
      setPlatformFocus(null)
      setMobilePane('section')
      // new section: scroll top; drop previous snapshot
      saveConfigNavPersisted({
        section: next,
        mobilePane: 'section',
        platformFocus: null,
        scrollY: 0,
      })
      syncConfigSectionToUrl(next)

      if (sameSection && guidePath) {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            const path = pendingGuideScrollRef.current
            pendingGuideScrollRef.current = null
            if (path) scrollToSettingGuide(path)
          })
        })
      }
    },
    [quickAccessItems, activeSection, federationEnabled, isAdmin, navigate],
  )

  useEffect(() => {
    if (typeof window === 'undefined') return
    const applySectionFromUrl = () => {
      const params = new URLSearchParams(window.location.search)
      const next = resolveConfigSectionFromSearch(
        params,
        isAdmin,
        federationEnabled,
      )
      if (!next) return
      const known = quickAccessItems.some((item) => item.section === next)
      if (!known) return
      setActiveSection(next)
      const platformQ = params.get('platform')
      if (platformQ === 'discord' || params.get('discord_oauth')) {
        setPlatformFocus('Discord')
      } else {
        setPlatformFocus(null)
      }
      setMobilePane('section')
      saveConfigNavPersisted({
        section: next,
        mobilePane: 'section',
        platformFocus:
          platformQ === 'discord' || params.get('discord_oauth')
            ? 'Discord'
            : null,
      })
    }
    applySectionFromUrl()
    window.addEventListener('popstate', applySectionFromUrl)
    return () => window.removeEventListener('popstate', applySectionFromUrl)
  }, [federationEnabled, isAdmin, quickAccessItems])

  const scrollSettingsToTop = useCallback(() => {
    if (typeof window === 'undefined') return
    window.scrollTo({ top: 0, behavior: 'auto' })
    saveConfigNavPersisted({ scrollY: 0 })
    const path = pendingGuideScrollRef.current
    if (!path) return
    pendingGuideScrollRef.current = null
    scheduleScrollToSettingGuide(path)
  }, [])

  const handleMobileBackToNav = useCallback(() => {
    setMobilePane('nav')
    setPlatformFocus(null)
    saveConfigNavPersisted({ mobilePane: 'nav', platformFocus: null })
    scrollSettingsToTop()
  }, [scrollSettingsToTop])

  const toggleFavorite = useCallback((section: string) => {
    setFavorites((prev) =>
      prev.includes(section)
        ? prev.filter((id) => id !== section)
        : [...prev, section],
    )
  }, [])

  const getSectionProps = useCallback(
    (sectionId: string) => {
      const item = quickAccessItems.find((i) => i.id === sectionId)
      if (!item) {
        return { title: '', icon: null, description: '' }
      }
      return {
        title: item.label,
        icon: item.icon,
        sectionId: item.id,
        description: item.description,
      }
    },
    [quickAccessItems],
  )

  return {
    activeSection,
    setActiveSection,
    sectionDir,
    mobilePane,
    setMobilePane,
    isMobileLayout,
    platformFocus,
    setPlatformFocus,
    favorites,
    setFavorites,
    favoritesDomain,
    quickAccessItems,
    canResetCurrentPage:
      sections.find((section) => section.id === activeSection)?.showReset ??
      false,
    handleSectionChange,
    scrollSettingsToTop,
    handleMobileBackToNav,
    toggleFavorite,
    getSectionProps,
  }
}
