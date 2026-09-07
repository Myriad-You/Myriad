import type { TourDefinition } from './tourTypes'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  computeTourCardPosition,
  fillTourHint,
  getLibraryTourSurfaceSnapshot,
  homeBrowseTourPanelPose,
  libraryTourSurfaceFromFlags,
  homeEditTourDockPose,
  personaTourPanel,
  isPredictedTourAnchor,
  predictedControlIslandTourBox,
  predictedControlPanelTourBox,
  predictedLibraryDockTourBox,
  sameTourHole,
  tourHoleSync,
  filterVisibleSteps,
  fitTourUnion,
  firstVisibleIndex,
  HOME_AGENT_PRESS_REM,
  homeAgentPressInsets,
  isHomeAgentActionStep,
  isTourActionSatisfied,
  nextIndexAfterTourAction,
  pickHomeAgentPressBox,
  holePadForBox,
  holePadForTourAnchor,
  inflateRect,
  intersectBoxes,
  isDegenerateBox,
  normalizeTourPath,
  pageNameForPath,
  pickLargestVisible,
  readTourSurface,
  pickTour,
  tourAnchorNeedsReveal,
  shouldAbortLibraryTour,
  tourMeasureWatchesHost,
  tourMeasureWatchesScroll,
  waitForTourAnchor,
  previousVisibleIndex,
  TOUR_HOLE_PAD,
  tourStepBlocksAdvance,
  unionBoxes,
} from './tourLogic'
import {
  CONFIG_AI_PERSONA_TOURS,
  CONFIG_PERSONA_TOURS,
  CONFIG_TOURS,
  HOME_TOURS,
  LIBRARY_TOURS,
  REPORTS_TOURS,
  TAPP_DETAIL_TOURS,
  TAPP_PLAYGROUND_TOURS,
  TAPP_TOURS,
  TOURS,
} from './tourRegistry'

describe('inflateRect', () => {
  it('pads every edge', () => {
    const next = inflateRect({ top: 40, left: 20, width: 100, height: 50 }, 8)
    assert.deepEqual(next, {
      top: 32,
      left: 12,
      width: 116,
      height: 66,
      right: 128,
      bottom: 98,
    })
  })

  it('uses TOUR_HOLE_PAD by default', () => {
    const next = inflateRect({ top: 10, left: 10, width: 10, height: 10 })
    assert.equal(next.width, 10 + TOUR_HOLE_PAD * 2)
  })
})

describe('filterVisibleSteps', () => {
  it('drops missing anchors', () => {
    const present = new Set(['nav', 'home-grid'])
    const next = filterVisibleSteps(
      [
        { id: 'nav', anchor: 'nav' },
        { id: 'edit', anchor: 'home-edit' },
        { id: 'grid', anchor: 'home-grid' },
      ],
      (anchor) => present.has(anchor),
    )
    assert.deepEqual(
      next.map((step) => step.id),
      ['nav', 'grid'],
    )
  })
})

describe('visible index walk', () => {
  const steps = [
    { id: 'a', anchor: 'a' },
    { id: 'b', anchor: 'b' },
    { id: 'c', anchor: 'c' },
  ]
  const has = (anchor: string) => anchor !== 'b'

  it('skips missing when walking forward', () => {
    assert.equal(firstVisibleIndex(steps, has, 1), 2)
  })

  it('skips missing when walking back', () => {
    assert.equal(previousVisibleIndex(steps, has, 2), 0)
  })

  it('returns -1 when nothing remains', () => {
    assert.equal(
      firstVisibleIndex(steps, () => false, 0),
      -1,
    )
  })
})

describe('normalizeTourPath', () => {
  it('keeps root', () => {
    assert.equal(normalizeTourPath('/'), '/')
  })

  it('strips trailing slashes', () => {
    assert.equal(normalizeTourPath('/library/'), '/library')
  })
})

