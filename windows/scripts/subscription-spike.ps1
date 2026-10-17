<#
.SYNOPSIS
  Phase 0 of the "Claude subscription" mode: checks, on this machine, how the
  official `claude` CLI behaves when Coucou would drive it. Nothing in Coucou is
  changed or run.

.DESCRIPTION
  Runs the 7 checks of docs/SUBSCRIPTION_MODE.md and writes a report to
  %LOCALAPPDATA%\Coucou\claude-spike\report.md (plus the raw stream-json sample
  next to it). The report holds no token and no account identifier: only the
  `authMethod` / `apiProvider` / `subscriptionType` fields of `claude auth status`
  are kept.

  Claude Code's credentials are never read: the CLI authenticates itself. The
  child processes run without ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN and
  CLAUDE_CODE_OAUTH_TOKEN (nor ANTHROPIC_BASE_URL) so that the subscription login
  is what gets tested.

  Needs PowerShell 7.2+ (`pwsh`): Windows PowerShell 5.1 drops empty arguments
  such as `--tools ""` when it starts a program.

.EXAMPLE
  pwsh -File windows\scripts\subscription-spike.ps1
  pwsh -File windows\scripts\subscription-spike.ps1 -SkipQuota
#>
param(
    # Skip check 7 (10 voice-like turns, which use some of the 5-hour quota).
    [switch]$SkipQuota,
    # Go on even if `authMethod` is not "claude.ai" (only once you know the value
    # your subscription login really reports).
    [string]$AcceptAuthMethod = 'claude.ai',
    [int]$TimeoutSec = 120
)

$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSVersion -lt [version]'7.2') {
    Write-Error 'PowerShell 7.2+ is needed (pwsh). Windows PowerShell 5.1 drops empty arguments.'
}

$OutDir = Join-Path ($env:LOCALAPPDATA ?? [IO.Path]::GetTempPath()) 'Coucou\claude-spike'
$WorkDir = Join-Path $OutDir 'cwd'
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
$Report = [System.Collections.Generic.List[string]]::new()
$System = 'Tu es un assistant concis.'

function Say([string]$Line) { Write-Host $Line; $Report.Add($Line) }
function Section([string]$Title) { Say ''; Say "## $Title"; Say '' }
function Excerpt([string]$Text, [int]$Max = 300) {
    if (-not $Text) { return '' }
    $one = ($Text -replace '\s+', ' ').Trim()
    if ($one.Length -gt $Max) { $one.Substring(0, $Max) + '…' } else { $one }
}

# ── 1. Where `claude` is ──────────────────────────────────────────────────────

# Same order Coucou will use: the native build first, then npm. With npm, node +
# cli.js is run directly when it is there, so no `cmd /c` (and its quoting) is
# involved; `claude.cmd` through cmd is the last resort.
function Find-Claude {
    $home_ = $env:USERPROFILE ?? $HOME
    $native = @(
        (Join-Path $home_ '.local\bin\claude.exe'),
        (Join-Path $home_ '.local/bin/claude')
    ) | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
    if ($native) { return [pscustomobject]@{ Kind = 'native'; File = $native; Pre = @() } }

    if ($env:APPDATA) {
        $npm = Join-Path $env:APPDATA 'npm'
        $cli = Join-Path $npm 'node_modules\@anthropic-ai\claude-code\cli.js'
        $node = (Get-Command node -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source
        if ((Test-Path -LiteralPath $cli) -and $node) {
            return [pscustomobject]@{ Kind = 'npm (node cli.js)'; File = $node; Pre = @($cli) }
        }
        $cmd = Join-Path $npm 'claude.cmd'
        if (Test-Path -LiteralPath $cmd) {
            return [pscustomobject]@{ Kind = 'npm (cmd /c claude.cmd)'; File = $cmd; Pre = @() }
        }
    }
    $onPath = Get-Command claude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($onPath) { return [pscustomobject]@{ Kind = 'PATH'; File = $onPath.Source; Pre = @() } }
    return $null
}

function New-StartInfo([string[]]$ArgList) {
    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    if ($Claude.File -like '*.cmd') {
        # cmd.exe has its own quoting: /s /c "<whole line>" keeps the inner quotes.
        $quoted = @($Claude.File) + $ArgList | ForEach-Object {
            if ($_ -eq '') { '""' } elseif ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ }
        }
        $psi.FileName = $env:ComSpec
        $psi.Arguments = '/d /s /c "' + ($quoted -join ' ') + '"'
    } else {
        $psi.FileName = $Claude.File
        foreach ($a in @($Claude.Pre) + $ArgList) { $psi.ArgumentList.Add($a) }
    }
    $psi.WorkingDirectory = $WorkDir
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $psi.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    foreach ($v in 'ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'CLAUDE_CODE_OAUTH_TOKEN', 'ANTHROPIC_BASE_URL') {
        [void]$psi.Environment.Remove($v)
    }
    return $psi
}

