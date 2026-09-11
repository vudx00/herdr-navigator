# Release

This project uses SemVer and Git tags.

## Development branch release hold

Do not release the machine/pane development branch as S01-qualified. N07 and
S01 remain incomplete. Provider preferences and offline early-lifecycle tests
are partial evidence, not qualified live-target persistence or invoking-client
workspace history. Jump-back and durable live-target replay remain disabled.

The fake-terminal suite checks failure recovery, exact dispatch and independent
cleanup attempts. It does not qualify live terminal behavior, macOS compilation
or runtime, or native/client handoff. Independent final review is still required.
Herdr 0.9.0/protocol 22 cannot detect a silent restart at the same socket/version.

Process-owned creation and templates require managed-plugin co-location and
fresh revalidation. Machine New workspace remains unavailable. No version bump,
tag, publication, or release claim accompanies this partial lifecycle work.

## Checklist

1. Update `Cargo.toml` and `herdr-plugin.toml` to the same version.
2. Move changelog entries from `Unreleased` to the new version.
3. Run checks:

   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test
   cargo build --release
   ```

4. Commit, tag, and push:

   ```bash
   git tag v0.1.0
   git push origin main --tags
   ```

GitHub Actions builds release archives for Linux and macOS when a `v*` tag is pushed.
