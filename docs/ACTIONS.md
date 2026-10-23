# ⚙️ Using fer in GitHub Actions

Ferrua's root `action.yml` is a **composite GitHub Action**. It installs a stable Rust toolchain, builds `fer` from the checked-out action source and adds its runner-local installation to `PATH`. It does not require Python or global system-wide changes.

```yaml
name: Lua scripts
on: [push]
jobs:
  script:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: sguzman/ferrua@main
      - name: Run Lua automation
        run: fer -e 'print(6 * 7)'
```

This example follows the moving development `main` branch. **For a production consumer workflow, pin an audited full commit SHA**, or use an immutable versioned release reference after Ferrua publishes one. Avoid assuming the current development branch is a stable API. The composite action will compile Rust dependencies from Cargo; installation is slower than a release binary. A signed/pinned release download action remains a later improvement.

The CI workflow also includes an **action-consumer job** that uses `uses: ./` after checkout to test exactly what downstream repositories will invoke. The consumer runs both ordinary Lua scripts and Ferrua's Rust-backed subprocess example, with no Python invoked by project automation.

To run a checked-in script, use `fer scripts/task.lua`. To run a short command, use `fer -e 'print(42)'`. GitHub Actions YAML still defines triggers, jobs and permissions. Scripts themselves are Lua.

The action requires a runner with network access to the Rust toolchain and Cargo package index when building from source. It has not yet been validated on macOS, Windows, or ARM runners. The Linux x86-64 preview artifact is separate from this action and is not treated as a stable signed release.

The composite installer runs `cargo install --locked`, using the checked-in `Cargo.lock`. Ferrua's Linux CI also uses `--locked` for its Rust builds, and publishes a SHA-256 file alongside the preview binary. A separate Rust 1.96 compatibility job checks the minimum toolchain version declared by `Cargo.toml`.