# One run of `claude`: arguments as an array (no shell), optional stdin, timeout.
function Invoke-Claude([string[]]$ArgList, [string]$Stdin = $null, [int]$Timeout = $TimeoutSec) {
    $p = [System.Diagnostics.Process]::new()
    $p.StartInfo = New-StartInfo $ArgList
    $watch = [Diagnostics.Stopwatch]::StartNew()
    [void]$p.Start()
    $out = $p.StandardOutput.ReadToEndAsync()
    $err = $p.StandardError.ReadToEndAsync()
    if ($Stdin) { $p.StandardInput.Write($Stdin) }
    $p.StandardInput.Close()
    $timedOut = -not $p.WaitForExit($Timeout * 1000)
    if ($timedOut) { try { $p.Kill($true) } catch {} ; [void]$p.WaitForExit(5000) }
    $watch.Stop()
    [pscustomobject]@{
        Exit     = if ($timedOut) { 'timeout' } else { $p.ExitCode }
        Ms       = $watch.ElapsedMilliseconds
        Out      = $out.Result
        Err      = Excerpt $err.Result 500
        TimedOut = $timedOut
    }
}

function Read-Lines([string]$Text) {
    foreach ($l in ($Text -split "`r?`n")) {
        if ($l.Trim()) { try { $l | ConvertFrom-Json -Depth 50 } catch {} }
    }
}

# What a stream-json run did: final result, hook events, tool calls, limits.
function Summarize-Stream($Lines) {
    $result = $Lines | Where-Object { $_.type -eq 'result' } | Select-Object -Last 1
    $hooks = @($Lines | Where-Object { $_.type -eq 'system' -and "$($_.subtype)" -match 'hook' })
    $tools = @($Lines | Where-Object { $_.type -eq 'assistant' } |
        ForEach-Object { $_.message.content } | Where-Object { $_.type -in 'tool_use', 'server_tool_use' } |
        ForEach-Object { $_.name })
    $init = $Lines | Where-Object { $_.type -eq 'system' -and $_.subtype -eq 'init' } | Select-Object -First 1
    [pscustomobject]@{
        Result     = $result
        HookEvents = $hooks
        HookNames  = @($hooks | ForEach-Object { $_.hook_name ?? $_.hook_event ?? $_.subtype } | Sort-Object -Unique)
        ToolUses   = $tools
        Init       = $init
        Limits     = @($Lines | Where-Object { $_.type -eq 'rate_limit_event' })
        Kinds      = @($Lines | ForEach-Object { if ($_.subtype) { "$($_.type)/$($_.subtype)" } else { "$($_.type)" } } | Sort-Object -Unique)
    }
}

$Report.Add('# Subscription mode — phase 0 measurements')
$Report.Add('')
$Report.Add("Run on $(Get-Date -Format 'yyyy-MM-dd HH:mm'), $([Environment]::OSVersion.VersionString), pwsh $($PSVersionTable.PSVersion).")

