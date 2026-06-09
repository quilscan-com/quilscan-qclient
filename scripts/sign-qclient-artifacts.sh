#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: sign-qclient-artifacts.sh <version> <dist-dir>

Required environment:
  DEV_NODE_SIGNING_PRIVATE_KEY  Base64-encoded Ed25519 private key.

Optional environment:
  AGENT_SIGNING_PRIVATE_KEY       Base64-encoded Ed25519 private key fallback.
  QCLIENT_RELEASE_BASE_URL        Base URL that will host these artifacts.
EOF
}

if [[ $# -lt 1 || $# -gt 2 ]]; then
  usage
  exit 2
fi

version="$1"
dist_dir="${2:-dist}"

if [[ -z "${DEV_NODE_SIGNING_PRIVATE_KEY:-}${AGENT_SIGNING_PRIVATE_KEY:-}" ]]; then
  echo "DEV_NODE_SIGNING_PRIVATE_KEY or AGENT_SIGNING_PRIVATE_KEY is required" >&2
  exit 1
fi

if [[ ! "$version" =~ ^[0-9]+(\.[0-9]+){2,3}$ ]]; then
  echo "version must be numeric dotted format, for example 2.1.0.23" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

binaries_file="$tmp_dir/qclient-binaries"
find "$dist_dir" -maxdepth 1 -type f \
  -name "qclient-${version}-*" \
  ! -name "*.dgst" \
  ! -name "*.sig.*" \
  ! -name "qclient-release" \
  | sort > "$binaries_file"

if [[ ! -s "$binaries_file" ]]; then
  echo "no qclient artifacts found in $dist_dir for version $version" >&2
  exit 1
fi

manifest="$dist_dir/qclient-release"
manifest_tmp="$tmp_dir/qclient-release"
version_json="$dist_dir/qclient-version.json"
version_json_tmp="$tmp_dir/qclient-version.json"
: > "$manifest_tmp"

count=0
{
  printf '{\n'
  printf '  "schema": 1,\n'
  printf '  "channel": "quilscan-qclient",\n'
  printf '  "version": "%s",\n' "$version"
  printf '  "generated_at": "%s",\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  printf '  "base_url": "%s",\n' "${QCLIENT_RELEASE_BASE_URL:-}"
  printf '  "manifest": "qclient-release",\n'
  printf '  "files": [\n'
} > "$version_json_tmp"

while IFS= read -r binary; do
  digest="${binary}.dgst"
  signature="${binary}.sig"
  binary_name="$(basename "$binary")"
  digest_name="$(basename "$digest")"
  signature_name="$(basename "$signature")"
  platform="${binary_name#qclient-${version}-}"

  openssl sha3-256 -out "$digest" "$binary"
  go run ./scripts/sign-qclient-artifacts.go "$binary"
  sha3_256="$(sed 's/^.*= //' "$digest" | tr -d '\r\n')"

  if [[ "$count" -gt 0 ]]; then
    printf ',\n' >> "$version_json_tmp"
  fi
  {
    printf '    {\n'
    printf '      "platform": "%s",\n' "$platform"
    printf '      "binary": "%s",\n' "$binary_name"
    printf '      "digest": "%s",\n' "$digest_name"
    printf '      "signature": "%s",\n' "$signature_name"
    printf '      "signature_type": "ed25519-binary",\n'
    printf '      "sha3_256": "%s"\n' "$sha3_256"
    printf '    }'
  } >> "$version_json_tmp"

  printf '%s\n' "$binary_name" >> "$manifest_tmp"
  printf '%s\n' "$digest_name" >> "$manifest_tmp"
  printf '%s\n' "$signature_name" >> "$manifest_tmp"
  count=$((count + 1))
done < "$binaries_file"

{
  printf '\n'
  printf '  ]\n'
  printf '}\n'
} >> "$version_json_tmp"

printf '%s\n' "qclient-version.json" >> "$manifest_tmp"
sort -u "$manifest_tmp" > "$manifest"
mv "$version_json_tmp" "$version_json"
echo "signed $count qclient artifact(s)"
