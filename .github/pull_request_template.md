<!-- PR title: Conventional Commits, e.g. `feat(core): compact arena tree`. Merged with squash. -->

## Summary

<!-- What does this change and why? Link the roadmap step or issue (e.g. "Step 2.1", "Closes #12"). -->

## Changes

-

## How to test

<!-- Commands and manual steps a reviewer can follow. -->

```bash
cargo test --workspace
npm run tauri dev
```

## Checklist

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `npm run build`
- [ ] IPC DTOs changed → `npm run gen:types` run and `src/types/generated/` committed
- [ ] UI changes checked in the running app (screenshot below if visual)
- [ ] File-system tests use `tempfile::TempDir` only — never real user directories
- [ ] No new `unsafe`, network access or broad Tauri permissions

## Screenshots

<!-- Optional: before / after. -->
