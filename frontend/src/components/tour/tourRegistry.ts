import type {
  TourDefinition,
  TourStepDef,
  TourSurface,
  TourSurfacePick,
} from './tourTypes'
import { pickTour } from './tourLogic'

function pair(
  page: string,
  route: string,
  steps: readonly TourStepDef[],
  surface?: TourSurface,
): readonly TourDefinition[] {
  return [
    {
      id: `${page}-visitor`,
      route,
      audience: 'visitor',
      ...(surface ? { surface } : {}),
      steps: [...steps],
    },
    {
      id: `${page}-owner`,
      route,
      audience: 'owner',
      ...(surface ? { surface } : {}),
      steps: [...steps],
    },
  ]
}

export const HOME_TOURS: readonly TourDefinition[] = [
  {
    id: 'home-visitor',
    route: '/',
    audience: 'visitor',
    steps: [
      { id: 'nav', anchor: 'nav' },
      { id: 'home-grid', anchor: 'home-grid' },
      { id: 'home-agent', anchor: 'home-agent', action: 'open-agent' },
      { id: 'home-agent-panel', anchor: 'home-agent-panel', after: 'open-agent' },
      { id: 'control-island', anchor: 'control-island' },
      { id: 'control-panel', anchor: 'control-panel' },
    ],
  },
  {
    id: 'home-owner',
    route: '/',
    audience: 'owner',
    steps: [
      { id: 'nav', anchor: 'nav' },
      { id: 'home-grid', anchor: 'home-grid' },
      { id: 'home-edit', anchor: 'home-edit' },
      { id: 'home-agent', anchor: 'home-agent', action: 'open-agent' },
      { id: 'home-agent-panel', anchor: 'home-agent-panel', after: 'open-agent' },
      { id: 'control-island', anchor: 'control-island' },
      { id: 'control-panel-owner', anchor: 'control-panel' },
    ],
  },
  {
    id: 'home-edit-owner',
    route: '/',
    audience: 'owner',
    surface: 'edit',
    steps: [
      { id: 'home-edit-grid', anchor: 'home-grid' },
      { id: 'home-widget-library', anchor: 'home-widget-library' },
      { id: 'home-free-layout', anchor: 'home-free-layout' },
      { id: 'home-sticker', anchor: 'home-sticker' },
    ],
  },
]

export const LIBRARY_TOURS: readonly TourDefinition[] = [
  ...pair('library', '/library', [
    { id: 'library-filters', anchor: 'library-filters' },
    { id: 'library-grid', anchor: 'library-grid' },
    { id: 'library-card', anchor: 'library-card' },
  ]),
  ...pair(
    'library-canvas',
    '/library',
    [
      { id: 'library-filters', anchor: 'library-filters' },
      { id: 'library-grid-canvas', anchor: 'library-grid' },
      { id: 'library-card', anchor: 'library-card' },
      { id: 'library-canvas', anchor: 'library-canvas' },
    ],
    'canvas',
  ),
]

export const REPORTS_TOURS: readonly TourDefinition[] = [
  {
    id: 'reports-visitor',
    route: '/reports',
    audience: 'visitor',
    steps: [
      { id: 'reports-status', anchor: 'reports-status' },
      { id: 'reports-play', anchor: 'reports-play' },
      { id: 'reports-cards', anchor: 'reports-cards' },
    ],
  },
  {
    id: 'reports-owner',
    route: '/reports',
    audience: 'owner',
    steps: [
      { id: 'reports-status', anchor: 'reports-status' },
      { id: 'reports-play', anchor: 'reports-play' },
      { id: 'reports-cards-owner', anchor: 'reports-cards' },
    ],
  },
]

export const TAPP_TOURS: readonly TourDefinition[] = [
  {
    id: 'tapp-visitor',
    route: '/tapp',
    audience: 'visitor',
    steps: [
      { id: 'tapp-store-entry', anchor: 'tapp-store-entry' },
      { id: 'tapp-scope', anchor: 'tapp-scope' },
      { id: 'tapp-grid', anchor: 'tapp-grid' },
    ],
  },
  {
    id: 'tapp-owner',
    route: '/tapp',
    audience: 'owner',
    steps: [
      { id: 'tapp-store-entry', anchor: 'tapp-store-entry' },
      { id: 'tapp-open-playground', anchor: 'tapp-open-playground' },
      { id: 'tapp-install', anchor: 'tapp-install' },
      { id: 'tapp-grid-owner', anchor: 'tapp-grid' },
    ],
  },
]

