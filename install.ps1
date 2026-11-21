# PES 브리지(pes-bridge) 원클릭 설치 — Windows (PowerShell)
#   irm https://raw.githubusercontent.com/clazic/pes-bridge/main/install.ps1 | iex
#
# 하는 일(다시 실행해도 된다 — 없는 것만 설치하고, worker 파일은 최신판으로 바꾼다)
#   1. 이 저장소를 %USERPROFILE%\.pes\bridge 에 받는다
#   2. Git(Git Bash) · Node.js 26+ 가 없으면 winget 으로 설치한다(pes-scripts\bootstrap.ps1 — clazic/pes 의 것을 그대로 따라 받는다)
#   3. Claude Code 가 없으면 공식 설치기(https://claude.ai/install.ps1)로 받고, 로그인이 안 돼 있으면 로그인 창을 연다
#   4. 조사 도구(kosis · vworld · sgis · korean-law · kordoc)를 설치한다(worker\tools.ps1)
#   5. 브라우저 ego lite 가 없으면 내려받기 페이지를 연다(Windows 는 설치 스크립트가 없다)
#   6. API 키(KOSIS · VWorld · SGIS · LAW_OC)를 단계별로 묻는다(처음 · 빠진 키가 있을 때)
#   7. 작업 스케줄러에 「PES Bridge」를 등록해 로그인하면 창 없이 켜지게 한다(꺼지면 5초 뒤 다시 켠다)
# 환경변수: PES_BRIDGE_REPO(기본 clazic/pes-bridge) · PES_BRIDGE_SRC(받지 않고 이 폴더를 쓴다 — 시험용)
#           PES_BRIDGE_TEST=1(시험: 로그인 · 키 · ego lite · 작업 등록을 건너뛴다)
$ErrorActionPreference = "Continue"
try { [Console]::OutputEncoding = [Text.Encoding]::UTF8 } catch { }
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$Repo = if ($env:PES_BRIDGE_REPO) { $env:PES_BRIDGE_REPO } else { "clazic/pes-bridge" }
$Dest = Join-Path $env:USERPROFILE ".pes\bridge"
$Here = Join-Path $Dest "worker"
$Test = [bool]$env:PES_BRIDGE_TEST
$Interactive = (-not $Test) -and (-not $env:CI) -and [Environment]::UserInteractive
$PsExe = (Get-Process -Id $PID).Path
function Step($n, $t) { Write-Host "`n[$n/7] $t" -ForegroundColor Cyan }
function Ok($t) { Write-Host "  OK  $t" -ForegroundColor Green }
function Warn($t) { Write-Host "  !!  $t" -ForegroundColor Yellow }
function Fail($t) { Write-Host "  XX  $t" -ForegroundColor Red }
function Stop-Install { if ($PSCommandPath) { exit 1 } }  # irm | iex 로 돌면 창을 닫지 않게 return 만
function Refresh-Path {
  $m = [Environment]::GetEnvironmentVariable("Path", "Machine")
  $u = [Environment]::GetEnvironmentVariable("Path", "User")
  $env:Path = "$env:USERPROFILE\.local\bin;$env:USERPROFILE\.pes\runtime\node;$env:APPDATA\npm;C:\Program Files\nodejs;C:\Program Files\Git\cmd;$m;$u"
}
function Show($line) {
  if ($line -like "OK *" -or $line -like "INSTALLED *") { Ok $line } elseif ($line -like "FAIL *" -or $line -like "MISSING *") { Fail $line } elseif ($line -like "USER *") { Warn $line } elseif ($line -like "KEY *") { Warn $line } else { Write-Host "  $line" }
}