describe('pickTour', () => {
  it('selects visitor home tour', () => {
    const tour = pickTour(HOME_TOURS, '/', false)
    assert.equal(tour?.id, 'home-visitor')
    assert.deepEqual(
      tour?.steps.map((step) => step.anchor),
      [
        'nav',
        'home-grid',
        'home-agent',
        'home-agent-panel',
        'control-island',
        'control-panel',
      ],
    )
  })

  it('selects owner home tour', () => {
    const tour = pickTour(HOME_TOURS, '/', true)
    assert.equal(tour?.id, 'home-owner')
    assert.equal(tour?.steps.at(-1)?.anchor, 'control-panel')
  })

  it('selects a separate owner tour on the edit surface', () => {
    const tour = pickTour(HOME_TOURS, '/', true, 'edit')
    assert.equal(tour?.id, 'home-edit-owner')
    assert.deepEqual(
      tour?.steps.map((step) => step.anchor),
      [
        'home-grid',
        'home-widget-library',
        'home-free-layout',
        'home-sticker',
      ],
    )
    assert.equal(pickTour(HOME_TOURS, '/', false, 'edit'), null)
    assert.equal(
      tour?.steps.some(
        (step) => step.anchor === 'nav' || step.anchor === 'control-island',
      ),
      false,
    )
  })

  it('returns null when the page has no tour', () => {
    assert.equal(pickTour(HOME_TOURS, '/library', true), null)
  })

  it('prefers exact route over a prefix match', () => {
    const tours = [
      {
        id: 'list',
        route: '/tapp',
        audience: 'visitor' as const,
        steps: [{ id: 'a', anchor: 'a' }],
      },
      {
        id: 'detail',
        route: '/tapp/detail',
        matchPrefix: true,
        audience: 'visitor' as const,
        steps: [{ id: 'b', anchor: 'b' }],
      },
    ]
    assert.equal(pickTour(tours, '/tapp', false)?.id, 'list')
    assert.equal(pickTour(tours, '/tapp/detail/x', false)?.id, 'detail')
    assert.equal(pickTour(tours, '/tapp/store', false), null)
  })
})

describe('HOME_TOURS', () => {
  it('keeps owner browse, owner edit, and visitor as separate definitions', () => {
    const ids = HOME_TOURS.map((tour: TourDefinition) => tour.id)
    assert.deepEqual(ids, ['home-visitor', 'home-owner', 'home-edit-owner'])
  })

  it('keeps the edit tour as grid, library, free layout, then stickers', () => {
    const tour = pickTour(HOME_TOURS, '/', true, 'edit')
    assert.deepEqual(
      tour?.steps.map((step) => step.id),
      [
        'home-edit-grid',
        'home-widget-library',
        'home-free-layout',
        'home-sticker',
      ],
    )
    assert.equal(
      tour?.steps.some((step) => step.action),
      false,
    )
  })

  it('keeps Agent after the grid, as the first action step', () => {
    const visitor = pickTour(HOME_TOURS, '/', false)
    const owner = pickTour(HOME_TOURS, '/', true)
    assert.equal(visitor?.steps[0]?.id, 'nav')
    assert.equal(visitor?.steps[2]?.id, 'home-agent')
    assert.equal(visitor?.steps[2]?.action, 'open-agent')
    assert.equal(visitor?.steps[3]?.id, 'home-agent-panel')
    assert.equal(visitor?.steps[3]?.after, 'open-agent')
    assert.equal(owner?.steps[3]?.id, 'home-agent')
    assert.equal(owner?.steps[3]?.action, 'open-agent')
    assert.equal(
      pickTour(HOME_TOURS, '/', true, 'edit')?.steps.some(
        (step) => step.action === 'open-agent',
      ),
      false,
    )
  })
})

