import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { themeBootInlineScript } from '../../utils/themeBootScript.ts'
import { EMBED_FIT_ATTR, EMBED_SCHEME_ATTR, embedHost } from './embedHost.ts'

const crate = (file: string) =>
  readFileSync(new URL(`../../../../crates/myriad-blog/src/${file}`, import.meta.url), 'utf8')

test('博客页那边用的是同样的属性名', () => {
  assert.match(crate('rewrite.rs'), new RegExp(`myriad-comments[^>]*${EMBED_FIT_ATTR}`))
  assert.match(crate('render.rs'), new RegExp(`myriad-friend-apply[^>]*${EMBED_FIT_ATTR}`))
  assert.ok(crate('postprocess.rs').includes(`iframe[${EMBED_SCHEME_ATTR}=dark]`))
  assert.ok(themeBootInlineScript().includes(`'${EMBED_SCHEME_ATTR}'`))
})

test('不在框里、外面跨源时没有宿主', () => {
  const top = {} as Window & { parent: Window }
  top.parent = top
  assert.equal(embedHost(top), null)
  const crossOrigin = {
    get parent(): Window {
      return {
        get document(): Document {
          throw new DOMException('Blocked', 'SecurityError')
        },
      } as Window
    },
  } as Window
  assert.equal(embedHost(crossOrigin), null)
})
