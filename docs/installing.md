# Installing

How `herdr plugin install` gets the plugin binary, and how to check that
a release binary came from this repository. For the plugin's
requirements, see the [README](../README.md#requirements).

## Install

```sh
herdr plugin install mcavallo/herdr-command-palette
```

Herdr clones the repository's default branch (`master`, always the
latest release) and runs the manifest's `[[build]]` command,
`scripts/install.sh`, in the checkout. The script puts the binary at
`target/release/herdr-command-palette`, where the manifest's actions
and panes run it:

1. Reads the version from `Cargo.toml` and the target from `uname`.
2. Downloads that version's archive and its `.sha256` file from the
   GitHub release, checks the SHA-256, and extracts the binary.
3. If there is no prebuilt binary for the platform, the download fails
   or the checksum doesn't match, builds from source with
   `cargo build --release --locked` instead.
4. If cargo isn't found either, the install stops with an error.

Herdr only shows the script's output when it fails, so a source build
(a minute or two) runs silently.

Update with `herdr plugin update`. It fetches `master` again and runs
the same script.

### What the script needs

- `curl`, `tar`, and `sha256sum` or `shasum` for the download.
- `git`, to read the repository from the checkout's `origin` remote.
- For a source build: [Rust](https://rustup.rs), at least the
  `rust-version` in `Cargo.toml`. The script also looks for cargo in
  `~/.cargo/env`, since Herdr's environment may not have it on `PATH`.

| Variable           | Default                     | What it does                                                               |
| ------------------ | --------------------------- | -------------------------------------------------------------------------- |
| `HCP_INSTALL_REPO` | the checkout's `origin` URL | GitHub repository (`owner/repo`) to download the release from, e.g. a fork |

### Release assets

Each release has one archive per target, plus a checksum file:

| Platform            | Target                       |
| ------------------- | ---------------------------- |
| Linux x86_64        | `x86_64-unknown-linux-musl`  |
| Linux arm64         | `aarch64-unknown-linux-musl` |
| macOS Apple silicon | `aarch64-apple-darwin`       |
| macOS Intel         | `x86_64-apple-darwin`        |

Names: `herdr-command-palette-<target>.tar.gz`, with the binary at the
archive root, and `herdr-command-palette-<target>.tar.gz.sha256`. The
Linux binaries are static.

### Build from source

On any other platform, the script builds from source automatically. To
build and link a checkout yourself, for example to try changes, see
[development.md](development.md#build-and-install-from-source).

## Verify a release

Each release archive has a build provenance attestation. It proves the
archive was built by GitHub Actions, by this repository's release
workflow, from the tagged source. The attestation is signed through
[Sigstore](https://www.sigstore.dev) with the workflow's GitHub
identity.

`install.sh` checks only the SHA-256, which catches a corrupted
download but not a replaced release. Verifying the attestation is a
separate, manual step with the [GitHub CLI](https://cli.github.com).

Download an archive and its checksum, and check the checksum:

```sh
# The latest release; or a tag such as v0.2.0
v=$(gh release view --repo mcavallo/herdr-command-palette --json tagName --jq .tagName)
gh release download "$v" --repo mcavallo/herdr-command-palette \
  --pattern 'herdr-command-palette-x86_64-unknown-linux-musl.tar.gz*'
sha256sum -c herdr-command-palette-x86_64-unknown-linux-musl.tar.gz.sha256
```

On macOS, use `shasum -a 256 -c` instead of `sha256sum -c`.

Then verify its attestation:

```sh
gh attestation verify herdr-command-palette-x86_64-unknown-linux-musl.tar.gz \
  --repo mcavallo/herdr-command-palette \
  --signer-workflow mcavallo/herdr-command-palette/.github/workflows/build.yml
```

`--signer-workflow` names `build.yml` because it is a reusable workflow:
it signs the attestation, called by the release workflow.

All attestations are listed on the repository's
[attestations page](https://github.com/mcavallo/herdr-command-palette/attestations).
For more options, such as offline verification, see the
[`gh attestation verify` manual](https://cli.github.com/manual/gh_attestation_verify).
