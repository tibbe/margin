#!/bin/sh
# Runs the checks for the files being committed (the staged ones), or all
# of them with --all. The pre-commit and pre-merge-commit hooks in
# tools/git-hooks run it. Prints a line per check, and a failing check's
# output.
#
#   tools/check.sh [--all]
set -eu
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

case ${1:-} in
    --all) files='' all=1 ;;
    '')
        # Every path a commit changes, renames as both of theirs, unquoted.
        files=$(git -c core.quotePath=false diff --cached --name-only --no-renames -z | tr '\0' '\n')
        all=''
        ;;
    *)
        echo "usage: tools/check.sh [--all]" >&2
        exit 2
        ;;
esac
# Not staged, or not tracked, read without refreshing the index.
unstaged=$(GIT_OPTIONAL_LOCKS=0 git -c core.quotePath=false status --porcelain --untracked-files=all |
    grep -v '^[MADRC] ' | cut -c4- || true)

# A hook runs with git's variables pointing at this repository, which would
# send the git commands in the checks (tests make repositories of their
# own) here.
# shellcheck disable=SC2046 # one variable name per word
unset $(git rev-parse --local-env-vars)

# Whether a file being committed, other than Markdown, matches the
# extended regex.
touches() {
    [ -n "$all" ] || printf '%s\n' "$files" | grep -v '\.md$' | grep -qE "$1"
}

rust='^(crates/|Cargo\.(toml|lock)$|rust-toolchain\.toml$|rustfmt\.toml$)'
# The app embeds the core, through the FFI crate.
macos='^(macos/|crates/(core|ffi)/|Cargo\.(toml|lock)$)'
# What each set of checks reads: the app's build compiles crates too.
reads=''
touches "$rust" && reads=$rust
touches "$macos" && reads='^(macos/|crates/|Cargo\.(toml|lock)$|rust-toolchain\.toml$)'
if [ -z "$reads" ]; then
    echo "no checks for these files"
    exit 0
fi

# The checks run on the working tree, so it must hold what is committed.
if [ -z "$all" ]; then
    other=$(printf '%s\n' "$unstaged" | grep -v '\.md$' | grep -E "$reads" || true)
    if [ -n "$other" ]; then
        printf 'error: the checks would also see changes not being committed:\n%s\n' "$other" >&2
        echo "Stage them, or commit them separately first." >&2
        exit 1
    fi
fi

log=$(mktemp)
trap 'rm -f "$log"' EXIT
failed=''

# Runs a check by name; on failure prints the end of its output.
run() {
    name=$1
    shift
    if "$@" >"$log" 2>&1; then
        echo "ok    $name"
    else
        echo "FAIL  $name: $*"
        tail -n 40 "$log"
        failed=1
    fi
}

if touches "$rust"; then
    run "cargo fmt" cargo fmt --all --check
    run "cargo clippy" env CARGO_BUILD_WARNINGS=deny cargo clippy -q --workspace --all-targets --message-format=short
    # The default members, which the app's build shares.
    run "cargo test" cargo test -q
    run "cargo doc" env CARGO_BUILD_WARNINGS=deny cargo doc -q --workspace --no-deps
fi

if touches "$macos"; then
    if command -v xcodebuild >/dev/null; then
        dirs="macos/Sources macos/App macos/MarginTests macos/MarginUITests macos/tools"
        # shellcheck disable=SC2086 # dirs is a list
        run "swift format" swift format lint --strict -p -r $dirs
        # The unit tests don't launch the app.
        run "macOS unit tests" xcodebuild -quiet -project macos/Margin.xcodeproj -scheme Margin \
            -destination "platform=macOS,arch=$(uname -m)" \
            test -only-testing:MarginTests SWIFT_TREAT_WARNINGS_AS_ERRORS=YES
    else
        echo "skip  macOS checks: no xcodebuild"
    fi
fi

[ -z "$failed" ]
