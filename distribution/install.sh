#!/bin/sh
# The release workflow stamps the repository locator and exact release version.
set -eu
repository=unstamped
release=unstamped

die() {
  printf 'rhythm: %s\n' "$*" >&2
  exit 1
}

find_mise() {
  for candidate in "$HOME/.local/bin/mise" "$HOME/.local/share/mise/bin/mise" /opt/homebrew/bin/mise /usr/local/bin/mise; do
    if [ -x "$candidate" ]; then
      printf '%s\n' "$candidate"
      return
    fi
  done
  command -v mise </dev/null
}

main() {
  selected=${RHYTHM_VERSION:-$release}
  [ "$repository" != unstamped ] && [ "$selected" != unstamped ] || die "Use the installer attached to a Rhythm release."
  case "$selected" in
    ''|*[!a-zA-Z0-9.+-]*) die "Supply an exact Rhythm release version." ;;
  esac
  [ "$(uname -s </dev/null)" = Darwin ] && [ "$(uname -m </dev/null)" = arm64 ] ||
    die "Rhythm releases require an Apple silicon Mac."
  if ! mise=$(find_mise); then
    mise=$HOME/.local/bin/mise
    script=$(mktemp </dev/null)
    trap 'rm -f "$script" </dev/null' EXIT
    /usr/bin/curl -fsSL https://mise.run -o "$script" </dev/null
    MISE_INSTALL_PATH=$mise sh "$script" </dev/null
    [ -x "$mise" ] || die "Could not install mise. See https://mise.jdx.dev/getting-started.html."
  fi
  "$mise" backends ls </dev/null | grep -qx packslip ||
    die "Upgrade mise to a version with the packslip backend."
  cd "$HOME"
  tool="packslip:github.com/$repository"
  MISE_YES=1 "$mise" use --global --pin \
    --tool-option "identity=https://github.com/$repository/.github/workflows/release.yml@refs/tags/v$selected" \
    --tool-option "issuer=https://token.actions.githubusercontent.com" \
    "$tool@$selected" </dev/null
  status=0
  result=$(MISE_YES=1 "$mise" exec -- rhythm --json install --open </dev/null) || status=$?
  printf '%s\n' "$result"
  case "$result" in
    *'"window":"background_session"'*|*'"window": "background_session"'*) status=0 ;;
  esac
  [ "$status" -eq 0 ] || die "Rhythm did not report a ready window. Run rhythm ui from the desktop to retry."
  case ":${PATH:-}:" in
    *":$HOME/.local/bin:"*) ;;
    *) printf '%s\n' 'To run rhythm directly, add this to your shell configuration:' '  export PATH="$HOME/.local/bin:$PATH"' >&2 ;;
  esac
}

main "$@"
