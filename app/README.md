# callme app

The GUI: an nsh app in TSX (`App.tsx`) with a Rust crate of its own (`src/lib.rs`) on callme-core.

```sh
CALLME_PROJECT=/path/to/project nsh run --target macos   # run it on a project
../scripts/test-app.sh                                   # headless tests against a local server
../scripts/test-app.sh tests/screenshot.test.tsx --target macos   # pictures in app/screenshots/
```

Needs nsh built from source (the editor is not in a release yet): `curl -fsSL https://nsh.dev/install.sh | sh -s -- --source`.
`Cargo.toml` points at that checkout (`~/.nsh/source`).
