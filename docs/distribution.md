# Distribution

Rhythm uses mise for pinned development tools and for installing signed
packslip releases. The initial release target is `aarch64-apple-darwin`, with
macOS 13 or newer. The release job builds and tests that target natively.
Other release targets require their own native verification before being added.
Source checks still run on macOS and Linux.

## Install a release

From this repository, with mise installed and GitHub access configured:

```sh
repository=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
mise use -g "packslip:github.com/$repository@0.2.0"
rhythm --version
rhythm demo
```

Or replace `OWNER/REPOSITORY` in `mise use -g
packslip:github.com/OWNER/REPOSITORY@0.2.0` with the repository you trust.
An exact version avoids ambiguity about which release is installed. A private
repository needs a GitHub token with read access, such as mise's existing gh
credential integration or `MISE_GITHUB_TOKEN`. Do not put credentials in
configuration, URLs, scripts, or review artifacts.

The packslip backend verifies the repository identity, signed manifest, requested
version, artifact digest and size before extracting anything. Its first install
records the signer; later installations enforce continuity. Use a project
`mise.toml` plus `mise lock` to carry the signer and artifact commitments between
environments. A failed verification is a failed install: do not replace the
`packslip:` backend with an unverified download or a local build.

The archive contains the executable at its root and a short README. A source
checkout and the packslip CLI are not runtime dependencies.

## Development tasks

The repository pins Python, Rust, packslip, actionlint, and Ruff.
`mise.lock` records available downloads/checksums for the development CI platforms;
Rust uses rustup's exact toolchain version from the matching toolchain file.
The mise Rust entry explicitly installs `rustfmt` and Clippy, including on fresh
runners whose rustup profile is minimal.

```sh
mise trust mise.toml
mise install --locked
mise run check
mise run test
mise run dist:build
mise run dist:rehearse
```

`check` runs Rust formatting/lints, validates workflows, and checks/tests the
distribution scripts. `test` runs the Rust tests. `dist:build` creates an
optimized native executable and writes the versioned archive to `dist/`.

Packaging refuses a target that cannot be tested natively, an executable reporting
a different version, or an executable containing the builder's private home
path. Rust paths are remapped and debug information is stripped. Archive entries
have fixed names, modes, owner IDs, and timestamps; there are no local usernames,
extended attributes, working directories, or inventory records in the archive.
This makes packaging deterministic for the same executable; it is not a claim
that compiler output is reproducible across toolchains or hosts.

## Release policy and CI

Like Mergegate, Rhythm publishes from a `vX.Y.Z` tag. A merge to `main` runs
source CI without publishing. For a release tag, the Release workflow:

1. Requires the tag to match `Cargo.toml` exactly and the tagged commit to belong
   to reviewed `main` history. A mismatched version or unmerged commit fails
   before a release build starts.
2. Installs pinned tools without caches, runs checks and Rust tests, and
   builds/packages the native release.
3. Signs a disposable candidate with an unlogged temporary test key. It verifies
   the archive, rejects a modified file and a different signer, extracts into a
   temporary installation, and runs real CLI flows
   using that exact executable.
4. Signs the built archive with packslip using the signing job's GitHub OIDC
   identity. Keyless signatures are logged. The signing job has no repository
   write permission. Public repositories also request GitHub build attestations;
   private repositories skip that separate Enterprise-only service while keeping
   packslip signature and artifact verification mandatory.
5. Publishes the archive and `packslip.sigstore.json` together under `vVERSION`
   using a separate job with repository write permission and no OIDC permission.
   Publishing requires the existing tag (`--verify-tag`). Existing releases and
   assets are never overwritten; trying to publish the same tag again fails.
   SemVer prereleases are marked as prereleases rather than stable releases.
6. Downloads the published files, verifies the expected signing workflow and
   source commit, installs the exact version through mise in fresh isolated
   directories, compares the installed executable with the verified archive,
   and repeats the CLI checks.

Pull requests run the candidate build and tests without production signing,
creating tags, or publishing releases. Release builds use fresh runners and no
build caches. The signing job has OIDC permission but no repository write access;
the publishing job has repository write access but no OIDC permission. Those
boundaries and publishing every asset in one command follow Mergegate’s workflow.

To release, first merge the distribution workflow and the intended Cargo version
through review. Fetch the reviewed target, inspect the commit, then create the
version tag through GitHub. For version 0.2.0:

```sh
git fetch mergegate
repository=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
commit=$(git rev-parse mergegate/main)
# Confirm this reviewed commit's Cargo version is 0.2.0 before tagging.
git show "$commit:Cargo.toml"
gh api "repos/$repository/git/refs" --method POST \
  -f ref=refs/tags/v0.2.0 -f sha="$commit"
```

A tag is the explicit release action; it never advances a branch. Do not move or
replace a release tag. Version bumps go through a reviewed change before the next
tag. The signing identity is `.github/workflows/release.yml` at the exact release
tag, so verification pins `@refs/tags/vVERSION` and GitHub’s OIDC issuer.

If a release workflow fails, inspect the failing job and rerun that job after
resolving the cause. Do not delete a signature, replace published files, clear
signer pins, or disable checks to make installation succeed. A new version is
needed to correct an already published executable. An interrupted draft or a tag
pointing at an unintended commit requires investigation rather than automatic
replacement.

## Verify an installed distribution

The verification script deliberately creates a separate mise config, cache,
data directory, state directory, and install tree. It does not change global
mise selection or the normal Rhythm state directory:

```sh
repository=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
commit=$(git rev-parse HEAD)
mise exec -- python distribution/install.py \
  --repository "$repository" --version 0.2.0 --commit "$commit"
```

Run this at the release commit, or supply that commit explicitly. The expected
project, version, and source commit must all match the verified statement.
The script accepts only logged production signatures from the expected workflow
and GitHub OIDC issuer, and pins both in the isolated mise configuration.
It has no test-key or unlogged bypass. Its receipt contains generic pass/fail
fields, version, and archive digest; it does not publish private paths or state.

On macOS with an available user launchd domain, add `--launchd` to register a
disposable failing agent and confirm that the installed executable observes its
failure. The registration is removed afterward. Every definition in these
checks uses a temporary home directory and a random label.

The local candidate rehearsal uses disposable test keys and never publishes them.
It verifies packaging and execution before review; production OIDC signing and
mise release discovery are separately verified by the post-publication job.
Passing a candidate rehearsal alone does not prove a published release exists.

## Upgrading from the scheduler releases

Releases up to 0.1.1 ran their own scheduler as a `dev.rhythm.runtime.*`
agent. `rhythm status` lists such a registration like any other job. Retire it
with the old executable's `rhythm runtime uninstall` before switching versions,
or with `launchctl bootout` and removing its plist. Its private state directory
is not read by newer releases.
