import type { ChangeEvent, DragEvent, FormEvent } from 'react'
import type { FeedType } from '../../../../types/brew'
import type { AddFieldKind, AddSourceKind, DiscoveredFeed } from './addSource'

import { useEffect, useRef, useState } from 'react'
import { currentCopy } from '../../../../i18n/localeCopy'
import { userFacingError } from '../../../../utils/userFacingError'
import { RequestTurn, unlessAborted } from '../../logic/requestTurn'
import {
  addFieldKind,
  canAutoDiscover,
  canSubmitAdd,
  faviconForUrl,
  isOpmlFilename,
  pickAddKind,
  resolveAddSourceType,
} from './addSource'

export interface AddSubmitInput {
  sourceType: AddSourceKind
  feedType: FeedType
  url: string
  name: string
  category: string
  customIcon: string | null
  notionToken?: string
  rsshubConfig?: unknown
  enableBrewlia?: boolean
}

export function useAddSourceForm(io: {
  onSubmit?: (
    data: AddSubmitInput,
  ) => Promise<{ success: boolean; error?: string; title?: string }>
  onDiscover?: (
    url: string,
    signal?: AbortSignal,
  ) => Promise<DiscoveredFeed | null>
  onImportOpml?: (
    content: string,
    signal?: AbortSignal,
  ) => Promise<{ imported: number; skipped: number }>
  onExportOpml?: () => void
}) {
  const discoverTurns = useRef(new RequestTurn())
  const importTurns = useRef(new RequestTurn())
  useEffect(
    () => () => {
      discoverTurns.current.cancel()
      importTurns.current.cancel()
    },
    [],
  )
  const [tab, setTab] = useState<'single' | 'opml'>('single')
  const [sourceType, setSourceType] = useState<AddSourceKind>('rss')
  const [feedType, setFeedType] = useState<
    Extract<FeedType, 'rss' | 'atom' | 'notion' | 'rsshub'>
  >('rss')
  const [url, setUrl] = useState('')
  const [name, setName] = useState('')
  const [category, setCategory] = useState('')
  const [customIcon, setCustomIcon] = useState<string | null>(null)
  const [notionToken, setNotionToken] = useState('')
  const [rsshubConfig, setRsshubConfig] = useState<unknown>(null)
  const [rsshubFullUrl, setRsshubFullUrl] = useState('')
  const [enableBrewliaForRsshub, setEnableBrewliaForRsshub] = useState(false)
  const [categoryOpen, setCategoryOpen] = useState(false)
  const [loading, setLoading] = useState(false)
  const [discovering, setDiscovering] = useState(false)
  const [discovered, setDiscovered] = useState<DiscoveredFeed | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<string | null>(null)
  const [opmlContent, setOpmlContent] = useState<string | null>(null)
  const [opmlLoading, setOpmlLoading] = useState(false)
  const [opmlResult, setOpmlResult] = useState<{
    imported: number
    skipped: number
  } | null>(null)
  const [dragOver, setDragOver] = useState(false)
  const [exporting, setExporting] = useState(false)

  const iconInputRef = useRef<HTMLInputElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const fieldKind = addFieldKind(sourceType, feedType)
  const displayIcon =
    customIcon || (discovered?.title ? faviconForUrl(url) : null)

  const pickKind = (kind: AddFieldKind) => {
    const next = pickAddKind(kind)
    setSourceType(next.sourceType)
    setFeedType(next.feedType)
    setDiscovered(null)
    setError(null)
    if (next.clearUrl) setUrl('')
  }

  const readOpml = (file: File | undefined) => {
    if (!file) return
    const reader = new FileReader()
    reader.onload = (event) => {
      setOpmlContent(event.target?.result as string)
    }
    reader.readAsText(file)
  }

  const handleDiscover = async () => {
    if (!url.trim() || !io.onDiscover) return
    const signal = discoverTurns.current.begin()
    setDiscovering(true)
    setError(null)
    try {
      const result = await io.onDiscover(url, signal)
      unlessAborted(signal, () => {
        setDiscovered(result)
        if (result) {
          setUrl(result.url)
          setName(result.title)
        }
      })
    } catch (err) {
      unlessAborted(signal, () => {
        setError(userFacingError(err, currentCopy().brew.errorDiscoverFailed))
      })
    } finally {
      unlessAborted(signal, () => setDiscovering(false))
    }
  }

  const handleIconUpload = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    if (!file) return
    const reader = new FileReader()
    reader.onload = (load) => {
      setCustomIcon(load.target?.result as string)
    }
    reader.readAsDataURL(file)
  }

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!io.onSubmit) return

    setLoading(true)
    setError(null)
    setSuccess(null)

    try {
      let resolvedUrl = sourceType === 'rsshub' ? rsshubFullUrl : url
      let resolvedName = name
      const discover = io.onDiscover
      const shouldAutoDiscover =
        canAutoDiscover(sourceType, feedType) && !!discover

      if (shouldAutoDiscover && discover) {
        const signal = discoverTurns.current.begin()
        setDiscovering(true)
        const feed =
          discovered?.url === url
            ? discovered
            : await discover(url.trim(), signal)
        if (signal.aborted) return
        if (!feed) throw new Error(currentCopy().brew.errorDiscoverFailed)

        resolvedUrl = feed.url
        resolvedName = name.trim() || feed.title
        setUrl(feed.url)
        setName(resolvedName)
        setDiscovered(feed)
        setDiscovering(false)
      }

      const result = await io.onSubmit({
        sourceType: resolveAddSourceType(sourceType, enableBrewliaForRsshub),
        feedType,
        url: resolvedUrl,
        name: resolvedName,
        category,
        customIcon,
        notionToken: feedType === 'notion' ? notionToken : undefined,
        rsshubConfig: sourceType === 'rsshub' ? rsshubConfig : undefined,
        enableBrewlia: enableBrewliaForRsshub,
      })

      if (result.success) {
        setSuccess(result.title || currentCopy().brew.addSuccess)
        setUrl('')
        setName('')
        setCategory('')
        setCustomIcon(null)
        setDiscovered(null)
        setNotionToken('')
      } else {
        setError(userFacingError(result.error, currentCopy().brew.errorAddFailed))
      }
    } catch (err) {
      setError(userFacingError(err, currentCopy().brew.errorAddFailed))
    } finally {
      setDiscovering(false)
      setLoading(false)
    }
  }

  const handleImport = async () => {
    if (!opmlContent || !io.onImportOpml) return
    const signal = importTurns.current.begin()
    setOpmlLoading(true)
    setError(null)
    try {
      const result = await io.onImportOpml(opmlContent, signal)
      if (signal.aborted) return
      setOpmlResult(result)
      setOpmlContent(null)
    } catch (err) {
      if (signal.aborted) return
      setError(userFacingError(err, currentCopy().brew.errorImportFailed))
    } finally {
      if (!signal.aborted) setOpmlLoading(false)
    }
  }

  const handleExport = () => {
    if (!io.onExportOpml) return
    setExporting(true)
    try {
      io.onExportOpml()
    } finally {
      setExporting(false)
    }
  }

  return {
    tab,
    setTab,
    sourceType,
    setSourceType,
    feedType,
    fieldKind,
    url,
    setUrl,
    name,
    setName,
    category,
    setCategory,
    customIcon,
    notionToken,
    setNotionToken,
    enableBrewliaForRsshub,
    setEnableBrewliaForRsshub,
    categoryOpen,
    setCategoryOpen,
    loading,
    discovering,
    discovered,
    setDiscovered,
    error,
    success,
    opmlContent,
    opmlLoading,
    opmlResult,
    dragOver,
    setDragOver,
    exporting,
    displayIcon,
    canSubmit: canSubmitAdd({ sourceType, url, name, rsshubFullUrl }),
    iconInputRef,
    fileInputRef,
    pickKind,
    setRsshub: (config: unknown, fullUrl: string) => {
      setRsshubConfig(config)
      setRsshubFullUrl(fullUrl)
    },
    handleDiscover,
    handleIconUpload,
    clearIcon: () => {
      setCustomIcon(null)
      if (iconInputRef.current) iconInputRef.current.value = ''
    },
    pickCategory: (value: string) => {
      setCategory(value)
      setCategoryOpen(false)
    },
    handleSubmit,
    handleFileSelect: (event: ChangeEvent<HTMLInputElement>) => {
      readOpml(event.target.files?.[0])
    },
    handleDrop: (event: DragEvent) => {
      event.preventDefault()
      setDragOver(false)
      const file = event.dataTransfer.files[0]
      if (file && isOpmlFilename(file.name)) readOpml(file)
    },
    handleImport,
    handleExport,
  }
}