describe('tour action gate', () => {
  it('blocks advance until the action is satisfied', () => {
    const step = { id: 'home-agent', anchor: 'home-agent', action: 'open-agent' as const }
    assert.equal(tourStepBlocksAdvance(step, () => false), true)
    assert.equal(isTourActionSatisfied(step, () => false), false)
    assert.equal(tourStepBlocksAdvance(step, () => true), false)
    assert.equal(isTourActionSatisfied({ id: 'nav', anchor: 'nav' }, () => false), true)
  })

  it('waits for the follow-up step instead of skipping past it', () => {
    const steps = [
      { id: 'home-agent', anchor: 'home-agent', action: 'open-agent' as const },
      {
        id: 'home-agent-panel',
        anchor: 'home-agent-panel',
        after: 'open-agent' as const,
      },
      { id: 'control-island', anchor: 'control-island' },
    ]
    const ready = new Set(['home-agent', 'control-island'])
    assert.equal(
      nextIndexAfterTourAction(
        steps,
        0,
        (step) => ready.has(step.anchor),
        () => true,
      ),
      'wait',
    )
    ready.add('home-agent-panel')
    assert.equal(
      nextIndexAfterTourAction(
        steps,
        0,
        (step) => ready.has(step.anchor),
        () => true,
      ),
      1,
    )
  })

  it('skips the follow-up when the action host is missing', () => {
    const steps = [
      { id: 'home-agent', anchor: 'home-agent', action: 'open-agent' as const },
      {
        id: 'home-agent-panel',
        anchor: 'home-agent-panel',
        after: 'open-agent' as const,
      },
      { id: 'control-island', anchor: 'control-island' },
    ]
    assert.equal(
      nextIndexAfterTourAction(
        steps,
        0,
        (step) => step.anchor === 'control-island',
        () => false,
      ),
      2,
    )
  })
})

describe('LIBRARY_TOURS', () => {
  it('keeps list and canvas as separate definitions', () => {
    const ids = LIBRARY_TOURS.map((tour: TourDefinition) => tour.id)
    assert.deepEqual(ids, [
      'library-visitor',
      'library-owner',
      'library-canvas-visitor',
      'library-canvas-owner',
    ])
  })
})

