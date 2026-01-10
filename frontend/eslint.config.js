// @ts-check
import antfu from '@antfu/eslint-config'

export default antfu(
  {
    formatters: true,
  },
  {
    rules: {
      'react/forbid-dom-props': 'off',
      'ts/no-unused-expressions': 'off',
    },
  },
)
