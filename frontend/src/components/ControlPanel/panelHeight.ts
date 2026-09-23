import { observeResize } from '../../hooks/animation'
import { emitAppEvent } from '../../utils/appEvents'
import { CONTROL_PANEL_HEIGHT_COMPENSATION } from '../../utils/libraryDockStage'

/** Follow content geometry without a timer lag or interrupting the opening morph. */
export function trackPanelHeight(
  shell: HTMLElement,
  content: HTMLElement,
  isMorphing: () => boolean,
): () => void {
  let lastHeight = 0
  let frame: number | null = null
  let disposed = false

  function cancelMeasure() {
    if (frame !== null) cancelAnimationFrame(frame)
    frame = null
  }

  function measure(duringMorph = false) {
    cancelMeasure()
    if (disposed || document.hidden || (isMorphing() && !duringMorph)) return

    // Content width stays at its expanded value throughout the shell morph.
    // Reading the actual layout avoids cloning the panel into the document.
    if (import.meta.env.DEV) {
      const rootFontSize = Number.parseFloat(getComputedStyle(document.documentElement).fontSize)
      const expectedWidth = window.innerWidth <= 640
        ? window.innerWidth - 4.25 * rootFontSize
        : 356
      if (Math.abs(content.offsetWidth - expectedWidth) > 2) {
        console.warn(
          `[GlobalControlPanel] 面板内容宽度 ${content.offsetWidth}px 偏离预期终值 ${Math.round(expectedWidth)}px：` +
          'scrollHeight 高度测量依赖 .expanded-panel-content 的固定宽度契约' +
          '（GlobalControlPanel.css），请勿改回 width: 100% 或移除 flex-shrink: 0',
        )
      }
    }

    const height = Math.ceil(content.scrollHeight * CONTROL_PANEL_HEIGHT_COMPENSATION)
    if (height !== lastHeight) {
      lastHeight = height
      shell.style.height = `${height}px`
      // The measured panel owns geometry notifications, including music and
      // notification content. Children must not start a second resize pipeline.
      emitAppEvent('control-panel-content-resize')
    }
  }

  function scheduleMeasure() {
    if (disposed || document.hidden || isMorphing() || frame !== null) return
    frame = requestAnimationFrame(() => measure())
  }

  // Initial target and morph-end correction are synchronous so the transition
  // always receives its final geometry before painting.
  measure(true)
  const handleAnimationEnd = () => measure(true)
  const handleRemeasure = (event: Event) => {
    const detail = (event as CustomEvent<{ immediate?: boolean } | undefined>).detail
    if (detail?.immediate) measure()
    else scheduleMeasure()
  }
  const handleVisibility = () => {
    if (document.hidden) cancelMeasure()
    else scheduleMeasure()
  }

  // The shared observer already delivers on a frame; do not add a second rAF.
  // Keep observing on mobile and reduced-motion settings: geometry is essential.
  const unobserve = observeResize(content, () => measure())
  const mutations = new MutationObserver(scheduleMeasure)
  mutations.observe(content, { childList: true, subtree: true, characterData: true })
  window.addEventListener('gcp-animation-end', handleAnimationEnd)
  window.addEventListener('gcp-remeasure', handleRemeasure)
  window.addEventListener('resize', scheduleMeasure)
  window.addEventListener('orientationchange', scheduleMeasure)
  document.addEventListener('visibilitychange', handleVisibility)

  return () => {
    disposed = true
    cancelMeasure()
    unobserve()
    mutations.disconnect()
    window.removeEventListener('gcp-animation-end', handleAnimationEnd)
    window.removeEventListener('gcp-remeasure', handleRemeasure)
    window.removeEventListener('resize', scheduleMeasure)
    window.removeEventListener('orientationchange', scheduleMeasure)
    document.removeEventListener('visibilitychange', handleVisibility)
  }
}
