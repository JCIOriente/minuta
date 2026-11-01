#!/usr/bin/env bash
# Remove stale git worktrees: clean, unlocked ones whose branch has a merged or
# closed PR and no commits beyond it. Deletes the local branch, then prunes
# dangling worktree records.
# Run from any checkout of the repo. Needs `gh`.
set -euo pipefail

main=$(git worktree list --porcelain | awk '/^worktree /{print substr($0, 10); exit}')

git worktree prune

path=""; branch=""; locked=0
check() {
  [ -n "$path" ] && [ "$path" != "$main" ] || return 0
  [ -n "$branch" ] || { echo "keep   $path (detached HEAD)"; return 0; }
  if [ "$locked" = 1 ]; then echo "keep   $path (locked)"; return 0; fi
  if [ -n "$(git -C "$path" status --porcelain)" ]; then
    echo "keep   $path (uncommitted changes)"; return 0
  fi
  # Newest PR for the branch: "<STATE> <head sha>", or "NONE" when it has none.
  pr=$(gh pr list --head "$branch" --state all --json state,headRefOid \
    --jq '(.[0] // {state: "NONE"}) | "\(.state) \(.headRefOid // "")"')
  read -r state head <<<"$pr"
  case "$state" in
    MERGED|CLOSED)
      if [ "$(git rev-parse "refs/heads/$branch")" != "$head" ]; then
        echo "keep   $path (local commits not in PR)"; return 0
      fi
      git worktree remove "$path"
      git branch -D "$branch" >/dev/null
      echo "remove $path ($branch, PR $state)" ;;
    *) echo "keep   $path (PR $state)" ;;
  esac
}

# Collect first: removing worktrees while reading the list would change it.
records=$(git worktree list --porcelain)
records+=$'\n'
while IFS= read -r line; do
  case "$line" in
    "worktree "*) path=${line#worktree }; branch=""; locked=0 ;;
    "branch "*) branch=${line#branch refs/heads/} ;;
    locked*) locked=1 ;;
    "") check; path="" ;;
  esac
done <<<"$records"

git worktree prune
