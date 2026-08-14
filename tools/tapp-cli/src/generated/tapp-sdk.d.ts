/**
 * Myriad Tapp sandbox SDK (window.Tapp).
 * Generated from the runtime permission catalog. Call sites still need
 * matching Manifest permissions and a handler in the current sandbox profile.
 *
 * Headless-denied Bridge actions: component.list, component.registerAgent, component.registerTheme, component.unregister, dynamicContent.get, dynamicContent.remove, dynamicContent.set, dynamicContent.update, file.download, shortcut.list, shortcut.register, shortcut.unregister, tappList.export, tappList.get, tappList.getInstallPackage, tappList.getRecent, tappList.install, tappList.list, tappList.resolveStoreSource, tappList.start, tappList.stop, tappList.uninstall, ui.confirm, ui.exitFullscreen, ui.isFullscreen, ui.listOpenUrls, ui.openUrl, ui.requestFullscreen, ui.setTitle, ui.toggleFullscreen, widget.listRegistered, widget.register, widget.unregister, widget.updateConfig
 *
 * Do not edit by hand — run: npm run sync-contract
 */

export interface TappSdk {
  lifecycle: {
    onReady(callback: () => void | Promise<void>): void
    onDestroy(callback: () => void): void
    onPause(callback: () => void): void
    onResume(callback: () => void): void
    getInfo(): { id: string; version: string; name: string; permissions: string[]; sandboxed: true }
  }

  i18n: {
    t(key: string, params?: Record<string, unknown>): string
    getLocale(): string
    getAll(): Record<string, unknown>
  }

  storage: {
    get(key: string): Promise<unknown>
    set(key: string, value: unknown): Promise<unknown>
    remove(key: string): Promise<unknown>
    keys(): Promise<string[]>
    getAll(): Promise<Record<string, unknown>>
    clear(): Promise<unknown>
    usage(): Promise<unknown>
    onChanged(callback: (event: { key?: string; operation?: string }) => void): () => void
  }

  settings: {
    get(key: string): Promise<unknown>
    set(key: string, value: unknown): Promise<unknown>
    getAll(): Promise<Record<string, unknown>>
  }

  ui: {
    setTitle(title: string): Promise<unknown>
    getTheme(): Promise<unknown>
    onThemeChange(callback: (theme: unknown) => void): () => void
    getPrimaryColor(): Promise<unknown>
    onPrimaryColorChange(callback: (color: unknown) => void): () => void
    getLocale(): Promise<unknown>
    onLocaleChange(callback: (locale: unknown) => void): () => void
    showNotification(options: unknown): Promise<unknown>
    confirm(message: string): Promise<boolean>
    /**
     * Open a host browser tab for a Manifest `openUrls` allowlisted id only.
     * Pass `{ id, path?, query? }` — never a free-form absolute URL.
     * Requires permission `ui:openUrl`.
     */
    openUrl(request: {
      id: string
      path?: string
      query?: Record<string, string>
    } | string): Promise<unknown>
    /** List install-time openUrls declarations for this Tapp. */
    listOpenUrls(): Promise<
      Array<{ id: string; url: string; match: 'exact' | 'prefix' | 'origin' }>
    >
    requestFullscreen(): Promise<unknown>
    exitFullscreen(): Promise<unknown>
    fullscreen: {
      request(): Promise<unknown>
      exit(): Promise<unknown>
      toggle(): Promise<unknown>
      isFullscreen(): Promise<boolean>
    }
  }

  api: {
    (name: string, params?: unknown): Promise<unknown>
    list(): Promise<unknown>
  }

  assets: {
    list(): Promise<unknown>
    get(path: string): Promise<unknown>
    getUrl(path: string): Promise<{ url: string; mimeType?: string; size?: number; path?: string }>
    getArrayBuffer(path: string): Promise<{ path: string; mimeType?: string; size?: number; buffer: ArrayBuffer }>
    getUrlMap(): Promise<Record<string, string>>
    resolve(path: string): Promise<{ url: string; mimeType?: string; size?: number; path?: string }>
    rewriteUrl(url: string): string
    revoke(url: string): void
    revokeAll(): void
  }

  dataExchange: {
    request(request: unknown): Promise<unknown>
    provide(exportId: string, handler: (request: unknown) => unknown | Promise<unknown>): Promise<() => void>
  }

  event: {
    publish(request: unknown): Promise<unknown>
    on(topic: string, callback: (event: unknown) => void): () => void
  }