Section '1. Executable'
$Claude = Find-Claude
if (-not $Claude) {
    Say '`claude` not found (native build, npm or PATH). Stopping.'
    $Report | Set-Content -Encoding utf8 (Join-Path $OutDir 'report.md'); exit 1
}
$v = Invoke-Claude @('--version') -Timeout 30
Say "- Kind: $($Claude.Kind)"
Say "- Path: ``$($Claude.File)`` $($Claude.Pre -join ' ')"
Say "- ``claude --version``: $(Excerpt $v.Out) ($($v.Ms) ms)"
foreach ($var in 'ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'CLAUDE_CODE_OAUTH_TOKEN', 'ANTHROPIC_BASE_URL') {
    if ([Environment]::GetEnvironmentVariable($var)) { Say "- Note: $var is set in this shell (removed for the child processes; value not read)." }
}

# ── 2. Login ──────────────────────────────────────────────────────────────────

Section '2. `claude auth status`'
$a = Invoke-Claude @('auth', 'status') -Timeout 30
$status = try { $a.Out | ConvertFrom-Json } catch { $null }
if (-not $status) {
    Say "Not JSON (exit $($a.Exit)): $(Excerpt $a.Out) $($a.Err)"
    $Report | Set-Content -Encoding utf8 (Join-Path $OutDir 'report.md'); exit 1
}
# Only these fields are kept: nothing that identifies the account.
foreach ($f in 'loggedIn', 'authMethod', 'apiProvider', 'subscriptionType') {
    if ($null -ne $status.$f) { Say "- ``$f``: $($status.$f)" }
}
Say "- Fields present: $(($status.PSObject.Properties.Name) -join ', ')"
if ($status.authMethod -ne $AcceptAuthMethod) {
    Say ''
    Say "**Stopped:** authMethod is ``$($status.authMethod)``, not ``$AcceptAuthMethod``. If this is your Pro login, rerun with ``-AcceptAuthMethod $($status.authMethod)``."
    $Report | Set-Content -Encoding utf8 (Join-Path $OutDir 'report.md'); exit 1
}

# ── 3. A minimal turn ─────────────────────────────────────────────────────────

Section '3. Minimal turn'
$base = @('--model', 'haiku', '--tools', '', '--strict-mcp-config', '--system-prompt', $System, '--no-session-persistence')

$r = Invoke-Claude (@('-p', 'Dis bonjour', '--output-format', 'json') + $base)
Say '`claude -p "Dis bonjour" --output-format json --model haiku --tools "" --strict-mcp-config --system-prompt "…" --no-session-persistence`'
Say ''
$j = try { $r.Out | ConvertFrom-Json -Depth 50 } catch { $null }
Say "- Exit $($r.Exit), total $($r.Ms) ms (process start to exit)"
if ($j) {
    Say "- JSON fields: $(($j.PSObject.Properties.Name) -join ', ')"
    Say "- result: $(Excerpt $j.result)"
    Say "- is_error: $($j.is_error), subtype: $($j.subtype), duration_ms: $($j.duration_ms), duration_api_ms: $($j.duration_api_ms)"
    Say "- session_id present: $([bool]$j.session_id)"
    Say "- total_cost_usd: $($j.total_cost_usd) (an API-price estimate, not what a subscription pays)"
    Say "- usage: $(Excerpt ($j.usage | ConvertTo-Json -Compress -Depth 5) 400)"
    if ($j.modelUsage) { Say "- models used: $(($j.modelUsage.PSObject.Properties.Name) -join ', ')" }
} else { Say "- Not JSON: $(Excerpt $r.Out) / stderr: $($r.Err)" }
$coldMs = $r.Ms

# The same through stdin, with every flag Phase 1 plans to pass.
$phase1 = @('-p', '--output-format', 'stream-json', '--verbose', '--restricted', '--model', 'haiku', '--strict-mcp-config',
    '--system-prompt', $System, '--disable-slash-commands', '--permission-mode', 'dontAsk',
    '--permission-prompts', 'none', '--setting-sources', '', '--tools', '',
    '--disallowedTools', 'Bash,PowerShell,Edit,MultiEdit,Write,Read,NotebookEdit,Glob,Grep,WebFetch,Agent,Task,Skill',
    '--settings', '{"permissions":{"allow":[],"deny":["Bash","Read"],"defaultMode":"dontAsk","disableBypassPermissionsMode":"disable"}}',
    '--no-session-persistence')
