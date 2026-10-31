// @ts-check
import antfu from '@antfu/eslint-config'

// The rules Myriad's frontend held this code to before it moved here.
export default antfu(
  {
    formatters: false,
    react: false,
  },
  {
    rules: {
      // Tests run with tsx + node:test (no vitest dependency).
      'test/no-import-node-test': 'off',
      'ts/no-unused-expressions': 'off',
      'no-console': 'off',
      'style/multiline-ternary': 'off',
      'style/max-statements-per-line': 'off',
      'style/arrow-parens': 'off',
      'style/brace-style': 'off',
      'style/indent': 'off',
      'style/indent-binary-ops': 'off',
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
  {
    files: ['**/*.{js,mjs,cjs,ts}'],
    rules: {
      // Lock ES2024+ clone; JSON.parse(JSON.stringify) drops undefined and dates.
      'unicorn/prefer-structured-clone': 'error',
      'no-restricted-syntax': ['error', 'TSEnumDeclaration[const=true]', 'TSExportAssignment'],
    },
  },
)
