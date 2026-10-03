// @ts-check
import antfu from '@antfu/eslint-config'

export default antfu(
  {
    formatters: false,
    // Vendored Three IIFE and Anime2.5DRig rigger/genericparts (upstream copies; see NOTICE) stay lint-untouched so they still diff against upstream.
    ignores: [
      'public/tapp-runtime/**',
      'src/utils/liquidGlass/vendor/**',
      'src/features/merope/anime25drig/vendor/**',
    ],
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
  {
    files: ['**/*.{js,mjs,cjs,ts,tsx}'],
    rules: {
      // Lock ES2024+ clone; JSON.parse(JSON.stringify) drops undefined and dates.
      'unicorn/prefer-structured-clone': 'error',
      // 前两条沿用基础配置；最后一条：文字/图标色用 --cfg-accent，--color-primary 是壁纸原色，可能和底色同色（#602）。
      'no-restricted-syntax': [
        'error',
        'TSEnumDeclaration[const=true]',
        'TSExportAssignment',
        // 前景属性的值里出现 var(--color-primary)（含兜底值、三元、模板字符串、带引号的键）。
        {
          selector: "Property[key.name=/^(color|caretColor|outlineColor|textDecorationColor|stroke|fill|WebkitTextFillColor)$/] Literal[value=/var\\(--color-primary[,)]/]",
          message: '前景色用 var(--cfg-accent)：--color-primary 是壁纸原色，可能和底色同色（#602）',
        },
        {
          selector: "Property[key.value=/^(color|caretColor|outlineColor|textDecorationColor|stroke|fill|WebkitTextFillColor)$/] Literal[value=/var\\(--color-primary[,)]/]",
          message: '前景色用 var(--cfg-accent)：--color-primary 是壁纸原色，可能和底色同色（#602）',
        },
        {
          selector: "Property[key.name=/^(color|caretColor|outlineColor|textDecorationColor|stroke|fill|WebkitTextFillColor)$/] TemplateElement[value.raw=/var\\(--color-primary[,)]/]",
          message: '前景色用 var(--cfg-accent)：--color-primary 是壁纸原色，可能和底色同色（#602）',
        },
      ],
    },
  },
  {
    // Merope grew several 1500-line files before; a warning here asks for a
    // split while the seams are still obvious. Vendored upstream is exempt.
    files: ['src/features/merope/**/*.{ts,tsx}'],
    ignores: [
      '**/*.test.{ts,tsx}',
      '**/*.fixture.ts',
      'src/features/merope/anime25drig/upstream/**',
    ],
    rules: {
      'max-lines': ['warn', { max: 600, skipBlankLines: true, skipComments: true }],
    },
  },
)
