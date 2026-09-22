import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

function walk(root: string, suffix: RegExp): string[] {
  const out: string[] = []
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const next = join(root, entry.name)
    if (entry.isDirectory()) out.push(...walk(next, suffix))
    else if (suffix.test(entry.name)) out.push(next)
  }
  return out
}

describe('phantasi/skin 边界', () => {
  it('皮不进口 phantasiApi / pageData / manager', () => {
    const files = walk(dir, /\.(ts|tsx)$/).filter(
      (file) => !file.endsWith('.test.ts'),
    )
    assert.ok(files.length > 0)
    for (const file of files) {
      const src = readFileSync(file, 'utf8')
      assert.doesNotMatch(src, /from ['"].*phantasiApi['"]/)
      assert.doesNotMatch(src, /from ['"].*pageData['"]/)
      assert.doesNotMatch(src, /from ['"].*useBoardPage['"]/)
      assert.doesNotMatch(src, /from ['"].*usePhantasiSources['"]/)
      assert.doesNotMatch(src, /from ['"].*usePhantasiItems['"]/)
      assert.doesNotMatch(src, /from ['"].*usePhantasiStarred['"]/)
      assert.doesNotMatch(src, /from ['"].*usePhantasiNotes['"]/)
      assert.doesNotMatch(src, /from ['"].*usePhantasiSeo['"]/)
      assert.doesNotMatch(src, /from ['"]\.\.\/manager/)
    }
  })

  it('工作台是仪表盘壳：侧栏一页一项，分类标题用 SettingSection', () => {
    const src = [
      'PhantasiWorkbench.tsx',
      'PhantasiWorkbenchHome.tsx',
      'PhantasiWorkbenchHomeOptions.tsx',
      'PhantasiWorkbenchNotes.tsx',
      'PhantasiWorkbenchComments.tsx',
      'PhantasiWorkbenchReviews.tsx',
      'PhantasiWorkbenchMedia.tsx',
      'PhantasiWorkbenchIo.tsx',
      'PhantasiWorkbenchFeeds.tsx',
      'PhantasiWorkbenchChrome.tsx',
    ]
      .map((file) => readFileSync(join(dir, file), 'utf8'))
      .join('\n')
    assert.match(src, /phantasi-workbench__rail/)
    assert.match(src, /config-sidebar/)
    assert.match(src, /config-nav-item/)
    assert.match(src, /onPane/)
    assert.match(src, /pane === 'notes'/)
    assert.match(src, /pane === 'comments'/)
    assert.match(src, /pane === 'reviews'/)
    assert.match(src, /pane === 'media'/)
    assert.match(src, /workbenchComments/)
    assert.match(src, /workbenchReviews/)
    assert.match(src, /filterWorkbenchComments/)
    assert.match(src, /filterWorkbenchReviews/)
    assert.match(src, /phantasi-workbench__kpis/)
    assert.match(src, /phantasi-workbench__home-data/)
    assert.match(src, /phantasi-workbench__home-body/)
    const home = readFileSync(join(dir, 'PhantasiWorkbenchHome.tsx'), 'utf8')
    const homeView = home.slice(home.indexOf('export function WorkbenchHome'))
    assert.ok(
      homeView.indexOf('phantasi-workbench__kpis') <
        homeView.indexOf('phantasi-workbench__home-data'),
    )
    assert.ok(
      homeView.indexOf('phantasi-workbench__kpis') <
        homeView.indexOf('phantasi-workbench__home-data'),
    )
    assert.match(src, /boardNavVisibility/)
    assert.ok(homeView.indexOf('phantasi-workbench__home-data') < homeView.indexOf('<WorkbenchHomeOptions'))
    assert.match(src, /workbenchHomeContinue/)
    assert.match(src, /workbenchHomeUpcoming/)
    assert.match(src, /workbenchHomeRecent/)
    assert.match(src, /phantasi-workbench__home-row/)
    assert.doesNotMatch(src, /phantasi-workbench__home-card/)
    assert.doesNotMatch(src, /HomeCard/)
    assert.match(src, /homeFaceSrc/)
    assert.match(src, /has-face/)
    assert.doesNotMatch(src, /scheduledCount/)
    assert.doesNotMatch(src, /failedCount/)
    assert.match(src, /SettingSection/)
    assert.match(src, /ManagedList/)
    assert.match(src, /SettingsButton/)
    assert.match(src, /workbenchNotes/)
    assert.match(src, /workbenchSearchNotes/)
    assert.match(src, /workbenchSearchMedia/)
    assert.match(src, /workbenchSearchSources/)
    assert.match(src, /workbenchSearchCategories/)
    assert.match(src, /workbenchCategories/)
    assert.match(src, /open\('noteCategories'\)/)
    assert.match(src, /onPane\('sourceCategories'\)|open\('sourceCategories'\)/)
    assert.match(src, /pane === 'noteCategories'/)
    assert.match(src, /pane === 'sourceCategories'/)
    assert.doesNotMatch(src, /pane: 'categories'/)
    assert.doesNotMatch(src, /pane: 'noteCategories'/)
    assert.doesNotMatch(src, /pane: 'sourceCategories'/)
    assert.match(src, /pack\.id\}-\$\{item\.pane\}/)
    assert.match(src, /workbenchDefaultSort/)
    assert.match(src, /layout="horizontal"/)
    assert.match(src, /onRefreshSources/)
    assert.match(src, /workbench-rsshub-actions/)
    assert.doesNotMatch(src, /description=\{phantasi\.workbenchSourcesHint\}/)
    assert.match(src, /usePhantasiGuides/)
    assert.match(src, /bindGuide/)
    assert.match(src, /workbench\.overview/)
    assert.match(src, /workbench\.notesIo/)
    assert.match(src, /workbench\.feedsIo/)
    assert.match(src, /toolbarPlacement="filters"/)
    assert.match(src, /onDeleteNotes/)
    assert.match(src, /selectAll/)
    assert.match(src, /deleteSelected/)
    assert.match(src, /workbenchDeleteSelectedNotesConfirm/)
    assert.match(src, /workbenchAssignCategory/)
    assert.match(src, /onAssignNotes/)
    assert.match(src, /filterWorkbenchNotes/)
    assert.match(src, /workbenchNoteAuthor/)
    assert.match(src, /collectWorkbenchNoteAuthors/)
    assert.match(src, /onMediaFilter/)
    assert.match(src, /workbenchMedia/)
    assert.match(src, /workbenchSources/)
    assert.match(src, /workbenchNavTransfer/)
    assert.match(src, /workbenchWordpressHint/)
    assert.match(src, /workbenchHaloHint/)
    assert.match(src, /workbenchPipackHint/)
    assert.match(src, /workbenchOpmlHint/)
    assert.doesNotMatch(src, /pack: 'transfer'/)
    assert.match(src, /workbenchPipack/)
    assert.match(src, /workbenchOpml/)
    assert.match(src, /workbenchWordpress/)
    assert.match(src, /workbenchHalo/)
    assert.match(src, /workbenchTypecho/)
    assert.match(src, /workbenchMarkdown/)
    assert.match(src, /SiWordpress/)
    assert.match(src, /HaloIcon/)
    assert.match(src, /TypechoIcon/)
    assert.match(src, /SiMarkdown/)
    assert.match(src, /workbenchExportNotes/)
    assert.match(src, /TransferActions/)
    assert.match(src, /phantasi-workbench__drop/)
    assert.match(src, /startImport/)
    assert.match(src, /pane === 'notesIo'/)
    assert.match(src, /pane === 'feedsIo'/)
    assert.match(src, /pane === 'add'/)
    assert.match(src, /open\('add'\)/)
    assert.match(src, /pane === 'topics'/)
    assert.match(src, /onPane\('topics'\)/)
    assert.doesNotMatch(src, /pane: 'topics'/)
    assert.match(src, /workbench\.topics/)
    assert.doesNotMatch(src, /sourceFormOpen/)
    assert.doesNotMatch(src, /pane === 'wordpress'/)
    assert.doesNotMatch(src, /pane === 'pipack'/)
    assert.match(src, /admin/)
    assert.match(src, /SettingGroupGrid/)
    assert.match(src, /phantasi-workbench__io-grid/)
    assert.doesNotMatch(
      readFileSync(join(dir, 'PhantasiWorkbenchIo.tsx'), 'utf8'),
      /<SettingGroup[\s/>]/,
    )
    assert.doesNotMatch(src, /from ['"]\.\.\/manager/)
  })
})
