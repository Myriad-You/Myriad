import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

describe('long-task guards', () => {
  it('observes longtask while the perf monitor is collapsed', () => {
    const metrics = readFileSync(
      new URL('../hooks/usePerfMetrics.ts', import.meta.url),
      'utf8',
    )
    assert.match(metrics, /type: 'longtask', buffered: true/)
    assert.match(metrics, /type: 'long-animation-frame', buffered: true/)
    const longtaskAt = metrics.indexOf("type: 'longtask', buffered: true")
    const expandedGateAt = metrics.indexOf(
      "if (!isExpanded || !('PerformanceObserver'",
    )
    assert.ok(longtaskAt >= 0)
    assert.ok(expandedGateAt > longtaskAt)
    const observe = metrics.slice(
      metrics.indexOf('recordTask'),
      metrics.indexOf("if (!isExpanded || !('PerformanceObserver'"),
    )
    assert.match(observe, /stabilityRef\.current =/)
    assert.equal(observe.includes('commit('), false)
    assert.equal(observe.includes('setSnapshot'), false)
    assert.match(metrics, /animations: prev.animations/)
  })

  it('does not drain idle or MessageChannel work in one timeout', () => {
    const core = readFileSync(
      new URL('../hooks/animation/core.ts', import.meta.url),
      'utf8',
    )
    const coordinator = readFileSync(
      new URL('../hooks/animation/coordinator.ts', import.meta.url),
      'utf8',
    )
    assert.match(core, /runIdleSlice/)
    assert.match(coordinator, /runIdleSlice/)
    assert.match(core, /TASK_FLUSH_BATCH/)
    assert.equal(core.includes('deadline.didTimeout)'), false)
    assert.equal(coordinator.includes('deadline.didTimeout)'), false)
  })

  it('does not keep an FPS rAF running on the production shell', () => {
    const layout = readFileSync(
      new URL('../layouts/AppLayout.tsx', import.meta.url),
      'utf8',
    )
    const metrics = readFileSync(
      new URL('../hooks/usePerfMetrics.ts', import.meta.url),
      'utf8',
    )
    const coordinator = readFileSync(
      new URL('../hooks/animation/coordinator.ts', import.meta.url),
      'utf8',
    )
    const core = readFileSync(
      new URL('../hooks/animation/core.ts', import.meta.url),
      'utf8',
    )
    const fitCss = readFileSync(
      new URL('../components/widgets/shared/FitText.css', import.meta.url),
      'utf8',
    )
    assert.equal(layout.includes('startFpsMonitor'), false)
    assert.match(metrics, /startFpsMonitor\(\)/)
    assert.match(coordinator, /pauseFpsMonitorLoop/)
    assert.match(coordinator, /coreIsPageVisible\(\)/)
    assert.match(core, /data-page-hidden/)
    assert.match(fitCss, /html\[data-page-hidden\]/)
    assert.match(fitCss, /\[data-offscreen\] \[data-fittext-track\]/)
  })

  it('yields code highlight, math, and feed color extraction', () => {
    const highlight = readFileSync(
      new URL('./codeHighlight.ts', import.meta.url),
      'utf8',
    )
    const math = readFileSync(
      new URL('../components/phantasi/notes/renderMath.ts', import.meta.url),
      'utf8',
    )
    const tile = readFileSync(
      new URL(
        '../components/phantasi/tiles/PhantasiSourceTile.tsx',
        import.meta.url,
      ),
      'utf8',
    )
    const social = readFileSync(
      new URL(
        '../components/widgets/reportCard/platforms/social.tsx',
        import.meta.url,
      ),
      'utf8',
    )
    assert.match(highlight, /yieldIfSliceExceeded/)
    assert.match(math, /yieldIfSliceExceeded/)
    assert.match(tile, /runWhenIdle/)
    assert.match(social, /runWhenIdle/)
    const fit = readFileSync(
      new URL('../hooks/useFitText.ts', import.meta.url),
      'utf8',
    )
    const scheduler = readFileSync(
      new URL('../hooks/fitTextScheduler.ts', import.meta.url),
      'utf8',
    )
    assert.match(fit, /scheduleFitText/)
    assert.match(fit, /fittingRef\.current/)
    assert.match(scheduler, /FIT_TEXT_SLICE_MS/)
    const extract = readFileSync(
      new URL('./colorExtractor.ts', import.meta.url),
      'utf8',
    )
    assert.match(extract, /await yieldToMain\(\)/)
    const routes = readFileSync(
      new URL('./codeSplitting.ts', import.meta.url),
      'utf8',
    )
    assert.match(routes, /await yieldToMain\(\)/)
  })

  it('slices RSS sanitizing and whole-document visual HTML', () => {
    const rss = readFileSync(
      new URL('./rssContentProcessor.ts', import.meta.url),
      'utf8',
    )
    const visual = readFileSync(
      new URL('../components/phantasi/notes/noteVisual.ts', import.meta.url),
      'utf8',
    )
    const render = readFileSync(
      new URL('../components/phantasi/reader/contentRender.ts', import.meta.url),
      'utf8',
    )
    const preview = readFileSync(
      new URL(
        '../components/phantasi/notes/useNoteEditorPreview.ts',
        import.meta.url,
      ),
      'utf8',
    )
    assert.match(rss, /export async function processRssContentAsync/)
    assert.match(rss, /yieldIfSliceExceeded/)
    assert.match(visual, /export async function markdownToVisualHtmlAsync/)
    assert.match(visual, /VISUAL_HTML_SYNC_CHARS/)
    assert.match(render, /processRssContentAsync/)
    assert.match(preview, /markdownToVisualHtmlAsync/)
    assert.match(preview, /VISUAL_HTML_SYNC_CHARS/)
  })
})
