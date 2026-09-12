/** Only text decorations. Media and their ancestors never leave the DOM. */
export function applyTextDecorations(
  container: HTMLElement,
  decoratedHtml: string,
): void {
  const doc = container.ownerDocument
  const desired = doc.createElement('div')
  desired.innerHTML = decoratedHtml
  const selector = 'mark.user-comment-highlight, mark.brewlia-annotation'
  const excluded =
    'script, style, button, iframe, .brew-embed-card, .brew-embed-exempt, .brew-bilibili-embed, .brew-netease-music, .brew-steam-game, .brew-bilibili-video'
  const textNodes = (root: HTMLElement) => {
    const walker = doc.createTreeWalker(root, 4)
    const nodes: Text[] = []
    while (walker.nextNode()) {
      const node = walker.currentNode as Text
      if (!node.parentElement?.closest(excluded)) nodes.push(node)
    }
    return nodes
  }
  const decorations: { start: number; end: number; mark: HTMLElement }[] = []
  const desiredNodes = textNodes(desired)
  let desiredOffset = 0
  for (const node of desiredNodes) {
    let parent = node.parentElement
    while (parent && parent !== desired) {
      if (parent.matches(selector)) {
        decorations.push({
          start: desiredOffset,
          end: desiredOffset + node.length,
          mark: parent,
        })
}
      parent = parent.parentElement
    }
    desiredOffset += node.length
  }
  if (
    desiredNodes.map((n) => n.data).join('') !==
    textNodes(container)
      .map((n) => n.data)
      .join('')
  ) {
    return
}
  const before = doc.createRange()
  const selection = doc.defaultView?.getSelection()
  let saved: { start: number; end: number } | undefined
  if (selection?.rangeCount) {
    const range = selection.getRangeAt(0)
    if (container.contains(range.commonAncestorContainer)) {
      before.selectNodeContents(container)
      before.setEnd(range.startContainer, range.startOffset)
      const start = before.toString().length
      saved = { start, end: start + range.toString().length }
    }
  }
  for (const mark of container.querySelectorAll(selector))
    mark.replaceWith(...mark.childNodes)
  const nodes: { node: Text; start: number; end: number }[] = []
  let offset = 0
  for (const node of textNodes(container)) {
    nodes.push({ node, start: offset, end: offset + node.length })
    offset += node.length
  }
  for (const { node, start, end } of nodes) {
    const hits = decorations.filter((d) => d.start < end && d.end > start)
    if (!hits.length) continue
    const cuts = Iterator.from(
      new Set([
        start,
        end,
        ...hits.flatMap((d) => [
          Math.max(start, d.start),
          Math.min(end, d.end),
        ]),
      ]),
    )
      .toArray()
      .toSorted((a, b) => a - b)
    const fragment = doc.createDocumentFragment()
    for (let i = 0; i < cuts.length - 1; i++) {
      let part: Node = doc.createTextNode(
        node.data.slice(cuts[i] - start, cuts[i + 1] - start),
      )
      for (const hit of hits
        .filter((d) => d.start <= cuts[i] && d.end >= cuts[i + 1])
        .toReversed()) {
        const mark = hit.mark.cloneNode(false) as HTMLElement
        mark.appendChild(part)
        part = mark
      }
      fragment.appendChild(part)
    }
    node.replaceWith(fragment)
  }
  if (saved && selection) {
    const range = doc.createRange()
    const current = doc.createTreeWalker(container, 4)
    let position = 0
    let started = false
    while (current.nextNode()) {
      const node = current.currentNode as Text
      if (!started && saved.start <= position + node.length) {
        range.setStart(node, Math.max(0, saved.start - position))
        started = true
      }
      if (started && saved.end <= position + node.length) {
        range.setEnd(node, Math.max(0, saved.end - position))
        selection.removeAllRanges()
        selection.addRange(range)
        break
      }
      position += node.length
    }
  }
}