  agent: {
    onInteraction(
      type: string,
      callback: (interaction: {
        type: string
        interactionId: string
        accept(): Promise<unknown>
        submitResult(result: unknown): Promise<unknown>
        reject(reason?: unknown): Promise<unknown>
        requestIntent(request: unknown): Promise<unknown>
        [key: string]: unknown
      }) => void,
    ): () => void
  }

  media: {
    play(): Promise<unknown>
    pause(): Promise<unknown>
    next(): Promise<unknown>
    prev(): Promise<unknown>
    seek(position: unknown): Promise<unknown>
    setVolume(volume: unknown): Promise<unknown>
    setMode(mode: unknown): Promise<unknown>
    mute(): Promise<unknown>
    unmute(): Promise<unknown>
    getStatus(): Promise<unknown>
    getPlaylist(): Promise<unknown>
    getSpectrum(): Promise<unknown>
    getLyrics(options?: unknown): Promise<unknown>
    getBeatGrid(): Promise<unknown>
    playTrack(id: unknown, index?: unknown): Promise<unknown>
    jumpToIndex(index: unknown): Promise<unknown>
    loadNeteasePlaylist(playlistId: unknown): Promise<unknown>
    getSkipVip(): Promise<unknown>
    setSkipVip(value: unknown): Promise<unknown>
    onStateChange(callback: (state: unknown) => void): () => void
    onProgress(callback: (progress: unknown) => void): () => void
  }

  ai: {
    tasks: {
      create(request: unknown): Promise<unknown>
      get(taskId: string): Promise<unknown>
      cancel(taskId: string): Promise<unknown>
      usage(): Promise<unknown>
      subscribe(
        taskId: string,
        callback: (event: { event: unknown; data: unknown }) => void,
      ): Promise<() => void>
    }
  }

  scheduler: {
    register(options: unknown): Promise<unknown>
    unregister(taskId: string): Promise<unknown>
    list(): Promise<unknown>
    get(taskId: string): Promise<unknown>
    enable(taskId: string): Promise<unknown>
    disable(taskId: string): Promise<unknown>
    trigger(taskId: string): Promise<unknown>
    onTask(
      taskId: string,
      callback: (payload: unknown, event: unknown) => unknown | Promise<unknown>,
    ): () => void
  }

  widgets: Record<
    string,
    {
      render(
        container: HTMLElement,
        props: {
          size?: string
          theme?: string
          settings?: Record<string, unknown>
          [key: string]: unknown
        },
      ): void | Promise<void>
    }
  >

  pages: Record<
    string,
    {
      render(container: HTMLElement): void | Promise<void>
    }
  >

  dom: {
    setAttribute(element: Element, name: string, value: string): void
    [key: string]: unknown
  }

