import type { TappInstance } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import { getTappRuntime } from '../../TappRuntime'
import { emitTappWidgetInvalidate } from '../../WidgetRuntimeSignals'
import {
  isLocalWidgetIdOfTapp,
  parseWidgetInvalidateTargetArgs,
  tryAcceptWidgetInvalidateTarget,
  WIDGET_INVALIDATE_TARGET_TAPP_MAX_PER_MINUTE,
} from '../../widgetInvalidateTarget'

export function registerWidgetInvalidateTargetHandler(
  bridge: TappBridge,
  tappInstance: TappInstance,
  options: { preview?: boolean } = {},
): void {
  bridge.registerHandler('widget.invalidateTarget', async (message) => {
    const args = (message.payload as { args?: unknown[] }).args || []
    const parsed = parseWidgetInvalidateTargetArgs(args)
    if (!parsed.ok) {
      return { success: false, error: parsed.error, code: 'INVALID_REQUEST' }
    }

    // Preview must not remount installed Dashboard cards in this tab.
    if (options.preview || tappInstance.previewMode) {
      return { success: true, data: null }
    }

    const runtime = getTappRuntime()
    const registeredLocalIds = runtime
      .getWidgetsByTapp(tappInstance.id)
      .map((widget) => widget.config.id)
      .filter((id): id is string => typeof id === 'string' && id.length > 0)
    const manifestIds = (tappInstance.manifest.widgets || []).map(
      (widget) => widget.id,
    )
    if (
      !isLocalWidgetIdOfTapp(parsed.widgetId, manifestIds, registeredLocalIds)
    ) {
      return {
        success: false,
        error: `Unknown widgetId for this Tapp: ${parsed.widgetId}`,
        code: 'INVALID_REQUEST',
      }
    }

    const accepted = tryAcceptWidgetInvalidateTarget(
      tappInstance.id,
      parsed.widgetId,
    )
    if (!accepted.ok) {
      const error =
        accepted.reason === 'cooldown'
          ? `Widget invalidate cooldown: wait ${Math.ceil(accepted.retryAfterMs / 1000)}s before targeting this widget again`
          : `Widget invalidate budget exceeded: at most ${WIDGET_INVALIDATE_TARGET_TAPP_MAX_PER_MINUTE} targeted invalidations per minute`
      return {
        success: false,
        error,
        code: 'RATE_LIMITED',
        retryAfter: accepted.retryAfterMs,
      }
    }

    emitTappWidgetInvalidate({
      tappId: tappInstance.id,
      widgetId: parsed.widgetId,
      reason: parsed.reason,
      source: bridge,
    })
    return { success: true, data: null }
  })
}