describe('tour hint copy', () => {
  const nav = {
    home: '首页',
    library: '资料库',
    brew: 'Brew',
    reports: '报告',
    tapp: 'Tapp',
    config: '配置',
  }

  it('names the home edit surface', () => {
    assert.equal(pageNameForPath('/', nav, '编辑模式', true), '编辑模式')
    assert.equal(pageNameForPath('/', nav, '编辑模式', false), '首页')
  })

  it('keeps config persona surfaces on /config only', () => {
    assert.equal(readTourSurface(true, '/'), 'edit')
    assert.equal(readTourSurface(false, '/library'), 'browse')
    assert.equal(readTourSurface(false, '/tapp'), 'browse')
    assert.equal(getLibraryTourSurfaceSnapshot(), 'list')
    assert.equal(
      libraryTourSurfaceFromFlags({
        preference: false,
        surface: false,
        empty: false,
      }),
      'list',
    )
    assert.equal(
      libraryTourSurfaceFromFlags({
        preference: true,
        surface: false,
        empty: false,
      }),
      'pending',
    )
    assert.equal(
      libraryTourSurfaceFromFlags({
        preference: true,
        surface: false,
        empty: true,
      }),
      'empty',
    )
    assert.equal(
      libraryTourSurfaceFromFlags({
        preference: true,
        surface: true,
        empty: false,
      }),
      'canvas',
    )
  })

  it('skips live measure on the library collection hole only', () => {
    assert.equal(tourMeasureWatchesHost('library-grid'), false)
    assert.equal(tourMeasureWatchesHost('home-grid'), true)
    assert.equal(tourMeasureWatchesHost('tapp-grid'), true)
    assert.equal(tourMeasureWatchesHost('reports-cards'), true)
    assert.equal(tourMeasureWatchesHost('library-card'), true)
    assert.equal(tourMeasureWatchesScroll(true), false)
    assert.equal(tourMeasureWatchesScroll(false), true)
  })

  it('aborts a canvas tour once the live canvas is gone', () => {
    assert.equal(shouldAbortLibraryTour('library-owner', 'canvas'), true)
    assert.equal(shouldAbortLibraryTour('library-visitor', 'pending'), false)
    assert.equal(shouldAbortLibraryTour('library-canvas-owner', 'canvas'), false)
    assert.equal(shouldAbortLibraryTour('library-canvas-owner', 'pending'), true)
    assert.equal(shouldAbortLibraryTour('library-canvas-visitor', 'empty'), true)
    assert.equal(shouldAbortLibraryTour('library-canvas-visitor', 'list'), true)
  })

  it('selects a separate library tour on the canvas surface', () => {
    assert.equal(
      pickTour(LIBRARY_TOURS, '/library', false, 'canvas')?.id,
      'library-canvas-visitor',
    )
    assert.equal(
      pickTour(LIBRARY_TOURS, '/library', true, 'canvas')?.id,
      'library-canvas-owner',
    )
    assert.equal(pickTour(LIBRARY_TOURS, '/library', true)?.id, 'library-owner')
  })

  it('fills {page} and drops empty values so the hint can fall back', () => {
    assert.equal(
      fillTourHint('了解一下 {page}', 'page', '编辑模式'),
      '了解一下 编辑模式',
    )
    assert.equal(fillTourHint('了解一下 {page}', 'page', ''), '')
    assert.equal(fillTourHint('了解一下 {page}', 'page', undefined), '')
  })

  it('opens the widget library dock only on that edit-tour step', () => {
    assert.equal(homeEditTourDockPose(null, 'home-widget-library'), undefined)
    assert.equal(
      homeEditTourDockPose('home-edit-owner', 'home-edit-grid'),
      'parked',
    )
    assert.equal(
      homeEditTourDockPose('home-edit-owner', 'home-widget-library'),
      'restored',
    )
    assert.equal(
      homeEditTourDockPose('home-edit-owner', 'home-free-layout'),
      'parked',
    )
  })

  it('expands the control panel only on that browse-tour step', () => {
    assert.equal(homeBrowseTourPanelPose(null, 'control-panel'), undefined)
    assert.equal(homeBrowseTourPanelPose('home-visitor', 'nav'), 'collapsed')
    assert.equal(
      homeBrowseTourPanelPose('home-visitor', 'control-panel'),
      'expanded',
    )
    assert.equal(
      homeBrowseTourPanelPose('home-owner', 'control-island'),
      'collapsed',
    )
    assert.equal(
      homeBrowseTourPanelPose('home-owner', 'control-panel-owner'),
      'expanded',
    )
    assert.equal(
      homeBrowseTourPanelPose('home-edit-owner', 'home-edit-grid'),
      undefined,
    )
  })

  it('switches the persona workbench tab with the tour step', () => {
    assert.equal(personaTourPanel(null, 'config-persona-identity'), undefined)
    assert.equal(
      personaTourPanel('config-persona-owner', 'config-persona-tabs'),
      'overview',
    )
    assert.equal(
      personaTourPanel('config-persona-owner', 'config-persona-overview'),
      'overview',
    )
    assert.equal(
      personaTourPanel('config-persona-owner', 'config-persona-identity'),
      'persona',
    )
    assert.equal(
      personaTourPanel('config-persona-owner', 'config-persona-wardrobe'),
      'wardrobe',
    )
    assert.equal(
      personaTourPanel('config-persona-owner', 'config-persona-motion'),
      'motion',
    )
    assert.equal(
      personaTourPanel('config-owner', 'config-persona-identity'),
      undefined,
    )
  })

  it('predicts the restored library box without reading transforms', () => {
    const box = predictedLibraryDockTourBox(1920, 1080, 16)
    assert.equal(box.width, 64 * 16 * (5 / 6))
    assert.equal(box.height, 40 * 16 * (5 / 6))
    assert.equal(box.left, 1920 / 2 - box.width / 2)
    assert.equal(box.top, 1080 - 5.25 * 16 - box.height)
  })

  it('predicts the expanded control-panel shell without reading morph', () => {
    const box = predictedControlPanelTourBox(1920, 480, 16)
    assert.equal(box.width, 400)
    assert.equal(box.left, 1920 - 16 - 400)
    assert.equal(box.top, 16)
    assert.equal(box.height, Math.ceil(480 * 1.08))
  })

  it('predicts the collapsed control-island shell without reading morph', () => {
    const box = predictedControlIslandTourBox(1920, 16)
    assert.equal(box.width, 160)
    assert.equal(box.height, 48)
    assert.equal(box.left, 1920 - 16 - 160)
    assert.equal(box.top, 16)
  })

  it('treats library and control chrome as predicted hole sources', () => {
    assert.equal(isPredictedTourAnchor('control-panel'), true)
    assert.equal(isPredictedTourAnchor('control-island'), true)
    assert.equal(isPredictedTourAnchor('home-widget-library'), true)
    assert.equal(isPredictedTourAnchor('home-grid', 'home-widget-library'), true)
    assert.equal(isPredictedTourAnchor('nav'), false)
    assert.equal(tourHoleSync({ id: 'home-widget-library', anchor: 'home-widget-library' }), 'dock')
    assert.equal(
      tourHoleSync({ id: 'control-panel', anchor: 'control-panel' }),
      'panel',
    )
    assert.equal(
      tourHoleSync({ id: 'control-island', anchor: 'control-island' }),
      undefined,
    )
    assert.equal(tourHoleSync({ id: 'nav', anchor: 'nav' }), undefined)
  })

  it('skips hole writes that only differ by subpixels', () => {
    const hole = { top: 16, left: 1504, width: 400, height: 551, radius: 24 }
    assert.equal(
      sameTourHole(hole, { ...hole, left: 1504.2, height: 551.4 }),
      true,
    )
    assert.equal(sameTourHole(hole, { ...hole, width: 180 }), false)
  })
})

