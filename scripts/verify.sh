#!/usr/bin/env bash
# Smoke test: build release, talk JSON-RPC over stdio, run one read-only search.
# Env: VAULT_MCP_VAULT / VAULT_MCP_ARCHIVE (as the server), VERIFY_QUERY, VERIFY_BUDGET_MS (default 5000)
set -u
cd "$(dirname "$0")/.."
fail() { echo "VERIFY vault-mcp FAIL $*"; exit 1; }

cargo build --release --quiet || fail "build"
exe=target/release/vault-mcp
[ -x "$exe" ] || exe=target/release/vault-mcp.exe
[ -e "$exe" ] || fail "binary not found"

query=${VERIFY_QUERY:-session}
budget=${VERIFY_BUDGET_MS:-5000}
start=$(date +%s%N)
out=$(printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"verify","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"search_sessions\",\"arguments\":{\"query\":\"$query\",\"limit\":3}}}" \
  | "$exe") || fail "server exited non-zero"
ms=$(( ($(date +%s%N) - start) / 1000000 ))

echo "$out" | grep -q '"id":1.*"serverInfo"' || fail "initialize"
tools=$(echo "$out" | grep '"id":2' | grep -o '"inputSchema"' | wc -l)
[ "$tools" -ge 5 ] || fail "tools/list returned $tools tools"
call=$(echo "$out" | grep '"id":3')
echo "$call" | grep -q '"isError":false' || fail "search_sessions reported an error"
echo "$call" | grep -qF '\"results\"' || fail "search_sessions result malformed"
[ "$ms" -le "$budget" ] || fail "too slow: ${ms}ms > ${budget}ms"
echo "VERIFY vault-mcp PASS tools=$tools latency_ms=$ms"
