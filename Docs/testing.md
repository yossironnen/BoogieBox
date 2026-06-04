# Testing Requirements

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
