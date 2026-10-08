#!/bin/sh
set -eu
case "$APPCOAT_PLATFORM" in macos|linux) ;; *) echo 'This example needs a host C compiler for macos or linux' >&2; exit 1;; esac
mkdir -p build
"${CC:-cc}" -std=c11 -Wall -Wextra -Werror main.c -o build/custom-c