describe('page tours', () => {
  it('registers library, reports, tapp, and owner-only config', () => {
    assert.equal(pickTour(LIBRARY_TOURS, '/library', false)?.id, 'library-visitor')
    assert.equal(pickTour(REPORTS_TOURS, '/reports', true)?.id, 'reports-owner')
    assert.equal(pickTour(TAPP_TOURS, '/tapp', false)?.id, 'tapp-visitor')
    assert.equal(pickTour(TOURS, '/tapp/store', true), null)
    assert.equal(pickTour(TOURS, '/tapp/store', false), null)
    assert.equal(pickTour(CONFIG_TOURS, '/config', true)?.id, 'config-owner')
    assert.equal(pickTour(CONFIG_TOURS, '/config', false), null)
    assert.equal(
      pickTour(CONFIG_AI_PERSONA_TOURS, '/config', true, 'ai-persona')?.id,
      'config-ai-persona-owner',
    )
    assert.equal(
      pickTour(CONFIG_PERSONA_TOURS, '/config', true, 'persona')?.id,
      'config-persona-owner',
    )
    assert.equal(pickTour(TOURS, '/config', true, 'none'), null)
    assert.equal(pickTour(TOURS, '/config', false, 'persona'), null)
    assert.equal(pickTour(TOURS, '/config', true, 'ai-persona')?.id, 'config-ai-persona-owner')
    assert.equal(pickTour(TOURS, '/config', true, 'persona')?.id, 'config-persona-owner')
    assert.equal(pickTour(TOURS, '/config', true)?.id, 'config-owner')
  })

  it('does not register brew reading routes', () => {
    assert.equal(pickTour(TOURS, '/brew', true), null)
    assert.equal(pickTour(TOURS, '/brew/item/1', true), null)
  })

  it('matches tapp detail by prefix and playground for owner only', () => {
    assert.equal(
      pickTour(TAPP_DETAIL_TOURS, '/tapp/detail/abc', false)?.id,
      'tapp-detail-visitor',
    )
    assert.equal(pickTour(TAPP_TOURS, '/tapp/detail/abc', false), null)
    assert.equal(
      pickTour(TAPP_PLAYGROUND_TOURS, '/tapp/playground', true)?.id,
      'tapp-playground-owner',
    )
    assert.equal(pickTour(TAPP_PLAYGROUND_TOURS, '/tapp/playground', false), null)
    assert.deepEqual(
      pickTour(TAPP_PLAYGROUND_TOURS, '/tapp/playground', true)?.steps.map(
        (step) => step.id,
      ),
      [
        'tapp-playground-toolbar',
        'tapp-playground-preview',
        'tapp-playground-widget',
        'tapp-playground-code',
        'tapp-playground-prompt',
        'tapp-playground-generate',
        'tapp-playground-history',
        'tapp-playground-revisions',
        'tapp-playground-clear',
        'tapp-playground-export',
        'tapp-playground-install',
      ],
    )
  })

  it('keeps nav and control island on home only', () => {
    assert.deepEqual(
      pickTour(HOME_TOURS, '/', true)?.steps.map((step) => step.anchor),
      [
        'nav',
        'home-grid',
        'home-edit',
        'home-agent',
        'home-agent-panel',
        'control-island',
        'control-panel',
      ],
    )
    assert.deepEqual(
      pickTour(HOME_TOURS, '/', false)?.steps.map((step) => step.id),
      [
        'nav',
        'home-grid',
        'home-agent',
        'home-agent-panel',
        'control-island',
        'control-panel',
      ],
    )
    assert.deepEqual(
      pickTour(LIBRARY_TOURS, '/library', true)?.steps.map((step) => step.anchor),
      ['library-filters', 'library-grid', 'library-card'],
    )
    assert.deepEqual(
      pickTour(LIBRARY_TOURS, '/library', true, 'canvas')?.steps.map(
        (step) => step.id,
      ),
      [
        'library-filters',
        'library-grid-canvas',
        'library-card',
        'library-canvas',
      ],
    )
    assert.equal(
      pickTour(LIBRARY_TOURS, '/library', true)?.steps.some(
        (step) => step.anchor === 'nav' || step.anchor === 'control-island',
      ),
      false,
    )
    assert.deepEqual(
      pickTour(TAPP_TOURS, '/tapp', false)?.steps.map((step) => step.anchor),
      ['tapp-store-entry', 'tapp-scope', 'tapp-grid'],
    )
    assert.deepEqual(
      pickTour(TAPP_TOURS, '/tapp', true)?.steps.map((step) => step.id),
      [
        'tapp-store-entry',
        'tapp-open-playground',
        'tapp-install',
        'tapp-grid-owner',
      ],
    )
    assert.equal(
      pickTour(REPORTS_TOURS, '/reports', false)?.steps.at(-1)?.id,
      'reports-cards',
    )
    assert.equal(
      pickTour(REPORTS_TOURS, '/reports', true)?.steps.at(-1)?.id,
      'reports-cards-owner',
    )
    assert.equal(
      pickTour(CONFIG_TOURS, '/config', true)?.steps.some(
        (step) => step.anchor === 'nav' || step.anchor === 'control-island',
      ),
      false,
    )
  })
})