$r = Invoke-Claude $phase1 -Stdin 'Dis bonjour'
$s = Summarize-Stream @(Read-Lines $r.Out)
Say ''
Say '**The flags Coucou uses, prompt on stdin** (`-p --output-format stream-json --verbose --restricted --model haiku --strict-mcp-config --system-prompt … --disable-slash-commands --permission-mode dontAsk --permission-prompts none --setting-sources "" --tools "" --disallowedTools … --settings {…}`)'
Say ''
Say "- Exit $($r.Exit), $($r.Ms) ms, result: $(Excerpt $s.Result.result 120), is_error: $($s.Result.is_error)"
if ($s.Init) { Say "- init tools: [$($s.Init.tools -join ', ')], mcp_servers: $(Excerpt ($s.Init.mcp_servers | ConvertTo-Json -Compress) 200), permissionMode: $($s.Init.permissionMode), apiKeySource: $($s.Init.apiKeySource)" }
Say "- Line kinds: $($s.Kinds -join ', ')"
if ($r.Err) { Say "- stderr: $($r.Err)" }

# Constraint 3: --bare should not use the subscription login.
$r = Invoke-Claude (@('--bare', '-p', 'Dis bonjour', '--output-format', 'json') + $base) -Timeout 60
$j = try { $r.Out | ConvertFrom-Json -Depth 50 } catch { $null }
Say ''
Say '**`--bare`** (expected to fail: bare mode reads only ANTHROPIC_API_KEY / apiKeyHelper)'
Say ''
Say "- Exit $($r.Exit), is_error: $($j.is_error), result: $(Excerpt ($j.result ?? $r.Out) 200) $($r.Err)"

# ── 4. Do the user's hooks (Coucou's) run? ────────────────────────────────────

Section '4. Hooks during a `-p` turn'
Say "Working directory: ``$WorkDir`` (empty). Watch the island while this runs: a session showing up there means Coucou's hooks ran."
Say ''
$variants = [ordered]@{
    'no --setting-sources (default)' = @()
    '--setting-sources ""'           = @('--setting-sources', '')
    '--setting-sources project'      = @('--setting-sources', 'project')
    '--restricted'                   = @('--restricted')
}
$Isolation = $null
foreach ($name in $variants.Keys) {
    $args_ = @('-p', '--output-format', 'stream-json', '--verbose', '--include-hook-events') + $base + $variants[$name]
    $r = Invoke-Claude $args_ -Stdin 'Dis bonjour'
    $s = Summarize-Stream @(Read-Lines $r.Out)
    $ok = $s.Result -and -not $s.Result.is_error
    Say "- **$name**: exit $($r.Exit), $($r.Ms) ms, answered: $ok, hook events: $($s.HookEvents.Count) [$($s.HookNames -join ', ')]$(if (-not $ok) { ' — ' + (Excerpt ($s.Result.result ?? $r.Err) 200) })"
    if ($ok -and $s.HookEvents.Count -eq 0 -and -not $Isolation -and $variants[$name].Count -gt 0) { $Isolation = $variants[$name] }
    Start-Sleep -Seconds 2
}
Say ''
$shown = if ($Isolation) { '`' + (($Isolation | ForEach-Object { if ($_ -eq '') { '""' } else { $_ } }) -join ' ') + '`' } else { 'none (every variant ran hooks or failed)' }
Say "Retained isolation: $shown"
if (-not $Isolation) { $Isolation = @('--setting-sources', '') }

# ── 5. WebSearch on a subscription ────────────────────────────────────────────

Section '5. WebSearch'
$web = @('-p', '--output-format', 'stream-json', '--verbose', '--model', 'haiku', '--strict-mcp-config',
    '--system-prompt', $System, '--tools', 'WebSearch', '--allowedTools', 'WebSearch',
    '--permission-mode', 'dontAsk', '--permission-prompts', 'none', '--no-session-persistence') + $Isolation
