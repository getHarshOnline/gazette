#!/bin/sh
set -eu

repository=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
if [ "$#" -gt 0 ]; then
    binary=$1
    if [ ! -x "$binary" ]; then
        printf '%s\n' 'the selected Gazette executable is not executable' >&2
        exit 1
    fi
    run_gazette() {
        "$binary" "$@"
    }
else
    run_gazette() {
        cargo run --quiet --manifest-path "$repository/Cargo.toml" --locked --bin gazette -- "$@"
    }
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/gazette-quickstart.XXXXXX")
cleanup() {
    rm -rf -- "$work"
}
trap cleanup EXIT HUP INT TERM

source_root="$work/source"
output_parent="$work/output"
mkdir -p "$source_root/theme/templates" "$source_root/content" "$output_parent"
# Native source selection requires an absolute physical root. macOS commonly
# exposes the temporary directory through /var while its canonical path is
# /private/var, so resolve it before placing it in the workspace contract.
source_root=$(CDPATH= cd -- "$source_root" && pwd -P)
output_parent=$(CDPATH= cd -- "$output_parent" && pwd -P)

git_fixture() {
    GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
        git -c core.hooksPath=/dev/null -c commit.gpgsign=false "$@"
}

cat >"$source_root/theme/theme.toml" <<'EOF'
package='theme:quickstart'
namespace='quickstart'
templates='templates'
browser_theme_colors={light='#ffffff',dark='#000000'}
render={page='page.html'}
EOF

cat >"$source_root/theme/templates/page.html" <<'EOF'
<!doctype html>
<html lang="en">
  <head><meta charset="utf-8"><title>{{ content.title }}</title></head>
  <body><main><h1>{{ content.title }}</h1>{{ body_html | safe }}</main></body>
</html>
EOF

cat >"$source_root/content/index.md" <<'EOF'
+++
[content]
id='content:quickstart'
site='site:quickstart'
title='Gazette quickstart'
lifecycle='published'
route={path='/'}
structure={kind='page',family={kind='article'}}
+++

This page was compiled from a pinned workspace by the standalone `gazette` command.
EOF

git_fixture -C "$source_root" init --quiet --object-format=sha1 --template=
git_fixture -C "$source_root" add -- theme content
GIT_AUTHOR_NAME='Gazette quickstart' \
GIT_AUTHOR_EMAIL='quickstart@example.invalid' \
GIT_AUTHOR_DATE='2000-01-01T00:00:00Z' \
GIT_COMMITTER_NAME='Gazette quickstart' \
GIT_COMMITTER_EMAIL='quickstart@example.invalid' \
GIT_COMMITTER_DATE='2000-01-01T00:00:00Z' \
    git_fixture -C "$source_root" commit --quiet --message='quickstart fixture'
commit=$(git_fixture -C "$source_root" rev-parse HEAD)

workspace="$work/workspace.toml"
cat >"$workspace" <<EOF
[composition]
theme={package='theme:quickstart'}

[[principals]]
id='owner:quickstart'
kind='person'

[[sites]]
id='site:quickstart'
publisher='owner:quickstart'
deployable=false

[[brands]]
id='brand:quickstart'
name='Gazette quickstart'

[[brands.publications]]
site='site:quickstart'
public_base_url='https://quickstart.example/'
authored=[{source='source:content',target='content'}]

[[sources]]
id='source:theme'
location={kind='repository',root='$source_root',commit='$commit',subdirectory='theme'}

[[theme_packages]]
package='theme:quickstart'
source='source:theme'
manifest='theme.toml'

[[sources]]
id='source:content'
location={kind='repository',root='$source_root',commit='$commit',subdirectory='content'}
EOF

receipt="$work/receipt.json"
run_gazette render-site \
    --local \
    --site site:quickstart \
    --model-output model.json \
    --output-parent "$output_parent" \
    <"$workspace" >"$receipt"

page=$(find "$output_parent" -type f -name index.html -print)
model=$(find "$output_parent" -type f -name model.json -print)
test -n "$page"
test -n "$model"
test "$(printf '%s\n' "$page" | wc -l | tr -d ' ')" = 1
test "$(printf '%s\n' "$model" | wc -l | tr -d ' ')" = 1
grep -q 'Gazette quickstart' "$page"
grep -q 'content:quickstart' "$model"
grep -Eq '"status"[[:space:]]*:[[:space:]]*"local_retained"' "$receipt"

printf '%s\n' 'Gazette quickstart passed: pinned workspace -> shared model -> retained local HTML.'
printf '%s\n' 'This disposable output is private and draft-inclusive; it is not admission, certification or deployment.'