describe('intersectBoxes', () => {
  it('clips an overflowing child back to the host', () => {
    const host = { top: 224, left: 328, width: 1264, height: 632 }
    const union = { top: 224, left: 328, width: 1264, height: 869 }
    assert.deepEqual(intersectBoxes(union, host), host)
  })

  it('returns null when boxes do not overlap', () => {
    assert.equal(
      intersectBoxes(
        { top: 0, left: 0, width: 10, height: 10 },
        { top: 40, left: 40, width: 10, height: 10 },
      ),
      null,
    )
  })
})

describe('fitTourUnion', () => {
  it('keeps the child union when the host has no box', () => {
    const fitted = fitTourUnion(
      [
        { top: 80, left: 40, width: 48, height: 48 },
        { top: 80, left: 100, width: 48, height: 48 },
      ],
      { top: 0, left: 0, width: 0, height: 0 },
    )
    assert.deepEqual(fitted, {
      top: 80,
      left: 40,
      width: 108,
      height: 48,
    })
  })

  it('drops off-canvas cards so the hole hugs the visible cluster', () => {
    const host = { top: 0, left: 0, width: 1280, height: 800 }
    const fitted = fitTourUnion(
      [
        { top: 220, left: 480, width: 200, height: 280 },
        { top: 240, left: 700, width: 160, height: 220 },
        { top: 180, left: 2200, width: 200, height: 280 },
      ],
      host,
    )
    assert.deepEqual(fitted, {
      top: 220,
      left: 480,
      width: 380,
      height: 280,
    })
  })
})

