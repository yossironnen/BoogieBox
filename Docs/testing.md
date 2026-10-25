# Testing Requirements

## Automated Test Coverage (MANDATORY)

All production code must maintain a minimum of **80% automated test coverage**. A task involving code changes is incomplete until all tests and coverage checks pass.

### Required Coverage Thresholds

Client coverage must meet or exceed 80% globally for:

- Statements
- Branches
- Functions
- Lines

Rust coverage must meet or exceed:

- Lines: 80%

Thresholds apply to the complete client and Rust workspace, not only changed files.

### Client Configuration

`client/vite.config.ts` must contain:

```ts
coverage: {
  provider: 'v8',
  reporter: ['text', 'html'],
  include: ['src/**/*.ts', 'src/**/*.tsx'],
  exclude: ['src/test/**', 'src/**/*.test.*', 'src/index.tsx'],
  thresholds: {
    statements: 80,
    branches: 80,
    functions: 80,
    lines: 80,
  },
},

When modifying or creating any code file, write or update Vitest tests and run them.

- Every new function, method, or component gets at least one test
- Tests live in `<filename>.test.ts` (or `.test.tsx`) alongside the source file
- Run `npx vitest run` after writing tests — fix failures before finishing
- Never skip tests, even for simple changes

### Structure
```ts
import { describe, it, expect, vi } from 'vitest'

describe('<ModuleName>', () => {
  it('should <expected behavior>', () => {
    // arrange, act, assert
  })
})
```

### Coverage
- Happy path
- Edge cases (nulls, empty arrays, boundary values)
- Error/failure cases where applicable

## UI Release Suite (Playwright)

Browser end-to-end tests live in `e2e/` and run against the packaged release build, not the dev client.
Run them before publishing a release, either with `build-server-rust.bat --e2e` (`./build-server-rust.sh --e2e`
on Linux) or `npm run e2e` against an existing build. They are not part of `npm test` or the coverage thresholds.

- Import `test`, `expect` and helpers from `e2e/support/test.ts`; every test fails on page errors, app console
  errors and HTTP 5xx responses.
- When a user-facing flow changes, update or add the matching spec in `e2e/tests/`.
- See `e2e/README.md` for commands, options and coverage.
