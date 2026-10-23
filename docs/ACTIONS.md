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

## Tagged releases

The [tag-triggered release workflow](../.github/workflows/release.yml) checks that the Git tag matches the package version and executes the Lua-based [release packager](../tools/package_release.lua). It builds and checks a Linux x86-64 archive with SHA-256, then creates a GitHub Release. Tags beginning with `v0.` are explicitly prereleases. This workflow is **not triggered by ordinary commits** and has no current claim of a published stable release. A `v1.0.0` tag remains gated on actual acceptance and crate versioning.

**CI acceptance:** [GitHub Actions #39946114053](https://github.com/sguzman/ferrua/actions/runs/39946114053) passed the source-build action consumer, Rust 1.96 compatibility, locked Rust tests and Lua-built/checksummed Linux archive. These are tested previews, not stable signed releases.

### Installing versioned binaries

The composite action accepts `with: {version: v0.1.0}` to install a specific release instead of compiling source (once the tag has been published). The [POSIX bootstrap installer](../scripts/install.sh) supports the same release and an optional installation prefix. It restricts the version string to semver, runs on Linux x86-64, downloads the archive and corresponding `.sha256` from GitHub Releases, validates the checksum record and tarball, and installs only the `fer` executable. For offline integration tests, `FERRUA_RELEASE_BASE` may be overridden **only to a loopback HTTP server**. Use `sh scripts/install.sh v0.1.0` to install into `~/.local/bin`. No Python is involved. The checksum confirms asset integrity relative to the same GitHub Release source; it does not independently authenticate its publisher.

CI additionally serves the **actual built tarball** via a temporary loopback HTTP fixture, installs it into a separate directory using the bootstrap installer, and executes the installed `fer` outside the repository checkout. This catches broken packaging or missing embedded runtime assets that a stub installer fixture would not detect. The test never downloads a public release or uses real credentials.

Consumers can additionally pin a known digest with `with: {version: v0.1.0, sha256: "<64 lowercase hex characters>"}` or by passing the expected SHA-256 as the third argument to `scripts/install.sh`. This compares an independently supplied digest after checking the GitHub checksum. CI exercises both matching and deliberately mismatching pinned digests. A digest must originate from a trusted independent verification if it is intended to protect against compromised release metadata.

CI now has per-job time limits and cancels superseded runs on the same branch/ref. This reduces wasted compute from indefinitely stalled test fixtures. [Run #39950728455](https://github.com/sguzman/ferrua/actions/runs/39950728455) independently exercised a packaged Linux binary outside its source checkout.

**Transport policy:** Official GitHub Release installation accepts HTTPS for the initial request **and every redirect**; the installer refuses plaintext downgrade. The only HTTP exception is a strictly constrained `127.0.0.1:PORT` loopback source for offline tests, where redirects remain HTTP-only. CI tests that nonloopback HTTP overrides fail before a network request.

The tag-triggered publication workflow additionally runs the five-dialect Lua smoke harness and installs/executes the **exact versioned archive** generated for the pushed tag, using a local HTTP fixture, before creating the GitHub Release. The acceptance test derives its version from the archive basename, so later versions do not inherit a hardcoded v0.1.0 assumption.

## Machine-readable CI acceptance status

On push to `main`, the CI workflow retains its four primary verification jobs: Linux formatting/lint/tests, Rust 1.96 compatibility, Lua 5.4 differential smoke, and the independent composite-action consumer. A small **fifth reporting job** runs after those jobs and publishes one GitHub commit status with context `ferrua/ci`. It reports success only if all four primary jobs succeeded; otherwise it reports failure. The status links directly to the corresponding Actions run. It does not add acceptance tests, replace the four job logs, or count as proof of native EndeavourOS acceptance. Only the reporter job receives `statuses: write`; test jobs retain read-only repository permissions.

The status enables lightweight automated verification through GitHub's commit-status API without depending on the Checks API or guessing workflow run identifiers. CI supersession/cancellation may leave an older commit without a final status; in that case the record is **unverified**, not accepted. Roadmap checks should reference verified commit status and, for detailed failures, their specific workflow run. [CI #40024909050](https://github.com/sguzman/ferrua/actions/runs/40024909050) verified all four primary jobs and successful publication of the `ferrua/ci` commit status.
