import svelte from 'eslint-plugin-svelte'
import ts from 'typescript-eslint'
export default [
  { ignores: ['dist/**', 'node_modules/**', 'src/lib/generated/**'] },
  ...svelte.configs['flat/recommended'],
  {
    files: ['**/*.svelte'],
    languageOptions: { parserOptions: { parser: ts.parser } },
    rules: { 'svelte/no-at-html-tags': 'error' },
  },
]
