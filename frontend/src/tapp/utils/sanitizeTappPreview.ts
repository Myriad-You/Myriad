const DANGEROUS_SELECTOR = [
  'script',
  'noscript',
  'iframe',
  'frame',
  'frameset',
  'object',
  'embed',
  'portal',
  'base',
  'meta',
  'link',
  'foreignObject',
  'animate',
  'animateMotion',
  'animateTransform',
  'set',
  'use',
].join(',')

const INTERACTIVE_SELECTOR = [
  'form',
  'input',
  'button',
  'textarea',
  'select',
  'option',
].join(',')

const URL_ATTRIBUTES = new Set([
  'action',
  'cite',
  'data',
  'formaction',
  'href',
  'manifest',
  'ping',
  'poster',
  'src',
  'srcset',
  'xlink:href',
])

const INTERACTIVE_SHELL_SCORE_LIMIT = 120

function isRuntimeDependentShell(
  parsed: Document,
  preserveControls: boolean,
): boolean {
  const blockedSelector = preserveControls
    ? DANGEROUS_SELECTOR
    : `${DANGEROUS_SELECTOR},${INTERACTIVE_SELECTOR}`
  const blockedNodeCount = parsed.querySelectorAll(blockedSelector).length
  const emptyMountCount = Iterator.from(
    parsed.body.querySelectorAll('div,span,main,section,aside'),
  ).filter(
    (element) =>
      element.childElementCount === 0 && !element.textContent?.trim(),
  ).reduce((count) => count + 1, 0)

  return blockedNodeCount + emptyMountCount * 2 >= INTERACTIVE_SHELL_SCORE_LIMIT
}

function sanitizePreviewCss(css: string): string {
  return css
    .slice(0, 512 * 1024)
    .replaceAll(/@import\s[^;]+;?/gi, '')
    .replaceAll(/(?:expression|behavior|-moz-binding)\s*:[^;}]*/gi, '')
    .replaceAll(/url\(([^)]*)\)/gi, (match, raw: string) => {
      const value = raw.trim().replaceAll(/^(['"])(.*)\1$/g, '$2')
      return /^(?:data:image\/|blob:)/i.test(value) ? match : 'none'
    })
}

function escapeStyleText(css: string): string {
  return css.replaceAll(/<\/style/gi, '<\\/style')
}

/**
 * Build a static preview document from an untrusted Tapp page template.
 *
 * The caller must still render the result in an iframe without sandbox
 * capabilities. Sanitizing here is defense in depth and also removes inert
 * controls that would otherwise make the preview look interactive.
 */
export function buildSanitizedTappPreview(
  sourceHtml: string,
  sourceCss = '',
  options?: {
    theme?: 'auto' | 'light' | 'dark'
    preserveControls?: boolean
  },
): string | null {
  if (typeof DOMParser === 'undefined' || !sourceHtml.trim()) return null

  const parsed = new DOMParser().parseFromString(
    sourceHtml.slice(0, 512 * 1024),
    'text/html',
  )
  const preserveControls = options?.preserveControls === true
  if (isRuntimeDependentShell(parsed, preserveControls)) return null

  const embeddedCss = Iterator.from(parsed.querySelectorAll('style'))
    .map((style) => style.textContent || '')
    .toArray()
    .join('\n')

  const blockedSelector = preserveControls
    ? DANGEROUS_SELECTOR
    : `${DANGEROUS_SELECTOR},${INTERACTIVE_SELECTOR}`
  parsed.querySelectorAll(blockedSelector).forEach((node) => node.remove())
  parsed.querySelectorAll('style').forEach((node) => node.remove())

  parsed.querySelectorAll('*').forEach((element) => {
    for (const attribute of element.attributes) {
      const name = attribute.name.toLowerCase()
      if (
        name.startsWith('on') ||
        name === 'srcdoc' ||
        name === 'contenteditable' ||
        name === 'autofocus' ||
        URL_ATTRIBUTES.has(name)
      ) {
        element.removeAttribute(attribute.name)
        continue
      }
      if (name === 'style') {
        const safeStyle = sanitizePreviewCss(attribute.value)
        if (safeStyle.trim()) element.setAttribute('style', safeStyle)
        else element.removeAttribute('style')
      }
    }

    if (preserveControls) {
      const tagName = element.tagName.toLowerCase()
      if (
        tagName === 'a' ||
        tagName === 'button' ||
        tagName === 'input' ||
        tagName === 'select' ||
        tagName === 'textarea'
      ) {
        element.setAttribute('tabindex', '-1')
      }
      if (tagName === 'form') {
        element.setAttribute('autocomplete', 'off')
      }
    }
  })

  const safeCss = escapeStyleText(
    sanitizePreviewCss(`${embeddedCss}\n${sourceCss}`),
  )
  const body = parsed.body.innerHTML.slice(0, 512 * 1024)
  const theme = options?.theme || 'auto'
  const rootClasses = Iterator.from(parsed.documentElement.classList)
    .filter((className) => /^[\w-]{1,64}$/.test(className))
    .toArray()
  if (theme !== 'auto') rootClasses.push(theme)
  const rootClassAttribute = rootClasses.length
    ? ` class="${Iterator.from(new Set(rootClasses)).toArray().join(' ')}"`
    : ''
  const themeAttribute = theme === 'auto' ? '' : ` data-theme="${theme}"`
  const colorScheme = theme === 'auto' ? 'light dark' : theme

  return `<!doctype html>
<html${rootClassAttribute}${themeAttribute}>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data: blob:; media-src data: blob:; font-src data:; style-src 'unsafe-inline'; form-action 'none'; base-uri 'none';">
  <style>
    :root { color-scheme: ${colorScheme}; }
    *, *::before, *::after { box-sizing: border-box; }
    html, body { min-height: 100%; margin: 0; }
    body {
      padding: 1rem;
      color: light-dark(#1d1d1f, #f5f5f7);
      font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif;
      background: light-dark(#f5f5f7, #151516);
    }
    ${safeCss}
    html, body {
      width: 100% !important;
      height: 100% !important;
      min-height: 0 !important;
      overflow: hidden !important;
      scrollbar-width: none !important;
    }
    html::-webkit-scrollbar, body::-webkit-scrollbar { display: none !important; }
  </style>
</head>
<body>${body}</body>
</html>`
}
