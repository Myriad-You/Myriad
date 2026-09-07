import type { WidgetConfig, WidgetSize, WidgetType } from './widgetGridTypes'

export function widgetHostConfig(
  widgetTypeId: string,
): { platformId: string } | undefined {
  if (widgetTypeId.startsWith('platform-')) {
    return { platformId: widgetTypeId.slice('platform-'.length) }
  }
  if (widgetTypeId.startsWith('report-')) {
    return { platformId: widgetTypeId.slice('report-'.length) }
  }
  return undefined
}

export function widgetPreviewConfig(widgetType: {
  id: string
  defaultSize: WidgetSize
}): WidgetConfig {
  return {
    id: `preview-${widgetType.id}`,
    type: widgetType.id,
    size: widgetType.defaultSize,
    position: { x: 0, y: 0 },
    config: widgetHostConfig(widgetType.id),
  }
}

export function widgetTranslationKey(id: string): string {
  return id.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase())
}

export function widgetDisplayLabel(
  widgetType: Pick<WidgetType, 'id' | 'name'>,
  widgetsI18n: Record<string, unknown>,
): string {
  const translated = widgetsI18n[widgetTranslationKey(widgetType.id)]
  return typeof translated === 'string' && translated.trim()
    ? translated
    : widgetType.name
}

export function widgetSearchExtras(
  widgetType: Pick<WidgetType, 'category' | 'tappId' | 'description'>,
): Array<string | null | undefined> {
  return [widgetType.category, widgetType.tappId, widgetType.description]
}

export function widgetLibraryKindSource(
  widgetType: Pick<WidgetType, 'id' | 'isTappWidget' | 'category'>,
): {
  id: string
  isTappWidget?: boolean
  category?: string
} {
  return {
    id: widgetType.id,
    isTappWidget: widgetType.isTappWidget,
    category: widgetType.category,
  }
}
