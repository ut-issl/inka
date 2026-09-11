set -euo pipefail

if [[ "${RUNNER_OS:-}" != Linux ]]; then
  echo 'Publish inka evidence currently supports Linux runners only.' >&2
  exit 1
fi

# Upload exactly the snapshot that was validated, without interpreting the
# caller's filename as an upload-artifact glob or including neighbouring files.
staging=$(mktemp -d "${RUNNER_TEMP:?}/inka-evidence.XXXXXXXX")
trap 'rm -rf -- "$staging"' ERR
cp -- "${INKA_BUNDLE:?}" "$staging/evidence.json"
"${INKA_BIN:?}" ci-export validate "$staging/evidence.json"
printf 'directory=%s\n' "$staging" >> "${GITHUB_OUTPUT:?}"