$r = Invoke-Claude $web -Stdin "Quelles sont les principales actualités d'aujourd'hui ($(Get-Date -Format 'yyyy-MM-dd')) ? Cherche sur le web, réponds en deux phrases."
$s = Summarize-Stream @(Read-Lines $r.Out)
Say "- Exit $($r.Exit), $($r.Ms) ms, is_error: $($s.Result.is_error)"
Say "- init tools: [$($s.Init.tools -join ', ')]"
Say "- Tool calls: [$($s.ToolUses -join ', ')]"
Say "- usage.server_tool_use: $(($s.Result.usage.server_tool_use | ConvertTo-Json -Compress) ?? 'absent')"
Say "- permission_denials: $(Excerpt ($s.Result.permission_denials | ConvertTo-Json -Compress -Depth 5) 200)"
Say "- result: $(Excerpt $s.Result.result 300)"

# ── 6. A persistent process ───────────────────────────────────────────────────

# One user message per line on stdin; read stdout until that turn's `result`.
function Send-Turn($Proc, [string]$Text, $Sample, [int]$Timeout = 90) {
    $msg = @{ type = 'user'; message = @{ role = 'user'; content = $Text }; parent_tool_use_id = $null; session_id = '' } |
        ConvertTo-Json -Compress -Depth 5
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $Proc.StandardInput.WriteLine($msg); $Proc.StandardInput.Flush()
    $Sample.Add('>>> ' + $msg)
    $first = $null; $lines = [System.Collections.Generic.List[object]]::new()
    while ($true) {
        $left = $Timeout * 1000 - $watch.ElapsedMilliseconds
        if ($left -le 0) { return [pscustomobject]@{ Ms = 'timeout'; FirstMs = $first; Lines = $lines } }
        $task = $Proc.StandardOutput.ReadLineAsync()
        if (-not $task.Wait([int]$left)) { return [pscustomobject]@{ Ms = 'timeout'; FirstMs = $first; Lines = $lines } }
        $line = $task.Result
        if ($null -eq $line) { return [pscustomobject]@{ Ms = 'exited'; FirstMs = $first; Lines = $lines } }
        $Sample.Add('<<< ' + $line)
        $o = try { $line | ConvertFrom-Json -Depth 50 } catch { $null }
        if (-not $o) { continue }
        $lines.Add($o)
        if ($o.type -eq 'assistant' -and -not $first) { $first = $watch.ElapsedMilliseconds }
        if ($o.type -eq 'result') { return [pscustomobject]@{ Ms = $watch.ElapsedMilliseconds; FirstMs = $first; Lines = $lines } }
    }
}

function Start-Persistent([string]$SystemPrompt) {
    $args_ = @('-p', '--input-format', 'stream-json', '--output-format', 'stream-json', '--verbose',
        '--model', 'haiku', '--tools', '', '--strict-mcp-config', '--system-prompt', $SystemPrompt,
        '--disable-slash-commands', '--permission-mode', 'dontAsk', '--permission-prompts', 'none',
        '--no-session-persistence') + $Isolation
    $p = [System.Diagnostics.Process]::new()
    $p.StartInfo = New-StartInfo $args_
    [void]$p.Start()
    [void]$p.StandardError.ReadToEndAsync()
    return $p
}

function Stop-Persistent($Proc) {
    $Proc.StandardInput.Close()
    if (-not $Proc.WaitForExit(10000)) { try { $Proc.Kill($true) } catch {} ; return 'killed after 10 s' }
    return "exited $($Proc.ExitCode) after stdin closed"
}

