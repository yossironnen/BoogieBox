/**
 * Defines Eslint.Config behavior for BoogieBox.
 */

import eslint from '@eslint/js'
import hooks from 'eslint-plugin-react-hooks'
import globals from 'globals'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  {
    ignores: [
      '**/node_modules/**',
      '**/coverage/**',
      '**/build/**',
      '**/dist/**',
      'Releases/**',
      'tmp/**',
      'desktop/src-tauri/target/**',
      'desktop/src-tauri/gen/**',
      'desktop/src-tauri/resources/**',
      'Services/boogiemix/python/.venv/**',
      'tools/**',
    ],
  },
  eslint.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['client/src/**/*.{ts,tsx}', 'scripts/**/*.{js,cjs,mjs}'],
    rules: {
      '@typescript-eslint/no-unused-vars': 'off',
      'no-empty': 'off',
      'no-useless-assignment': 'off',
      'no-useless-escape': 'off',
      'preserve-caught-error': 'off',
    },
  },
  {
    files: ['client/src/**/*.{ts,tsx}'],
    languageOptions: {
      globals: globals.browser,
    },
    plugins: {
      'react-hooks': hooks,
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
      'react-hooks/exhaustive-deps': 'warn',
      'react-hooks/rules-of-hooks': 'error',
    },
  },
  {
    files: ['scripts/**/*.{js,cjs,mjs}'],
    languageOptions: {
      globals: globals.node,
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-namespace': 'off',
      '@typescript-eslint/no-require-imports': 'off',
    },
  },
)
