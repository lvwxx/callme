#!/bin/sh
# Runs the GUI's tests against a local server and a generated project.
#   scripts/test-app.sh                     headless
#   scripts/test-app.sh --target macos      in macOS windows (tests/screenshot.test.tsx saves app/screenshots/)
set -eu
here="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'kill "$server" 2>/dev/null; rm -rf "$work"' EXIT INT TERM
python3 -I "$here/app/tests/server.py" > "$work/port" &
server=$!
while [ ! -s "$work/port" ]; do sleep 0.1; done
port="$(cat "$work/port")"
mkdir -p "$work/project/.callme"
printf '{"dev": {"baseUrl": "http://127.0.0.1:%s"}}\n' "$port" > "$work/project/http-client.env.json"
echo '{"env": "dev"}' > "$work/project/.callme/state.json"
cat > "$work/project/api.http" <<'HTTP'
### 用户
# @name me
GET {{baseUrl}}/me
Accept: application/json

### 流式
# @name stream
GET {{baseUrl}}/sse
HTTP
cd "$here/app"
CALLME_PROJECT="$work/project" nsh test "$@"