& {
Write-Host "PES 브리지(pes-bridge) 설치 ($Repo)" -ForegroundColor Cyan

Step 1 "worker 받기 → $Dest"
New-Item -ItemType Directory -Force -Path $Dest | Out-Null
if ($env:PES_BRIDGE_SRC) {
  Copy-Item -Path (Join-Path $env:PES_BRIDGE_SRC "*") -Destination $Dest -Recurse -Force
} else {
  $zip = Join-Path $env:TEMP "pes-bridge-$(Get-Random).zip"; $tmp = Join-Path $env:TEMP "pes-bridge-$(Get-Random)"
  try {
    Invoke-WebRequest "https://codeload.github.com/$Repo/zip/refs/heads/main" -OutFile $zip -UseBasicParsing
    Expand-Archive $zip -DestinationPath $tmp -Force
    $top = Get-ChildItem $tmp -Directory | Select-Object -First 1
    Copy-Item -Path (Join-Path $top.FullName "*") -Destination $Dest -Recurse -Force
  } catch { Fail "받지 못했어요: $($_.Exception.Message)"; Stop-Install; return }
  finally { Remove-Item $zip, $tmp -Recurse -Force -ErrorAction SilentlyContinue }
}
if (-not (Test-Path (Join-Path $Here "worker.mjs"))) { Fail "worker.mjs 가 없어요 ($Here)"; Stop-Install; return }
Ok $Dest

Step 2 "기본 도구 (Git · Node.js 26 이상)"
$nodeCmd = $null
& $PsExe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Dest "pes-scripts\bootstrap.ps1") | ForEach-Object {
  $l = "$_"
  if ($l -like "NODE *") { $nodeCmd = $l.Substring(5).Trim() }
  elseif ($l -like "FAIL node*") { Warn "winget 으로 Node.js 를 설치하지 못했어요 — nodejs.org 공식 파일로 받아요" }
  else { Show $l }
}
Refresh-Path
# winget 이 없거나 실패하면 nodejs.org 공식 zip 을 받아 %USERPROFILE%\.pes\runtime\node 에 둔다(관리자 권한 없이 · SHA-256 확인)
function Node-Ok { $v = & node --version 2>$null; return ($v -match '^v(\d+)\.' -and [int]$Matches[1] -ge 26) }
if (-not (Node-Ok)) {
  $ar = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" }
  $D = "https://nodejs.org/dist/latest-v26.x"
  try {
    $sums = (Invoke-WebRequest "$D/SHASUMS256.txt" -UseBasicParsing).Content -split "`n"
    $line = $sums | Where-Object { $_ -match "node-v[\d.]+-win-$ar\.zip$" } | Select-Object -First 1
    $sha, $file = ($line.Trim() -split '\s+')
    $zip = Join-Path $env:TEMP $file
    Invoke-WebRequest "$D/$file" -OutFile $zip -UseBasicParsing
    if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() -ne $sha.ToLower()) { throw "SHA-256 이 맞지 않아요" }
    $rt = Join-Path $env:USERPROFILE ".pes\runtime"; New-Item -ItemType Directory -Force -Path $rt | Out-Null
    Remove-Item (Join-Path $rt "node") -Recurse -Force -ErrorAction SilentlyContinue
    Expand-Archive $zip -DestinationPath $rt -Force
    Rename-Item (Join-Path $rt ($file -replace '\.zip$', '')) "node"
    Remove-Item $zip -ErrorAction SilentlyContinue
    $u = [Environment]::GetEnvironmentVariable("Path", "User"); $nd = Join-Path $rt "node"
    if ($u -notlike "*$nd*") { [Environment]::SetEnvironmentVariable("Path", "$nd;$u", "User") }  # 새 창 · npm 전역 도구가 찾게
    Refresh-Path
    if (Node-Ok) { Ok "INSTALLED node $(& node --version) ($nd)" }
  } catch { Fail "Node.js 를 받지 못했어요: $($_.Exception.Message)" }
}
$Node = (Get-Command node -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if (-not $Node -or -not (Node-Ok)) { Fail "Node.js 26 이상이 필요해요 — https://nodejs.org 에서 설치한 뒤 새 PowerShell 창에서 다시 실행해 주세요"; Stop-Install; return }
if (-not (Get-Command git -ErrorAction SilentlyContinue)) { Warn "Git 이 없어요 — Claude Code 가 Git Bash 를 써요. https://git-scm.com/download/win 에서 설치해 주세요" }

Step 3 "AI (Claude Code · Codex · OpenCode)"
$Claude = (Get-Command claude -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if (-not $Claude) {
  Warn "Claude Code 가 없어 공식 설치기로 받아요 (https://claude.ai/install.ps1)"
  try { Invoke-RestMethod https://claude.ai/install.ps1 | Invoke-Expression } catch { Fail "Claude Code 설치 실패: $($_.Exception.Message)" }
  Refresh-Path
  $Claude = (Get-Command claude -ErrorAction SilentlyContinue | Select-Object -First 1).Source
}
if (-not $Claude) { Fail "claude 를 찾지 못했어요 — 새 PowerShell 창에서 다시 실행해 주세요"; if (-not $Test) { Stop-Install; return } }
else {
  Ok "claude $(& $Claude --version 2>$null)"
  $st = & $Claude auth status 2>$null | Out-String
  if ($st -match '"loggedIn":\s*true') { Ok "로그인돼 있어요" }
  elseif ($Interactive) { Warn "로그인 창을 열어요 — 브라우저에서 로그인을 마쳐 주세요(worker 가 이 로그인으로 일해요)"; & $Claude auth login }
  else { Warn "로그인이 필요해요 — PowerShell 에서 claude 를 한 번 실행해 로그인해 주세요" }
}

# Codex · OpenCode — PES 「외부 AI」에서 쓸 AI로 고르면 이것으로 일한다. 없으면 npm 전역(%APPDATA%\npm)으로 받는다
foreach ($e in @(@{ n = "codex"; pkg = "@openai/codex"; login = "codex login" }, @{ n = "opencode"; pkg = "opencode-ai"; login = "opencode auth login" })) {
  $b = (Get-Command $e.n -ErrorAction SilentlyContinue | Select-Object -First 1).Source
  if ($b) { Ok "$($e.n) $(& $b --version 2>$null | Select-Object -First 1)"; continue }
  $npm = (Get-Command npm -ErrorAction SilentlyContinue | Select-Object -First 1).Source
  if (-not $npm) { Warn "$($e.n): npm 이 없어 받지 못했어요"; continue }
  Warn "$($e.n) 가 없어 받아요 (npm install -g $($e.pkg))"
  & $npm install -g $e.pkg 2>&1 | Out-Null
  Refresh-Path
  if (Get-Command $e.n -ErrorAction SilentlyContinue) { Ok "INSTALLED $($e.n) — 이 AI로 일하려면 PowerShell 에서 한 번 로그인해 주세요: $($e.login)" } else { Fail "$($e.n) 설치하지 못했어요 — npm install -g $($e.pkg)" }
}

Step 4 "조사 도구 (kosis · vworld · sgis · korean-law · kordoc)"
& $PsExe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Here "tools.ps1") install | ForEach-Object { Show "$_" }

Step 5 "브라우저 (ego lite)"
if ($Interactive) { & $PsExe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Here "tools.ps1") ego | ForEach-Object { Show "$_" } } else { Write-Host "  건너뜀" }

Step 6 "API 키"
if ($Interactive) {
  $missing = & $PsExe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Here "tools.ps1") check | Where-Object { "$_" -like "KEY *" }
  if (-not (Test-Path "$env:USERPROFILE\.pes\worker-keys-asked") -or $missing) { & $PsExe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Here "tools.ps1") keys } else { Ok "모두 등록돼 있어요" }
} else { Write-Host "  건너뜀 — 나중에: powershell -ExecutionPolicy Bypass -File `"$Here\tools.ps1`" keys" }

Step 7 "자동 실행 (작업 스케줄러 「PES Bridge」)"
# 예전 이름(pes-worker · 「PES Worker」)으로 설치돼 있으면 정리한다 — 연결 토큰은 새 폴더로 옮겨 다시 「허용」하지 않아도 되게
Unregister-ScheduledTask -TaskName "PES Worker" -Confirm:$false -ErrorAction SilentlyContinue
Remove-Item (Join-Path ([Environment]::GetFolderPath("Startup")) "PES Worker.lnk") -ErrorAction SilentlyContinue
$OldCfg = Join-Path $env:USERPROFILE ".config\pes-worker"; $NewCfg = Join-Path $env:USERPROFILE ".config\pes-bridge"
if ((Test-Path $OldCfg) -and -not (Test-Path (Join-Path $NewCfg "token.json")) -and -not (Test-Path (Join-Path $NewCfg "key"))) { New-Item -ItemType Directory -Force -Path $NewCfg | Out-Null; Copy-Item -Path (Join-Path $OldCfg "*") -Destination $NewCfg -Force -ErrorAction SilentlyContinue; Ok "연결 토큰을 새 폴더로 옮겼어요" }
if (-not $Test) { Remove-Item (Join-Path $env:USERPROFILE ".pes\worker") -Recurse -Force -ErrorAction SilentlyContinue }
# 창 없이 node worker.mjs 를 돌리는 VBScript — 끝나면 5초 뒤 다시 켠다. 로그는 %LOCALAPPDATA%\pes-bridge\worker.log
$LogDir = Join-Path $env:LOCALAPPDATA "pes-bridge"; New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$Vbs = Join-Path $Here "run.vbs"
$cmd = "cmd /c chcp 65001 >nul & `"`"$Node`"`" `"`"$Here\worker.mjs`"`" >> `"`"$LogDir\worker.log`"`" 2>&1"
$vb = @"
' PES 브리지(pes-bridge) — 창 없이 실행하고, 끝나면 5초 뒤 다시 켠다 (install.ps1 이 만든다)
Set sh = CreateObject("WScript.Shell")
Do
  sh.Run "$cmd", 0, True
  WScript.Sleep 5000
Loop
"@
[IO.File]::WriteAllText($Vbs, $vb, (New-Object Text.UnicodeEncoding($false, $true)))
if ($Test) { Write-Host "  건너뜀(PES_BRIDGE_TEST) — $Vbs 만 만들었어요"; Write-Host "`n완료(시험)"; return }
$Task = "PES Bridge"
Get-CimInstance Win32_Process -Filter "Name='wscript.exe'" -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -like "*pes*run.vbs*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -like "*worker.mjs*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
$registered = $false
try {
  $action = New-ScheduledTaskAction -Execute "wscript.exe" -Argument "`"$Vbs`""
  $trigger = New-ScheduledTaskTrigger -AtLogOn -User "$env:USERDOMAIN\$env:USERNAME"
  $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
  $principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Limited
  Register-ScheduledTask -TaskName $Task -Action $action -Trigger $trigger -Settings $settings -Principal $principal -Description "PES 브리지(pes-bridge) — https://github.com/$Repo" -Force -ErrorAction Stop | Out-Null
  Start-ScheduledTask -TaskName $Task -ErrorAction Stop
  $registered = $true; Ok "작업 스케줄러에 등록하고 켰어요: $Task"
} catch {
  # 작업 스케줄러를 쓸 수 없으면(회사 정책 등) 시작프로그램 폴더에 넣는다
  $startup = [Environment]::GetFolderPath("Startup")
  $lnk = Join-Path $startup "PES Bridge.lnk"
  $ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut($lnk); $s.TargetPath = "wscript.exe"; $s.Arguments = "`"$Vbs`""; $s.WorkingDirectory = $Here; $s.Save()
  Start-Process wscript.exe -ArgumentList "`"$Vbs`""
  Warn "작업 스케줄러에 등록하지 못해 시작프로그램에 넣고 켰어요: $lnk"
}
if (-not ((Test-Path "$env:USERPROFILE\.config\pes-bridge\key") -or (Test-Path "$env:USERPROFILE\.config\pes-bridge\token.json") -or (Get-ChildItem "$env:USERPROFILE\.config\pes-bridge\accounts" -ErrorAction SilentlyContinue))) { Warn "곧 브라우저에 PES 「허용」 화면이 열려요 — 허용을 누르면 PES 브리지가 스스로 연결해요." }
Write-Host "`n완료. 로그: Get-Content `"$LogDir\worker.log`" -Wait -Tail 20   (「PES 알림에 붙었어요」가 보이면 연결된 거예요)" -ForegroundColor Cyan
Write-Host "PES 계정을 더 붙이려면: & `"$Node`" `"$Here\worker.mjs`" --add   (보기 --list · 떼기 --remove <번호>)"
}
