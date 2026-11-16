# Contributing

Thanks for looking. MiniMacPad is a small project, so this is short.

## Building

You need a Mac, [Rust](https://rustup.rs) and the Xcode command-line tools.

```sh
cargo test --release          # build and run the tests
scripts/bundle.sh             # build MiniMacPad.app, sign it and install it to /Applications
scripts/bundle.sh --no-install   # build target/MiniMacPad.app only
```

`scripts/bundle.sh` signs with the "MiniMacPad Dev" identity if your keychain has one
(`scripts/create-signing-identity.sh` makes your own) and falls back to an ad-hoc signature. With an
ad-hoc signature macOS forgets the app's privacy permissions on every build, and the app can't update
itself.

To try the pieces without the app: `cargo run --release --bin minipad -- monitor` prints what the pad
sends, and `cargo run --release --bin minipad -- run` runs the background service in the terminal.

## Layout

| Path | What it is |
|---|---|
| `src/main.rs` | `minipad`, the background service and command-line tool |
| `src/ui.rs` | `minipad-ui`, the settings app |
| `src/device.rs` | finding the pad and turning its reports into events |
| `src/flash.rs` | programming the pad's own memory |
| `src/config.rs` | the config file |
| `src/actions.rs` | carrying out actions |
| `src/macos.rs` | volume, synthetic key presses and permission prompts |
| `src/service.rs` | the login item |
| `src/update.rs` | the self-updater |
| `scripts/` | versioning, bundling and signing |

## Changes

- Work on `dev`; `master` only moves through pull requests.
- Write [Conventional Commits](https://www.conventionalcommits.org): `fix:` and `feat:` commits become
  releases, so the subject line ends up in the changelog. See [docs/RELEASING.md](docs/RELEASING.md).
- Keep it small: the service should stay a program that sleeps until a key is pressed.
- Adding a pad model? `minipad monitor --raw` shows its raw reports, and the protocol notes in
  `src/flash.rs` are the place to start.