export const TAPP_DETAIL_TOURS: readonly TourDefinition[] = pair(
  'tapp-detail',
  '/tapp/detail',
  [
    { id: 'tapp-detail-overview', anchor: 'tapp-detail-overview' },
    { id: 'tapp-detail-settings', anchor: 'tapp-detail-settings' },
    { id: 'tapp-detail-permissions', anchor: 'tapp-detail-permissions' },
  ],
).map((tour) => ({ ...tour, matchPrefix: true }))

export const TAPP_PLAYGROUND_TOURS: readonly TourDefinition[] = [
  {
    id: 'tapp-playground-owner',
    route: '/tapp/playground',
    audience: 'owner',
    steps: [
      { id: 'tapp-playground-toolbar', anchor: 'tapp-playground-toolbar' },
      { id: 'tapp-playground-preview', anchor: 'tapp-playground-preview' },
      { id: 'tapp-playground-widget', anchor: 'tapp-playground-widget' },
      { id: 'tapp-playground-code', anchor: 'tapp-playground-code' },
      { id: 'tapp-playground-prompt', anchor: 'tapp-playground-prompt' },
      { id: 'tapp-playground-generate', anchor: 'tapp-playground-generate' },
      { id: 'tapp-playground-history', anchor: 'tapp-playground-history' },
      { id: 'tapp-playground-revisions', anchor: 'tapp-playground-revisions' },
      { id: 'tapp-playground-clear', anchor: 'tapp-playground-clear' },
      { id: 'tapp-playground-export', anchor: 'tapp-playground-export' },
      { id: 'tapp-playground-install', anchor: 'tapp-playground-install' },
    ],
  },
]

export const CONFIG_TOURS: readonly TourDefinition[] = [
  {
    id: 'config-owner',
    route: '/config',
    audience: 'owner',
    steps: [
      { id: 'config-search', anchor: 'config-search' },
      { id: 'config-sidebar', anchor: 'config-sidebar' },
      { id: 'config-content', anchor: 'config-content' },
    ],
  },
]

export const CONFIG_AI_PERSONA_TOURS: readonly TourDefinition[] = [
  {
    id: 'config-ai-persona-owner',
    route: '/config',
    audience: 'owner',
    surface: 'ai-persona',
    steps: [
      { id: 'config-ai-persona-toggle', anchor: 'config-ai-persona-toggle' },
      { id: 'config-ai-persona-card', anchor: 'config-ai-persona-card' },
      { id: 'config-ai-persona-speech', anchor: 'config-ai-persona-speech' },
    ],
  },
]

export const CONFIG_PERSONA_TOURS: readonly TourDefinition[] = [
  {
    id: 'config-persona-owner',
    route: '/config',
    audience: 'owner',
    surface: 'persona',
    steps: [
      { id: 'config-persona-tabs', anchor: 'config-persona-tabs' },
      { id: 'config-persona-portrait', anchor: 'config-persona-portrait' },
      { id: 'config-persona-overview', anchor: 'config-persona-overview' },
      { id: 'config-persona-identity', anchor: 'config-persona-identity' },
      { id: 'config-persona-wardrobe', anchor: 'config-persona-wardrobe' },
      { id: 'config-persona-motion', anchor: 'config-persona-motion' },
    ],
  },
]

export const TOURS: readonly TourDefinition[] = [
  ...HOME_TOURS,
  ...LIBRARY_TOURS,
  ...REPORTS_TOURS,
  ...TAPP_TOURS,
  ...TAPP_DETAIL_TOURS,
  ...TAPP_PLAYGROUND_TOURS,
  ...CONFIG_TOURS,
  ...CONFIG_AI_PERSONA_TOURS,
  ...CONFIG_PERSONA_TOURS,
]

export function pickRegisteredTour(
  pathname: string,
  isOwner: boolean,
  surface: TourSurfacePick = 'browse',
): TourDefinition | null {
  return pickTour(TOURS, pathname, isOwner, surface)
}
