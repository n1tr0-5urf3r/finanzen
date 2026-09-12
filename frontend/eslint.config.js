import js from '@eslint/js';
import globals from 'globals';
import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  { ignores: ['dist', 'coverage'] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ['**/*.{ts,tsx}'],
    languageOptions: { ecmaVersion: 2022, globals: globals.browser },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      'react-refresh/only-export-components': 'off',
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      // Shared primitives must not depend on features; that dependency direction is
      // what keeps the feature folders from turning back into a flat pile.
      'no-restricted-imports': ['error', {
        patterns: [{
          group: ['**/features/*'],
          message: 'components/, charts/ and lib/ must not import from features/.',
        }],
      }],
    },
  },
  {
    // Features may import each other's hooks, and App/main are composition roots
    // whose whole job is to wire features together.
    files: ['src/features/**/*.{ts,tsx}', 'src/App.tsx', 'src/main.tsx'],
    rules: { 'no-restricted-imports': 'off' },
  },
);
