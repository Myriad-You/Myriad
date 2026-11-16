import type { SetStateAction } from 'react'
import type { ShellTranslationKeys } from '../../../i18n/assembleLocale'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { deferred, mountNoteHook } from './noteHookTestUtils'
import { useNoteRemoteImages } from './useNoteRemoteImages'

type Host = Parameters<typeof useNoteRemoteImages>[0]

const t = { phantasi: { notePasteImagesKept: '{count} kept' } } as unknown as ShellTranslationKeys

function settle() {
  return new Promise((resolve) => setTimeout(resolve, 0))
}

function host(pane: 'write' | 'visual' | 'preview', save: Host['save']) {
  const contents: Array<SetStateAction<string>> = []
  const commits: string[] = []
  const value: Host = {
    t,
    format: (template, values) => template.replace('{count}', String(values.count)),
    visualRef: { current: null },
    textareaRef: { current: null },
    paneRef: { current: pane },
    visualEditing: { current: false },
    commitVisualMd: (md) => commits.push(md),
    setContentMd: (next) => contents.push(next),
    save,
  }
  return { value, contents, commits }
}

describe('useNoteRemoteImages', () => {
  it('写栏：存好后只换图片地址，期间打的字和光标都留着', async () => {
    const saved = deferred<string>()
    const { value, contents } = host('write', () => saved.promise)
    const hook = await mountNoteHook(useNoteRemoteImages, value)
    const el = document.createElement('textarea')
    document.body.append(el)
    el.value = '![a](https://x.example/a.png)\n\n[link](https://x.example/a.png)'
    value.textareaRef = { current: el }
    await hook.render(value)
    hook.current.keepPastedImages(el.value, 'markdown')
    // 图片还没存好，站长接着往后打字。
    el.setRangeText('\n\n接着写', el.value.length, el.value.length, 'end')
    const caret = el.selectionStart
    saved.resolve('/media/assets/1/a.png')
    await settle()
    assert.equal(
      el.value,
      '![a](/media/assets/1/a.png)\n\n[link](https://x.example/a.png)\n\n接着写',
    )
    assert.equal(el.selectionStart, caret + '/media/assets/1/a.png'.length - 'https://x.example/a.png'.length)
    assert.deepEqual(contents, [el.value])
    await hook.close()
  })

  it('可视层正在编辑：只改那张 img，交回整篇 Markdown', async () => {
    const { value, commits } = host('visual', async () => '/media/assets/2/b.png')
    const hook = await mountNoteHook(useNoteRemoteImages, value)
    const root = document.createElement('div')
    root.innerHTML = '<p>hi <img src="https://x.example/b.png" alt="b"> 后面</p>'
    value.visualRef = { current: root }
    value.visualEditing.current = true
    await hook.render(value)
    hook.current.keepPastedImages('<p><img src="https://x.example/b.png" alt="b"></p>', 'html')
    await settle()
    assert.equal(root.querySelector('img')?.getAttribute('src'), '/media/assets/2/b.png')
    assert.equal(commits.length, 1)
    assert.match(commits[0]!, /hi !\[b\]\(\/media\/assets\/2\/b\.png\) 后面/)
    await hook.close()
  })

  it('别的情况改状态里的原文，读的是换那一刻的值', async () => {
    const { value, contents } = host('preview', async () => '/media/assets/3/c.png')
    const hook = await mountNoteHook(useNoteRemoteImages, value)
    hook.current.keepPastedImages('![c](https://x.example/c.png)', 'markdown')
    await settle()
    assert.equal(contents.length, 1)
    const update = contents[0]!
    assert.equal(typeof update, 'function')
    assert.equal(
      (update as (current: string) => string)('新打的字 ![c](https://x.example/c.png)'),
      '新打的字 ![c](/media/assets/3/c.png)',
    )
    await hook.close()
  })

  it('本站图片、data: 不发请求；存不下的不改正文', async () => {
    const asked: string[] = []
    const { value, contents, commits } = host('preview', async (url) => {
      asked.push(url)
      throw new Error('404')
    })
    const hook = await mountNoteHook(useNoteRemoteImages, value)
    hook.current.keepPastedImages(
      '![](https://test.invalid/x.png) ![](data:image/png;base64,aa) ![](https://x.example/gone.png)',
      'markdown',
    )
    await settle()
    assert.deepEqual(asked, ['https://x.example/gone.png'])
    assert.deepEqual(contents, [])
    assert.deepEqual(commits, [])
    await hook.close()
  })

  it('编辑器关了才存好的，不再去改', async () => {
    const saved = deferred<string>()
    const { value, contents } = host('preview', () => saved.promise)
    const hook = await mountNoteHook(useNoteRemoteImages, value)
    hook.current.keepPastedImages('![d](https://x.example/d.png)', 'markdown')
    await hook.close()
    saved.resolve('/media/assets/4/d.png')
    await settle()
    assert.deepEqual(contents, [])
  })
})
