# Backend Safety Rules

Enforce the following when backend code is modified:

1. **Input Validation** — Validate all request params, body fields, and query values. Never trust user input implicitly.
2. **SQL Safety** — All queries must be parameterized. String concatenation with user input is forbidden.
3. **Filesystem Safety** — No directory traversal (`../`). File operations restricted to configured library roots. UNC path normalization must be preserved.
4. **Blocking & Performance** — No heavy blocking work in request handlers. Stream large media files; never load fully into memory.
5. **Secrets & Sensitive Data** — Never log API keys, tokens, or sensitive paths. Never expose stack traces in API responses.

If any backend change violates these rules, refuse the change.
