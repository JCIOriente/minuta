#!/usr/bin/env bash
# Tell whether a version of a crate is already on crates.io, without credentials.
# Usage: crate-published.sh <crate> <X.Y.Z>
# Exit 0 and print "published" if the version exists (a yanked one counts: crates.io never
# accepts the same version again), exit 1 and print "not published" on 404, exit 2 on any
# other answer or a failed request, so an outage is never read as "not published".
# crates.io rejects requests without a User-Agent (403).
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: crate-published.sh <crate> <X.Y.Z>" >&2
  exit 2
fi
crate=$1
version=$2
api=${CRATES_IO_API:-https://crates.io/api/v1/crates}

if ! [[ "$crate" =~ ^[A-Za-z0-9_-]+$ ]]; then
  echo "crate-published: crate name '$crate' is not valid" >&2
  exit 2
fi
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "crate-published: version '$version' is not X.Y.Z" >&2
  exit 2
fi

body=$(mktemp)
trap 'rm -f "$body"' EXIT
if ! status=$(curl --silent --show-error --location --max-time 30 \
  --user-agent "minuta-release (https://github.com/JCIOriente/minuta)" \
  --output "$body" --write-out '%{http_code}' "$api/$crate/$version"); then
  echo "crate-published: request to crates.io failed" >&2
  exit 2
fi

case "$status" in
  200)
    # Check the answer is for this version, not some other 200 page.
    if ! grep -Eq "\"num\" *: *\"${version//./\\.}\"" "$body"; then
      echo "crate-published: crates.io answered 200 without version $version" >&2
      exit 2
    fi
    echo published
    ;;
  404)
    echo "not published"
    exit 1
    ;;
  *)
    echo "crate-published: crates.io answered HTTP $status for $crate $version" >&2
    exit 2
    ;;
esac
