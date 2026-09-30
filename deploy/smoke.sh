#!/usr/bin/env bash
# Smoke-check a running signalbox front from outside: nothing but the login
# answers without a session, and the build has no dev login.
# usage: deploy/smoke.sh <base-url> <303|503>
#   303: /auth/login must redirect to the provider's authorize endpoint
#   503: the provider is not reachable (a local test run)
set -euo pipefail
base=${1:?usage: deploy/smoke.sh <base-url> <303|503>}
login=${2:?usage: deploy/smoke.sh <base-url> <303|503>}
fail=0
# check <path> <status> [location glob]
check() {
  local out got loc
  out=$(curl -sk -o /dev/null -w '%{http_code} %{redirect_url}' "$base$1" || true)
  got=${out%% *}
  loc=${out#* }
  if [[ $got != "$2" ]] || { [[ -n ${3:-} ]] && [[ $loc != $3 ]]; }; then
    echo "FAIL $1: $got $loc (want $2 ${3:-})"
    fail=1
  else
    echo "ok   $1: $got $loc"
  fi
}
check / 303 "$base/auth/login"
check /ws 401
check "/auth/dev?user=smoke" 404
check /auth/logout 200
if [[ $login == 303 ]]; then
  check /auth/login 303 "https://*/application/o/authorize/*"
else
  check /auth/login 503
fi
exit $fail