describe('unionBoxes', () => {
  it('wraps every widget rect', () => {
    const union = unionBoxes([
      { top: 200, left: 80, width: 200, height: 160 },
      { top: 200, left: 300, width: 400, height: 200 },
      { top: 380, left: 80, width: 200, height: 120 },
    ])
    assert.deepEqual(union, {
      top: 200,
      left: 80,
      width: 620,
      height: 300,
    })
  })

  it('ignores empty rects', () => {
    assert.equal(
      unionBoxes([{ top: 0, left: 0, width: 0, height: 10 }]),
      null,
    )
  })
})

describe('pickLargestVisible', () => {
  it('prefers the on-screen grid over a clipped right-edge sliver', () => {
    const panel = { id: 'panel', top: 80, left: 1200, width: 320, height: 200 }
    const home = { id: 'home', top: 220, left: 80, width: 1100, height: 480 }
    const picked = pickLargestVisible(
      [panel, home],
      (item) => item,
      1280,
      800,
    )
    assert.equal(picked?.id, 'home')
  })
})

describe('isDegenerateBox', () => {
  it('rejects a sliver', () => {
    assert.equal(
      isDegenerateBox({ top: 0, left: 1200, width: 8, height: 400 }),
      true,
    )
  })

  it('rejects the 40×22 toggle used on AI persona rows', () => {
    assert.equal(
      isDegenerateBox({ top: 80, left: 400, width: 40, height: 22 }),
      true,
    )
  })
})

describe('tourAnchorNeedsReveal', () => {
  it('leaves an on-screen control alone', () => {
    assert.equal(
      tourAnchorNeedsReveal(
        { top: 120, left: 80, width: 280, height: 48 },
        1280,
        800,
      ),
      false,
    )
  })

  it('asks to jump when the persona row is below the fold', () => {
    assert.equal(
      tourAnchorNeedsReveal(
        { top: 1400, left: 80, width: 280, height: 48 },
        1280,
        800,
      ),
      true,
    )
  })

  it('asks to jump when the row sits above or left of the viewport', () => {
    assert.equal(
      tourAnchorNeedsReveal(
        { top: -80, left: 80, width: 280, height: 48 },
        1280,
        800,
      ),
      true,
    )
    assert.equal(
      tourAnchorNeedsReveal(
        { top: 120, left: -200, width: 160, height: 48 },
        1280,
        800,
      ),
      true,
    )
  })
})

describe('waitForTourAnchor', () => {
  it('times out when the anchor never appears', async () => {
    let t = 0
    const queue: Array<() => void> = []
    const pending = waitForTourAnchor(
      'missing',
      50,
      () => t,
      (cb) => {
        queue.push(cb)
      },
    )
    t = 50
    for (const cb of queue) cb()
    assert.equal(await pending, false)
  })
})

describe('holePadForBox', () => {
  it('uses a tighter pad on large surfaces', () => {
    assert.equal(
      holePadForBox({ top: 0, left: 0, width: 900, height: 400 }),
      6,
    )
  })

  it('uses a looser pad on small chrome', () => {
    assert.equal(
      holePadForBox({ top: 0, left: 0, width: 40, height: 32 }),
      10,
    )
  })

  it('hugs the expanded control panel shell', () => {
    assert.equal(
      holePadForTourAnchor('control-panel', {
        top: 80,
        left: 1500,
        width: 400,
        height: 640,
      }),
      0,
    )
    assert.equal(
      holePadForTourAnchor('nav', { top: 0, left: 0, width: 900, height: 400 }),
      6,
    )
    assert.equal(
      holePadForTourAnchor('home-agent', {
        top: 200,
        left: 400,
        width: 120,
        height: 120,
      }),
      0,
    )
  })
})

