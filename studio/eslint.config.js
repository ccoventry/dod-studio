import globals from 'globals';

// Two rules only, to start: dead code (an error) and long parameter lists (a
// warning), the JS half of docs/coding_standards.md's checks (#1179). Add a
// rule with the PR that fixes what it finds, not before.
export default [
  {
    files: ['src/**/*.js'],
    languageOptions: {
      ecmaVersion: 'latest',
      sourceType: 'module',
      globals: { ...globals.browser, ...globals.node },
    },
    rules: {
      // A leading underscore marks a name taken only to be left out (a rest
      // destructure, a callback's first argument).
      'no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_', ignoreRestSiblings: true }],
      'max-params': ['warn', 5],
    },
  },
];
