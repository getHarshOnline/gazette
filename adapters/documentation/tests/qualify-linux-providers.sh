#!/usr/bin/env bash
# Qualify the connected five-provider documentation fixture on an already
# provisioned Linux host. Provisioning is intentionally outside this runner:
# the fixture receives only exact absolute paths and run-bound identities.

set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "qualify-linux-providers.sh requires Linux" >&2
  exit 2
fi

required_file() {
  local variable="$1"
  local value="${!variable:-}"
  if [[ -z "$value" || "$value" != /* || ! -f "$value" || -L "$value" ]]; then
    echo "$variable must name an absolute regular file, not a symlink: ${value:-<unset>}" >&2
    exit 2
  fi
}

required_directory() {
  local variable="$1"
  local value="${!variable:-}"
  if [[ -z "$value" || "$value" != /* || ! -d "$value" ]]; then
    echo "$variable must name an absolute directory: ${value:-<unset>}" >&2
    exit 2
  fi
}

for variable in \
  GAZETTE_TEST_SPHINX_PYTHON \
  GAZETTE_TEST_RUSTDOC_STABLE \
  GAZETTE_TEST_NODE \
  GAZETTE_TEST_TYPEDOC \
  GAZETTE_TEST_GOMARKDOC \
  GAZETTE_TEST_SWAGGER
do
  required_file "$variable"
done
required_directory GAZETTE_TEST_GO_ROOT
required_file GAZETTE_TEST_CARGO

if [[ ! -f "$GAZETTE_TEST_GO_ROOT/bin/go" || -L "$GAZETTE_TEST_GO_ROOT/bin/go" ]]; then
  echo "GAZETTE_TEST_GO_ROOT/bin/go must be a regular Go executable" >&2
  exit 2
fi

qualification_root="$(mktemp -d "${RUNNER_TEMP:-/tmp}/gazette-documentation-providers.XXXXXX")"
trap 'rm -rf -- "$qualification_root"' EXIT

sha256_file() {
  sha256sum "$1" | awk '{print $1}'
}

capture_version() {
  local name="$1"
  shift
  local output="$qualification_root/$name.version"
  # Match captured generation: probe outside the provider checkout. TypeDoc's
  # version text otherwise changes with the caller's working directory.
  if ! (cd "$qualification_root" && "$@" >"$output"); then
    echo "$name version command failed" >&2
    return 2
  fi
  if [[ ! -s "$output" ]]; then
    echo "$name returned an empty version on stdout" >&2
    exit 2
  fi
  sha256_file "$output"
}

go_environment=(
  "PATH=$GAZETTE_TEST_GO_ROOT/bin"
  "GOROOT=$GAZETTE_TEST_GO_ROOT"
  "GOTOOLCHAIN=local"
  "GOENV=off"
  "GOWORK=off"
  "GOPROXY=off"
  "GOSUMDB=off"
  "GOFLAGS=-mod=readonly"
  "CGO_ENABLED=0"
  "GOCACHE=$qualification_root/go/build"
  "GOMODCACHE=$qualification_root/go/modules"
  "GOPATH=$qualification_root/go/gopath"
  "TMPDIR=$qualification_root/go/tmp"
)
mkdir -p \
  "$qualification_root/go/build" \
  "$qualification_root/go/modules" \
  "$qualification_root/go/gopath" \
  "$qualification_root/go/tmp"

export GAZETTE_TEST_SPHINX_PYTHON_SHA256
GAZETTE_TEST_SPHINX_PYTHON_SHA256="$(sha256_file "$GAZETTE_TEST_SPHINX_PYTHON")"
export GAZETTE_TEST_SPHINX_VERSION_SHA256
GAZETTE_TEST_SPHINX_VERSION_SHA256="$(capture_version sphinx env -i \
  PYTHONNOUSERSITE=1 PYTHONDONTWRITEBYTECODE=1 \
  "$GAZETTE_TEST_SPHINX_PYTHON" -B -s -c \
  "import sphinx; assert sphinx.__version__ == '8.2.3', sphinx.__version__; print('Sphinx ' + sphinx.__version__)")"

export GAZETTE_TEST_RUSTDOC_STABLE_SHA256
GAZETTE_TEST_RUSTDOC_STABLE_SHA256="$(sha256_file "$GAZETTE_TEST_RUSTDOC_STABLE")"
export GAZETTE_TEST_RUSTDOC_VERSION_SHA256
GAZETTE_TEST_RUSTDOC_VERSION_SHA256="$(capture_version rustdoc env -i \
  "$GAZETTE_TEST_RUSTDOC_STABLE" --version)"

export GAZETTE_TEST_NODE_SHA256
GAZETTE_TEST_NODE_SHA256="$(sha256_file "$GAZETTE_TEST_NODE")"
export GAZETTE_TEST_TYPEDOC_VERSION_SHA256
GAZETTE_TEST_TYPEDOC_VERSION_SHA256="$(capture_version typedoc env -i \
  "$GAZETTE_TEST_NODE" "$GAZETTE_TEST_TYPEDOC" --version)"

export GAZETTE_TEST_GOMARKDOC_SHA256
GAZETTE_TEST_GOMARKDOC_SHA256="$(sha256_file "$GAZETTE_TEST_GOMARKDOC")"
export GAZETTE_TEST_GOMARKDOC_VERSION_SHA256
GAZETTE_TEST_GOMARKDOC_VERSION_SHA256="$(capture_version gomarkdoc env -i \
  "${go_environment[@]}" "$GAZETTE_TEST_GOMARKDOC" --version)"

export GAZETTE_TEST_SWAGGER_SHA256
GAZETTE_TEST_SWAGGER_SHA256="$(sha256_file "$GAZETTE_TEST_SWAGGER")"
export GAZETTE_TEST_SWAGGER_VERSION_SHA256
GAZETTE_TEST_SWAGGER_VERSION_SHA256="$(capture_version swagger env -i \
  "${go_environment[@]}" "$GAZETTE_TEST_SWAGGER" version)"

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd -P)"
mdx_qualifier="$repository_root/adapters/documentation/tests/providers/typedoc/qualify-mintlify-mdx.mjs"
if [[ ! -f "$mdx_qualifier" || -L "$mdx_qualifier" ]]; then
  echo "Mintlify Markdown/HTML qualifier must be a checked-in regular file" >&2
  exit 2
fi
cd "$repository_root"

"$GAZETTE_TEST_CARGO" test --locked \
  -p gazette-launcher \
  --test documentation_domain_doc_cli \
  one_brand_owned_domain_doc_composes_installed_native_references_deterministically \
  -- \
  --ignored \
  --exact \
  --nocapture

GAZETTE_TEST_MDX_NODE="$GAZETTE_TEST_NODE" \
GAZETTE_TEST_MDX_QUALIFIER="$mdx_qualifier" \
GAZETTE_TEST_RUSTDOC_STABLE="$GAZETTE_TEST_RUSTDOC_STABLE" \
"$GAZETTE_TEST_CARGO" test --locked \
  -p gazette-adapter-mintlify \
  --test rustdoc \
  installed_rustdoc_markdown_html_compiles_renders_and_resolves_navigation \
  -- \
  --ignored \
  --exact \
  --nocapture
