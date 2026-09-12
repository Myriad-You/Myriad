import assert from 'node:assert/strict'
import { it } from 'node:test'
import {
  READER_ANNOTATIONS_PANEL_ID,
  READER_COMMENTS_PANEL_ID,
  READER_MOBILE_CONTROLS_ID,
  READER_PODCAST_PANEL_ID,
  READER_TOC_PANEL_ID,
  READER_TOOL_SHEET_ID,
} from './constants.ts'

it('reader overlay ids stay unique', () => {
  const ids = [
    READER_COMMENTS_PANEL_ID,
    READER_TOOL_SHEET_ID,
    READER_TOC_PANEL_ID,
    READER_ANNOTATIONS_PANEL_ID,
    READER_PODCAST_PANEL_ID,
    READER_MOBILE_CONTROLS_ID,
  ]
  assert.equal(new Set(ids).size, ids.length)
})