describe('home agent press box', () => {
  it('keeps a circular press target inside the inset', () => {
    const rem = 16
    const insets = homeAgentPressInsets(rem, false)
    const box = pickHomeAgentPressBox(1440, 900, rem, [], insets)
    assert.equal(box.width, HOME_AGENT_PRESS_REM * rem)
    assert.equal(box.height, box.width)
    assert.ok(box.left >= insets.left - 0.01)
    assert.ok(box.top >= insets.top - 0.01)
    assert.ok(box.left + box.width <= 1440 - insets.right + 0.01)
    assert.ok(box.top + box.height <= 900 - insets.bottom + 0.01)
    assert.equal(isHomeAgentActionStep({ id: 'home-agent', action: 'open-agent' }), true)
    assert.equal(isHomeAgentActionStep({ id: 'nav' }), false)
  })

  it('slides off an occupied center', () => {
    const rem = 16
    const insets = homeAgentPressInsets(rem, false)
    const empty = pickHomeAgentPressBox(1440, 900, rem, [], insets)
    const blocker = {
      top: empty.top - 8,
      left: empty.left - 8,
      width: empty.width + 16,
      height: empty.height + 16,
    }
    const next = pickHomeAgentPressBox(1440, 900, rem, [blocker], insets)
    const overlap = (a: typeof empty, b: typeof blocker) => {
      const left = Math.max(a.left, b.left)
      const top = Math.max(a.top, b.top)
      const right = Math.min(a.left + a.width, b.left + b.width)
      const bottom = Math.min(a.top + a.height, b.top + b.height)
      return Math.max(0, right - left) * Math.max(0, bottom - top)
    }
    assert.ok(overlap(next, blocker) < overlap(empty, blocker))
  })
})

describe('computeTourCardPosition', () => {
  it('puts the card to the right of a tall left rail, centered', () => {
    const hole = {
      top: 200,
      left: 16,
      width: 56,
      height: 400,
      right: 72,
      bottom: 600,
    }
    const pos = computeTourCardPosition(hole, 300, 160, 1280, 800)
    assert.equal(pos.placement, 'right')
    assert.equal(pos.left, 72 + 14)
    assert.ok(pos.top > 200 && pos.top + 160 < 600)
  })

  it('puts the card above a wide bottom bar, centered', () => {
    const hole = {
      top: 740,
      left: 400,
      width: 480,
      height: 48,
      right: 880,
      bottom: 788,
    }
    const pos = computeTourCardPosition(hole, 300, 160, 1280, 800)
    assert.equal(pos.placement, 'top')
    assert.equal(pos.top + 160 + 14, 740)
    const cardMid = pos.left + 150
    const holeMid = 400 + 240
    assert.ok(Math.abs(cardMid - holeMid) < 1)
  })

  it('puts the card to the left of a top-right island', () => {
    const hole = {
      top: 16,
      left: 1100,
      width: 160,
      height: 48,
      right: 1260,
      bottom: 64,
    }
    const pos = computeTourCardPosition(hole, 300, 160, 1280, 800)
    assert.equal(pos.placement, 'left')
    assert.ok(pos.left + 300 <= 1100)
    assert.ok(pos.top >= 16)
  })

  it('sits below a free-layout canvas when the top and bottom have room', () => {
    const hole = {
      top: 224,
      left: 328,
      width: 1264,
      height: 632,
      right: 1592,
      bottom: 856,
    }
    const pos = computeTourCardPosition(hole, 320, 122, 1920, 1080)
    assert.equal(pos.placement, 'bottom')
    assert.equal(pos.top, 856 + 14)
    assert.ok(pos.left >= 328)
    assert.ok(pos.left + 320 <= 1592)
  })

  it('docks a near-full grid card to the bottom of the viewport', () => {
    const hole = {
      top: 80,
      left: 80,
      width: 1120,
      height: 640,
      right: 1200,
      bottom: 720,
    }
    const pos = computeTourCardPosition(hole, 300, 160, 1280, 800)
    assert.equal(pos.placement, 'dock')
    assert.ok(pos.top + 160 <= 800)
    assert.ok(Math.abs(pos.left + 150 - 640) < 1)
  })

  it('sits a tall rail hole on the right without overlapping it', () => {
    const hole = {
      top: 200,
      left: 16,
      width: 56,
      height: 400,
      right: 72,
      bottom: 600,
    }
    const pos = computeTourCardPosition(hole, 300, 160, 1280, 800)
    assert.equal(pos.placement, 'right')
    assert.ok(pos.left >= 72)
    assert.ok(pos.top >= 16)
    assert.ok(pos.top + 160 <= 800 - 16)
  })
})
