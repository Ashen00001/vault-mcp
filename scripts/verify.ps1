# Smoke test: build release, talk JSON-RPC over stdio, run one read-only search.
# Env: VAULT_MCP_VAULT / VAULT_MCP_ARCHIVE (as the server), VERIFY_QUERY, VERIFY_BUDGET_MS (default 5000)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
function Fail($m) { Write-Output "VERIFY vault-mcp FAIL $m"; exit 1 }

& cargo build --release --quiet
if ($LASTEXITCODE -ne 0) { Fail 'build' }
$exe = Join-Path (Get-Location) 'target/release/vault-mcp.exe'
if (-not (Test-Path $exe)) { $exe = Join-Path (Get-Location) 'target/release/vault-mcp' }
if (-not (Test-Path $exe)) { Fail 'binary not found' }

$query = if ($env:VERIFY_QUERY) { $env:VERIFY_QUERY } else { 'session' }
$budget = if ($env:VERIFY_BUDGET_MS) { [int]$env:VERIFY_BUDGET_MS } else { 5000 }

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $exe
$psi.RedirectStandardInput = $true
$psi.RedirectStandardOutput = $true
$psi.UseShellExecute = $false
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = [Diagnostics.Process]::Start($psi)
$msgs = @(
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"verify","version":"0"}}}',
  '{"jsonrpc":"2.0","method":"notifications/initialized"}',
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}',
  ('{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_sessions","arguments":{"query":"' + $query + '","limit":3}}}')
)
$bytes = [Text.Encoding]::UTF8.GetBytes((($msgs -join "`n") + "`n"))
$p.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
$p.StandardInput.BaseStream.Flush()
$p.StandardInput.Close()
$out = $p.StandardOutput.ReadToEnd()
if (-not $p.WaitForExit($budget + 5000)) { $p.Kill(); Fail 'server hung' }
$ms = [int]$sw.ElapsedMilliseconds

$lines = $out -split "`n" | Where-Object { $_.Trim() }
$byId = @{}
foreach ($l in $lines) { try { $o = $l | ConvertFrom-Json; $byId[[string]$o.id] = $o } catch { Fail "non-JSON output line" } }
if (-not $byId['1'].result.serverInfo) { Fail 'initialize' }
$tools = @($byId['2'].result.tools).Count
if ($tools -lt 5) { Fail "tools/list returned $tools tools" }
$c = $byId['3'].result
if ($c.isError) { Fail 'search_sessions reported an error' }
$payload = $c.content[0].text | ConvertFrom-Json
if ($null -eq $payload.results -or $null -eq $payload.truncated) { Fail 'search_sessions result malformed' }
if ($ms -gt $budget) { Fail "too slow: ${ms}ms > ${budget}ms" }
Write-Output "VERIFY vault-mcp PASS tools=$tools latency_ms=$ms"
