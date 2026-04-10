#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
GHOSTTY_DIR="${ROOT_DIR}/ghostty"
PATCH_DIR="${ROOT_DIR}/patches/ghostty-upstream-rebase"

UPSTREAM_REMOTE_NAME="chostty-upstream"
UPSTREAM_REMOTE_URL="https://github.com/ghostty-org/ghostty.git"
UPSTREAM_REF="main"
TARGET_BRANCH="chostty-upstream-sync"
REPOINT_ORIGIN=0
COMMITTER_NAME=""
COMMITTER_EMAIL=""

usage() {
    cat <<'EOF'
Usage: ./scripts/update_ghostty.sh [options]

Refresh the Ghostty submodule from upstream and replay Chostty's minimal
embedded-Linux patch queue on top.

Options:
  --ref <ref>            Upstream ref to replay onto. Default: main
  --branch <name>        Local ghostty branch to reset to the upstream ref
                         before applying patches. Default: chostty-upstream-sync
  --remote-url <url>     Upstream Ghostty remote URL.
                         Default: https://github.com/ghostty-org/ghostty.git
  --remote-name <name>   Local remote name used for upstream fetches.
                         Default: chostty-upstream
  --repoint-origin       Also set ghostty's origin remote to the upstream URL.
                         This is optional and off by default.
  --committer-name <v>   Committer name used for git am if ghostty has no
                         local git identity configured.
  --committer-email <v>  Committer email used for git am if ghostty has no
                         local git identity configured.
  -h, --help             Show this help.

What the script does:
  1. Verifies the ghostty submodule and patch queue exist.
  2. Verifies the ghostty worktree is clean.
  3. Fetches the requested upstream ref.
  4. Resets the local sync branch to the fetched upstream commit.
  5. Applies patches/ghostty-upstream-rebase/*.patch with git am.

After it succeeds, review and test the ghostty submodule, then commit the
updated gitlink in the Chostty repository or push the resulting Ghostty branch
to your own fork.
EOF
}

die() {
    printf 'ERROR: %s\n' "$1" >&2
    exit 1
}

while (($# > 0)); do
    case "$1" in
        --ref)
            (($# >= 2)) || die "--ref requires a value"
            UPSTREAM_REF="$2"
            shift 2
            ;;
        --branch)
            (($# >= 2)) || die "--branch requires a value"
            TARGET_BRANCH="$2"
            shift 2
            ;;
        --remote-url)
            (($# >= 2)) || die "--remote-url requires a value"
            UPSTREAM_REMOTE_URL="$2"
            shift 2
            ;;
        --remote-name)
            (($# >= 2)) || die "--remote-name requires a value"
            UPSTREAM_REMOTE_NAME="$2"
            shift 2
            ;;
        --repoint-origin)
            REPOINT_ORIGIN=1
            shift
            ;;
        --committer-name)
            (($# >= 2)) || die "--committer-name requires a value"
            COMMITTER_NAME="$2"
            shift 2
            ;;
        --committer-email)
            (($# >= 2)) || die "--committer-email requires a value"
            COMMITTER_EMAIL="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown argument: $1"
            ;;
    esac
done

git -C "$GHOSTTY_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1 || \
    die "ghostty submodule is missing; run: git submodule update --init --recursive"
[ -d "$PATCH_DIR" ] || die "patch directory missing: $PATCH_DIR"

GHOSTTY_GIT_DIR="$(git -C "$GHOSTTY_DIR" rev-parse --git-dir)"

shopt -s nullglob
PATCH_FILES=("$PATCH_DIR"/*.patch)
shopt -u nullglob
[ ${#PATCH_FILES[@]} -gt 0 ] || die "no patch files found in $PATCH_DIR"

if ! git -C "$GHOSTTY_DIR" diff --quiet || ! git -C "$GHOSTTY_DIR" diff --cached --quiet; then
    die "ghostty worktree is dirty; commit, stash, or discard changes before running this script"
fi

if [ -d "$GHOSTTY_GIT_DIR/rebase-apply" ] || [ -d "$GHOSTTY_GIT_DIR/rebase-merge" ]; then
    die "ghostty has an in-progress rebase or git am operation"
fi

printf '== Ghostty Upstream Refresh ==\n'
printf 'Remote name: %s\n' "$UPSTREAM_REMOTE_NAME"
printf 'Remote URL:  %s\n' "$UPSTREAM_REMOTE_URL"
printf 'Upstream ref: %s\n' "$UPSTREAM_REF"
printf 'Target branch: %s\n' "$TARGET_BRANCH"
printf 'Patch queue: %s (%s patches)\n' "$PATCH_DIR" "${#PATCH_FILES[@]}"

if git -C "$GHOSTTY_DIR" remote get-url "$UPSTREAM_REMOTE_NAME" >/dev/null 2>&1; then
    git -C "$GHOSTTY_DIR" remote set-url "$UPSTREAM_REMOTE_NAME" "$UPSTREAM_REMOTE_URL"
else
    git -C "$GHOSTTY_DIR" remote add "$UPSTREAM_REMOTE_NAME" "$UPSTREAM_REMOTE_URL"
fi

if [ "$REPOINT_ORIGIN" -eq 1 ]; then
    git -C "$GHOSTTY_DIR" remote set-url origin "$UPSTREAM_REMOTE_URL"
fi

git -C "$GHOSTTY_DIR" fetch "$UPSTREAM_REMOTE_NAME" "$UPSTREAM_REF"

FETCHED_COMMIT="$(git -C "$GHOSTTY_DIR" rev-parse FETCH_HEAD)"
printf 'Fetched upstream commit: %s\n' "$FETCHED_COMMIT"

git -C "$GHOSTTY_DIR" checkout -B "$TARGET_BRANCH" "$FETCHED_COMMIT"

if [ -d "$GHOSTTY_GIT_DIR/rebase-apply" ] || [ -d "$GHOSTTY_GIT_DIR/rebase-merge" ]; then
    git -C "$GHOSTTY_DIR" am --abort || true
fi

if [ -z "$COMMITTER_NAME" ]; then
    COMMITTER_NAME="$(git -C "$GHOSTTY_DIR" config user.name || git -C "$ROOT_DIR" config user.name || true)"
fi
if [ -z "$COMMITTER_EMAIL" ]; then
    COMMITTER_EMAIL="$(git -C "$GHOSTTY_DIR" config user.email || git -C "$ROOT_DIR" config user.email || true)"
fi

[ -n "$COMMITTER_NAME" ] || die "no git committer name configured; pass --committer-name or set git config user.name"
[ -n "$COMMITTER_EMAIL" ] || die "no git committer email configured; pass --committer-email or set git config user.email"

printf 'Using committer: %s <%s>\n' "$COMMITTER_NAME" "$COMMITTER_EMAIL"

git -C "$GHOSTTY_DIR" \
    -c user.name="$COMMITTER_NAME" \
    -c user.email="$COMMITTER_EMAIL" \
    am "${PATCH_FILES[@]}"

printf '\nGhostty patch replay complete.\n'
printf 'Resulting branch: %s\n' "$TARGET_BRANCH"
printf 'Resulting commit: %s\n' "$(git -C "$GHOSTTY_DIR" rev-parse HEAD)"
printf '\nNext steps:\n'
printf '  1. Build/test Chostty against the updated ghostty checkout.\n'
printf '  2. Commit the updated ghostty gitlink in the Chostty repo if it looks good.\n'
printf '  3. Optionally push the ghostty branch to your own fork.\n'