  analytics: {
    getSummary(...args: unknown[]): Promise<unknown> // permission: analytics:read
    getVisitorCard(...args: unknown[]): Promise<unknown> // permission: analytics:read
  }
  animation: {
    getConfig(...args: unknown[]): Promise<unknown>
    getLevel(...args: unknown[]): Promise<unknown>
    getStaggerDelay(...args: unknown[]): Promise<unknown>
    shouldAnimate(...args: unknown[]): Promise<unknown>
  }
  background: {
    has(...args: unknown[]): Promise<unknown>
    list(...args: unknown[]): Promise<unknown>
    release(...args: unknown[]): Promise<unknown> // permission: event:subscribe
    require(...args: unknown[]): Promise<unknown> // permission: event:subscribe
  }
  brewList: {
    addSource(...args: unknown[]): Promise<unknown> // permission: brew:manage
    categories(...args: unknown[]): Promise<unknown> // permission: brew:read
    createCategory(...args: unknown[]): Promise<unknown> // permission: brew:manage
    createComment(...args: unknown[]): Promise<unknown> // permission: brew:comment
    createReply(...args: unknown[]): Promise<unknown> // permission: brew:comment
    deleteCategory(...args: unknown[]): Promise<unknown> // permission: brew:manage
    deleteComment(...args: unknown[]): Promise<unknown> // permission: brew:comment
    deleteSource(...args: unknown[]): Promise<unknown> // permission: brew:manage
    discover(...args: unknown[]): Promise<unknown> // permission: brew:manage
    exportOpml(...args: unknown[]): Promise<unknown> // permission: brew:read
    get(...args: unknown[]): Promise<unknown> // permission: brew:read
    getComments(...args: unknown[]): Promise<unknown> // permission: brew:comment
    getReplies(...args: unknown[]): Promise<unknown> // permission: brew:comment
    importOpml(...args: unknown[]): Promise<unknown> // permission: brew:manage
    list(...args: unknown[]): Promise<unknown> // permission: brew:read
    markAllRead(...args: unknown[]): Promise<unknown> // permission: brew:write
    markRead(...args: unknown[]): Promise<unknown> // permission: brew:write
    markUnread(...args: unknown[]): Promise<unknown> // permission: brew:write
    refreshSource(...args: unknown[]): Promise<unknown> // permission: brew:manage
    sources(...args: unknown[]): Promise<unknown> // permission: brew:read
    star(...args: unknown[]): Promise<unknown> // permission: brew:write
    stats(...args: unknown[]): Promise<unknown> // permission: brew:read
    unstar(...args: unknown[]): Promise<unknown> // permission: brew:write
    updateComment(...args: unknown[]): Promise<unknown> // permission: brew:comment
    updateSource(...args: unknown[]): Promise<unknown> // permission: brew:manage
  }
  component: {
    list(...args: unknown[]): Promise<unknown>
    registerAgent(...args: unknown[]): Promise<unknown> // permission: component:agent
    registerTheme(...args: unknown[]): Promise<unknown> // permission: component:theme
    unregister(...args: unknown[]): Promise<unknown>
  }
  context: {
    getApp(...args: unknown[]): Promise<unknown>
    getGeo(...args: unknown[]): Promise<unknown>
    getNavigation(...args: unknown[]): Promise<unknown>
    getPlayer(...args: unknown[]): Promise<unknown>
    getSystem(...args: unknown[]): Promise<unknown>
    getUser(...args: unknown[]): Promise<unknown>
  }
  data: {
    transform(...args: unknown[]): Promise<unknown>
  }
  dynamicContent: {
    get(...args: unknown[]): Promise<unknown>
    remove(...args: unknown[]): Promise<unknown> // permission: ui:notification
    set(...args: unknown[]): Promise<unknown> // permission: ui:notification
    update(...args: unknown[]): Promise<unknown> // permission: ui:notification
  }
  federation: {
    acceptChannel(...args: unknown[]): Promise<unknown> // permission: federation:write
    acceptRoomInvite(...args: unknown[]): Promise<unknown> // permission: federation:write
    addPeer(...args: unknown[]): Promise<unknown> // permission: federation:write
    addRoomSticker(...args: unknown[]): Promise<unknown> // permission: federation:write
    announce(...args: unknown[]): Promise<unknown> // permission: federation:write
    bookmark(...args: unknown[]): Promise<unknown> // permission: federation:write
    cancelAllPendingDelivery(...args: unknown[]): Promise<unknown> // permission: federation:write
    cancelDelivery(...args: unknown[]): Promise<unknown> // permission: federation:write
    cancelTransfer(...args: unknown[]): Promise<unknown> // permission: federation:files
    closeChannel(...args: unknown[]): Promise<unknown> // permission: federation:write
    composeExternalShare(...args: unknown[]): Promise<unknown> // permission: federation:read
    createChannel(...args: unknown[]): Promise<unknown> // permission: federation:write
    createNote(...args: unknown[]): Promise<unknown> // permission: federation:write
    createRing(...args: unknown[]): Promise<unknown> // permission: federation:write
    createRoom(...args: unknown[]): Promise<unknown> // permission: federation:write
    deleteChannel(...args: unknown[]): Promise<unknown> // permission: federation:write
    deleteRoom(...args: unknown[]): Promise<unknown> // permission: federation:write
    dismissDelivery(...args: unknown[]): Promise<unknown> // permission: federation:write
    downloadTransfer(...args: unknown[]): Promise<unknown> // permission: federation:files
    follow(...args: unknown[]): Promise<unknown> // permission: federation:write
    getBookmarks(...args: unknown[]): Promise<unknown> // permission: federation:read
    getChannel(...args: unknown[]): Promise<unknown> // permission: federation:read
    getChannels(...args: unknown[]): Promise<unknown> // permission: federation:read
    getDeliveryStats(...args: unknown[]): Promise<unknown> // permission: federation:read
    getExternalShareStatus(...args: unknown[]): Promise<unknown> // permission: federation:read
    getFeed(...args: unknown[]): Promise<unknown> // permission: federation:read
    getFollowers(...args: unknown[]): Promise<unknown> // permission: federation:read
    getFollowing(...args: unknown[]): Promise<unknown> // permission: federation:read
    getIdentity(...args: unknown[]): Promise<unknown> // permission: federation:read
    getInstances(...args: unknown[]): Promise<unknown> // permission: federation:trust
    getMessages(...args: unknown[]): Promise<unknown> // permission: federation:read
    getObject(...args: unknown[]): Promise<unknown> // permission: federation:read
    getPublished(...args: unknown[]): Promise<unknown> // permission: federation:read
    getRing(...args: unknown[]): Promise<unknown> // permission: federation:read
    getRingPeers(...args: unknown[]): Promise<unknown> // permission: federation:read
    getRings(...args: unknown[]): Promise<unknown> // permission: federation:read
    getRoom(...args: unknown[]): Promise<unknown> // permission: federation:read
    getRoomMembers(...args: unknown[]): Promise<unknown> // permission: federation:read
    getRoomMessages(...args: unknown[]): Promise<unknown> // permission: federation:read
    getRooms(...args: unknown[]): Promise<unknown> // permission: federation:read
    getTimeline(...args: unknown[]): Promise<unknown> // permission: federation:read
    getTransfer(...args: unknown[]): Promise<unknown> // permission: federation:files
    getTrustPolicy(...args: unknown[]): Promise<unknown> // permission: federation:trust
    initiateChannelE2e(...args: unknown[]): Promise<unknown> // permission: federation:write
    initiateRoomE2e(...args: unknown[]): Promise<unknown> // permission: federation:write
    initiateRoomTransfer(...args: unknown[]): Promise<unknown> // permission: federation:files
    initiateTransfer(...args: unknown[]): Promise<unknown> // permission: federation:files
    inviteMember(...args: unknown[]): Promise<unknown> // permission: federation:write
    joinRoom(...args: unknown[]): Promise<unknown> // permission: federation:write
    leaveRing(...args: unknown[]): Promise<unknown> // permission: federation:write
    leaveRoom(...args: unknown[]): Promise<unknown> // permission: federation:write
    like(...args: unknown[]): Promise<unknown> // permission: federation:write
    listDelivery(...args: unknown[]): Promise<unknown> // permission: federation:read
    listRoomFiles(...args: unknown[]): Promise<unknown> // permission: federation:files
    listRoomTransfers(...args: unknown[]): Promise<unknown> // permission: federation:files
    listTransfers(...args: unknown[]): Promise<unknown> // permission: federation:files
    pinRoomMessage(...args: unknown[]): Promise<unknown> // permission: federation:write
    publish(...args: unknown[]): Promise<unknown> // permission: federation:write
    purgeDeadDelivery(...args: unknown[]): Promise<unknown> // permission: federation:write
    rejectRoomInvite(...args: unknown[]): Promise<unknown> // permission: federation:write
    removeMember(...args: unknown[]): Promise<unknown> // permission: federation:write
    removePeer(...args: unknown[]): Promise<unknown> // permission: federation:write
    removeRoomSticker(...args: unknown[]): Promise<unknown> // permission: federation:write
    retryAllDeadDelivery(...args: unknown[]): Promise<unknown> // permission: federation:write
    retryDelivery(...args: unknown[]): Promise<unknown> // permission: federation:write
    rotateKeys(...args: unknown[]): Promise<unknown> // permission: federation:write
    sendMessage(...args: unknown[]): Promise<unknown> // permission: federation:message
    sendRoomMessage(...args: unknown[]): Promise<unknown> // permission: federation:message
    setMemberRole(...args: unknown[]): Promise<unknown> // permission: federation:write
    subscribeChannel(...args: unknown[]): Promise<unknown> // permission: federation:message
    subscribeRoom(...args: unknown[]): Promise<unknown> // permission: federation:message
    toggleInstanceBlock(...args: unknown[]): Promise<unknown> // permission: federation:trust
    transferRoomOwnership(...args: unknown[]): Promise<unknown> // permission: federation:write
    triggerSync(...args: unknown[]): Promise<unknown> // permission: federation:write
    unannounce(...args: unknown[]): Promise<unknown> // permission: federation:write
    unbookmark(...args: unknown[]): Promise<unknown> // permission: federation:write
    unfollow(...args: unknown[]): Promise<unknown> // permission: federation:write
    unlike(...args: unknown[]): Promise<unknown> // permission: federation:write
    unpublish(...args: unknown[]): Promise<unknown> // permission: federation:write
    unsubscribeChannel(...args: unknown[]): Promise<unknown> // permission: federation:message
    unsubscribeRoom(...args: unknown[]): Promise<unknown> // permission: federation:message
    updateInstanceTrust(...args: unknown[]): Promise<unknown> // permission: federation:trust
    updateRoom(...args: unknown[]): Promise<unknown> // permission: federation:write
    updateTrustPolicy(...args: unknown[]): Promise<unknown> // permission: federation:trust
    uploadChunk(...args: unknown[]): Promise<unknown> // permission: federation:files
    uploadMedia(...args: unknown[]): Promise<unknown> // permission: federation:write
  }
  file: {
    download(...args: unknown[]): Promise<unknown> // permission: storage
  }
  platform: {
    addItem(...args: unknown[]): Promise<unknown> // permission: platform:write
    addItems(...args: unknown[]): Promise<unknown> // permission: platform:write
    getData(...args: unknown[]): Promise<unknown> // permission: platform:read
    getDistribution(...args: unknown[]): Promise<unknown> // permission: platform:read
    getStats(...args: unknown[]): Promise<unknown> // permission: platform:read
    listEnabled(...args: unknown[]): Promise<unknown> // permission: platform:read
    registerPlatform(...args: unknown[]): Promise<unknown> // permission: platform:register
  }
  report: {
    create(...args: unknown[]): Promise<unknown> // permission: report:write
    delete(...args: unknown[]): Promise<unknown> // permission: report:write
    get(...args: unknown[]): Promise<unknown> // permission: report:read
    getPlatformReport(...args: unknown[]): Promise<unknown> // permission: report:read
    getReport(...args: unknown[]): Promise<unknown> // permission: report:read
    list(...args: unknown[]): Promise<unknown> // permission: report:read
    listReports(...args: unknown[]): Promise<unknown> // permission: report:read
    update(...args: unknown[]): Promise<unknown> // permission: report:write
  }
  shortcut: {
    list(...args: unknown[]): Promise<unknown>
    register(...args: unknown[]): Promise<unknown> // permission: shortcut:register
    unregister(...args: unknown[]): Promise<unknown> // permission: shortcut:register
  }
  speech: {
    asr(...args: unknown[]): Promise<unknown> // permission: speech:asr
    getStatus(...args: unknown[]): Promise<unknown> // permission: speech:tts
    getVoices(...args: unknown[]): Promise<unknown> // permission: speech:tts
    tts(...args: unknown[]): Promise<unknown> // permission: speech:tts
  }
  tappList: {
    export(...args: unknown[]): Promise<unknown> // permission: tappList:manage
    get(...args: unknown[]): Promise<unknown> // permission: tappList:read
    getInstallPackage(...args: unknown[]): Promise<unknown> // permission: tappList:read
    getRecent(...args: unknown[]): Promise<unknown> // permission: tappList:read
    install(...args: unknown[]): Promise<unknown> // permission: tappList:manage
    list(...args: unknown[]): Promise<unknown> // permission: tappList:read
    resolveStoreSource(...args: unknown[]): Promise<unknown> // permission: tappList:read
    start(...args: unknown[]): Promise<unknown> // permission: tappList:manage
    stop(...args: unknown[]): Promise<unknown> // permission: tappList:manage
    uninstall(...args: unknown[]): Promise<unknown> // permission: tappList:manage
  }
  user: {
    canUsePermissionLevel(...args: unknown[]): Promise<unknown>
    getAllowedPermissionLevels(...args: unknown[]): Promise<unknown>
    getRole(...args: unknown[]): Promise<unknown>
    isAdmin(...args: unknown[]): Promise<unknown>
    isGuest(...args: unknown[]): Promise<unknown>
    isLoggedIn(...args: unknown[]): Promise<unknown>
  }
  widget: {
    invalidate(...args: unknown[]): Promise<unknown>
    listRegistered(...args: unknown[]): Promise<unknown> // permission: widget:register
    register(...args: unknown[]): Promise<unknown> // permission: widget:register
    unregister(...args: unknown[]): Promise<unknown> // permission: widget:register
    updateConfig(...args: unknown[]): Promise<unknown> // permission: widget:register
    instanceSettings: {
      update(...args: unknown[]): Promise<unknown>
    }
  }
}

declare const Tapp: TappSdk

interface Window {
  Tapp: TappSdk
}

export {}