Section '6. Persistent process (stream-json in and out)'
$sample = [System.Collections.Generic.List[string]]::new()
$watch = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Persistent $System
$t1 = Send-Turn $p 'Dis bonjour' $sample
Start-Sleep -Seconds 3
$alive = -not $p.HasExited
$t2 = Send-Turn $p 'Et maintenant dis au revoir, en rappelant ce que tu as dit avant.' $sample
$end = Stop-Persistent $p
$s1 = Summarize-Stream $t1.Lines; $s2 = Summarize-Stream $t2.Lines
Say "- Cold ``-p`` (check 3): $coldMs ms"
Say "- Turn 1 (includes start-up): result after $($t1.Ms) ms (first assistant line $($t1.FirstMs) ms) — $(Excerpt $s1.Result.result 80)"
Say "- Still running after turn 1: $alive"
Say "- Turn 2 (warm): result after $($t2.Ms) ms (first assistant line $($t2.FirstMs) ms) — $(Excerpt $s2.Result.result 80)"
Say "- Same session_id on both turns: $($s1.Result.session_id -and $s1.Result.session_id -eq $s2.Result.session_id)"
Say "- Line kinds: $((@($s1.Kinds) + @($s2.Kinds) | Sort-Object -Unique) -join ', ')"
Say "- End: $end"
$samplePath = Join-Path $OutDir 'stream-sample.jsonl'
$sample | Set-Content -Encoding utf8 $samplePath
Say "- Raw exchange: ``$samplePath``"

# ── 7. Quota ──────────────────────────────────────────────────────────────────

Section '7. Quota used by 10 voice-like turns'
if ($SkipQuota) {
    Say 'Skipped (-SkipQuota).'
} else {
    Write-Host ''
    Write-Host "Read the plan usage pill in Coucou now (it refreshes when an interactive Claude Code session updates its status line)."
    $before = Read-Host 'Before: 5-hour % and 7-day % (e.g. "12 / 30", empty if unknown)'
    $voice = "You are Coucou, a voice assistant living at the top of the screen of the user's Windows computer, a bit like Jarvis: quick, warm, a little playful. It is $(Get-Date -Format 'yyyy-MM-dd HH:mm').`nEverything you write is read aloud: answer in the language the user spoke, in one or two short sentences, no markdown, no lists, no emojis, no links.`nWhen you need an answer from the user, end with one short question. Otherwise never end with a question."
    $asks = 'Salut Coucou, ça va ?', 'Combien font 17 fois 23 ?', 'Donne-moi une idée de dîner rapide.',
        'Traduis « bonne nuit » en japonais.', "C'est quoi la capitale de l'Australie ?", 'Raconte une blague courte.',
        "Rappelle-moi ce que je t'ai demandé en premier.", 'Quel est le plus long fleuve de France ?',
        'Donne-moi un mot qui rime avec soleil.', 'Merci, à plus tard.'
    $p = Start-Persistent $voice
    $sample = [System.Collections.Generic.List[string]]::new()
    $cost = 0.0; $ins = 0; $outs = 0; $cache = 0; $limits = @(); $times = @()
    foreach ($ask in $asks) {
        $t = Send-Turn $p $ask $sample
        $s = Summarize-Stream $t.Lines
        $times += $t.Ms
        $cost += [double]($s.Result.total_cost_usd ?? 0)
        $ins += [int]($s.Result.usage.input_tokens ?? 0); $outs += [int]($s.Result.usage.output_tokens ?? 0)
        $cache += [int]($s.Result.usage.cache_read_input_tokens ?? 0) + [int]($s.Result.usage.cache_creation_input_tokens ?? 0)
        $limits += $s.Limits
    }
    [void](Stop-Persistent $p)
    Write-Host ''
    Write-Host 'Now open an interactive `claude`, let its status line refresh, then read the pill again.'
    $after = Read-Host 'After: 5-hour % and 7-day %'
    Say "- Per-turn latency (ms): $($times -join ', ')"
    Say "- Tokens over 10 turns: input $ins, output $outs, cache read+write $cache"
    Say "- Sum of total_cost_usd: $([math]::Round($cost, 4)) (API-price estimate; total_cost_usd may be cumulative per process — compare with the last turn's value)"
    Say "- Plan usage before: $before — after: $after"
    Say "- rate_limit_event lines seen: $($limits.Count)"
    foreach ($l in ($limits | Select-Object -Last 3)) { Say "  - $(Excerpt ($l | ConvertTo-Json -Compress -Depth 5) 300)" }
}

$path = Join-Path $OutDir 'report.md'
$Report | Set-Content -Encoding utf8 $path
Write-Host ''
Write-Host "Report: $path"
