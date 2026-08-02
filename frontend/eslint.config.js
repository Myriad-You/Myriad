// @ts-check
import antfu from '@antfu/eslint-config'

export default antfu(
  {
    formatters: false,
  },
  {
    rules: {
      // Project runs tests with tsx + node:test (no vitest dependency).
      'test/no-import-node-test': 'off',
      'react/forbid-dom-props': 'off',
      'ts/no-unused-expressions': 'off',
      'no-console': 'off',
      'no-alert': 'off',
      'style/multiline-ternary': 'off',
      'style/max-statements-per-line': 'off',
      'style/arrow-parens': 'off',
      'style/brace-style': 'off',
      'style/indent': 'off',
      'style/indent-binary-ops': 'off',
      'style/jsx-closing-tag-location': 'off',
      'style/jsx-curly-newline': 'off',
      'style/jsx-one-expression-per-line': 'off',
      'style/jsx-wrap-multilines': 'off',
      'style/operator-linebreak': 'off',
      'style/quote-props': 'off',
      'style/quotes': 'off',
      'antfu/consistent-list-newline': 'off',
      'antfu/consistent-chaining': 'off',
      'antfu/if-newline': 'off',
      'jsdoc/check-param-names': 'off',
      'ts/no-use-before-define': 'off',
      'e18e/prefer-array-fill': 'off',
      'regexp/no-unused-capturing-group': 'off',
      'no-control-regex': 'off',
      'style/no-mixed-operators': 'off',
      'unused-imports/no-unused-vars': [
        'error',
        {
          vars: 'all',
          varsIgnorePattern: '^_',
          args: 'after-used',
          argsIgnorePattern: '^_',
          caughtErrors: 'all',
          caughtErrorsIgnorePattern: '^_',
          destructuredArrayIgnorePattern: '^_',
        },
      ],
      'ts/prefer-literal-enum-member': 'off',
      'style/member-delimiter-style': 'off',
    },
  },
)
