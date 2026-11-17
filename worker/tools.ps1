# PES 외부 AI worker 의 조사 도구 (Windows) — tools.sh 와 같은 일을 PowerShell 로.
#   kosis       clazic/kosis        scripts/install.ps1 (릴리스 실행 파일 + 스킬)
#   vworld      clazic/vworld       scripts/install.ps1 -Scope user
#   sgis        clazic/sgis         scripts/install.ps1
#   korean-law  clazic/korean-law-mcp  소스를 받아 빌드(npm ci · tsc) → npm 전역 설치
#   kordoc      chrisryugj/kordoc   npm 패키지 kordoc
#   ego-browser ego lite 앱 — Windows 는 설치 스크립트가 없어 내려받기 페이지(https://lite.ego.app/)를 연다
# 사용(PowerShell):
#   powershell -ExecutionPolicy Bypass -File tools.ps1 install   빠진 도구만 설치(묻지 않음 — worker 가 켜질 때 부른다)
#   powershell -ExecutionPolicy Bypass -File tools.ps1 keys      API 키를 단계별로 물어 각 도구에 등록하고 확인한다
#   powershell -ExecutionPolicy Bypass -File tools.ps1 ego       ego lite 가 없으면 내려받기 페이지를 연다
#   powershell -ExecutionPolicy Bypass -File tools.ps1 check     줄마다 "<OK|MISSING|KEY> <이름> <설명>"
# 키 값은 화면에 보이지 않게 받고, 출력에는 앞뒤 4자리만 보인다.
param([string]$Cmd = "check")
$ErrorActionPreference = "Continue"
try { [Console]::OutputEncoding = [Text.Encoding]::UTF8 } catch { }
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$L = $env:LOCALAPPDATA
$Settings = Join-Path $env:USERPROFILE ".claude\settings.json"
$Raw = "https://raw.githubusercontent.com/clazic"

function Refresh-Path {
  $m = [Environment]::GetEnvironmentVariable("Path", "Machine")
  $u = [Environment]::GetEnvironmentVariable("Path", "User")
  $extra = @("$env:USERPROFILE\.local\bin", "$env:USERPROFILE\.pes\runtime\node", "$env:APPDATA\npm", "$L\Programs\kosis", "$L\Programs\sgis", "$L\vworld", "C:\Program Files\nodejs", "C:\Program Files\Git\cmd")
  $env:Path = (@($extra) + @("$m", "$u")) -join ";"
}
Refresh-Path
function Have($n) { [bool](Get-Command $n -ErrorAction SilentlyContinue) }
function Where-Is($n) { $c = Get-Command $n -ErrorAction SilentlyContinue | Select-Object -First 1; if ($c) { $c.Source } else { "" } }
function Mask($k) { if ($k.Length -gt 8) { $k.Substring(0, 4) + "…" + $k.Substring($k.Length - 4) } else { "****" } }
$PsExe = (Get-Process -Id $PID).Path
$ToolLog = Join-Path $env:TEMP "pes-tools-install.log" # 설치기 출력(실패하면 마지막 줄을 함께 보인다)

# 각 도구의 설치 스크립트를 파일로 받아 따로 돌린다(그 스크립트가 exit 해도 이 스크립트는 계속)
function Run-Remote($urls, $argList) {
  $f = Join-Path $env:TEMP "pes-tool-$(Get-Random).ps1"
  $ok = $false
  # Windows PowerShell 5 는 BOM 없는 UTF-8 스크립트를 ANSI 로 읽어 한글이 깨진다 — 글로 받아 BOM 을 붙여 쓴다
  foreach ($u in $urls) { try { $wc = New-Object Net.WebClient; $wc.Encoding = [Text.Encoding]::UTF8; [IO.File]::WriteAllText($f, $wc.DownloadString($u), (New-Object Text.UTF8Encoding($true))); $ok = $true; break } catch { } }
  if (-not $ok) { "받지 못함: $($urls -join ' · ')" | Out-File $ToolLog -Append -Encoding utf8; return $false }
  "== $($urls[0])" | Out-File $ToolLog -Append -Encoding utf8
  & $PsExe -NoProfile -ExecutionPolicy Bypass -File $f @argList *>&1 | Out-File $ToolLog -Append -Encoding utf8
  Remove-Item $f -ErrorAction SilentlyContinue
  return $true
}
function Install-One($t) {
  switch ($t) {
    "kosis" { $env:KOSIS_TARGET = "claude"; $env:KOSIS_CLAUDE_SCOPE = "global"; Run-Remote @("$Raw/kosis/master/scripts/install.ps1", "$Raw/kosis/main/scripts/install.ps1") @() | Out-Null }
    "vworld" { Run-Remote @("$Raw/vworld/main/scripts/install.ps1") @("-Scope", "user") | Out-Null }
    "sgis" { $env:SGIS_TARGET = "claude"; $env:SGIS_CLAUDE_SCOPE = "global"; Run-Remote @("$Raw/sgis/master/scripts/install.ps1", "$Raw/sgis/main/scripts/install.ps1") @() | Out-Null }
    "korean-law" {
      if (-not (Have npm)) { return }
      $d = Join-Path $env:TEMP "pes-law-$(Get-Random)"; New-Item -ItemType Directory -Force -Path $d | Out-Null
      try {
        Invoke-WebRequest "https://codeload.github.com/clazic/korean-law-mcp/zip/refs/heads/main" -OutFile "$d\src.zip" -UseBasicParsing
        Expand-Archive "$d\src.zip" -DestinationPath $d -Force
        $s = Get-ChildItem $d -Directory | Select-Object -First 1
        Push-Location $s.FullName
        & npm ci --ignore-scripts --no-audit --no-fund --loglevel=error *> $null
        & npm run build --silent *> $null
        & npm pack --silent *> $null
        $tgz = Get-ChildItem -Filter "korean-law-mcp-*.tgz" | Select-Object -First 1
        if ($tgz) { & npm install -g $tgz.FullName --ignore-scripts --no-audit --no-fund --loglevel=error *> $null }
        Pop-Location
      } catch { } finally { Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue }
    }
    "kordoc" { if (Have npm) { & npm install -g kordoc@latest --no-audit --no-fund --loglevel=error *> $null } }
  }
  Refresh-Path
}
$Tools = @("kosis", "vworld", "sgis", "korean-law", "kordoc")

# ── 키 상태 ──
function Kosis-N { if (-not (Have kosis)) { return 0 }; @(& kosis config key-list 2>$null | Where-Object { "$_" -match '^\s*\[\d+\]' }).Count }
function Vworld-N { if (-not (Have vworld)) { return 0 }; ([regex]::Matches((& vworld config list-keys 2>$null | Out-String), '"masked"')).Count }
function Sgis-Ok { if (-not (Have sgis)) { return $false }; [bool](& sgis config get 2>$null | Where-Object { "$_" -match '^consumer_key:' -and "$_" -notmatch '미설정' }) }
function Law-Oc { try { (Get-Content $Settings -Raw -Encoding UTF8 | ConvertFrom-Json).env.LAW_OC } catch { "" } }

function Cmd-Install {
  foreach ($t in $Tools) {
    if (Have $t) { Write-Output "OK $t $(Where-Is $t)"; continue }
    Write-Output "... $t 설치 중"
    Install-One $t
    if (Have $t) { Write-Output "INSTALLED $t" } else {
      $tail = if (Test-Path $ToolLog) { ((Get-Content $ToolLog -Tail 3 -Encoding UTF8) -join ' / ') } else { "" }
      Write-Output "FAIL $t 설치하지 못했어요 — $tail (전체: $ToolLog)"
    }
  }
}
function Cmd-Check {
  foreach ($t in $Tools) { if (Have $t) { Write-Output "OK $t $(Where-Is $t)" } else { Write-Output "MISSING $t" } }
  if (Have ego-browser) { Write-Output "OK ego-browser $(Where-Is ego-browser)" } else { Write-Output "MISSING ego-browser" }
  $n = Kosis-N; if ($n -gt 0) { Write-Output "OK key-kosis ${n}개" } else { Write-Output "KEY key-kosis KOSIS API 키" }
  $n = Vworld-N; if ($n -gt 0) { Write-Output "OK key-vworld ${n}개" } else { Write-Output "KEY key-vworld VWorld API 키" }
  if (Sgis-Ok) { Write-Output "OK key-sgis 등록됨" } else { Write-Output "KEY key-sgis SGIS 서비스 ID · 보안 Key" }
  $oc = Law-Oc; if ($oc) { Write-Output "OK key-law $(Mask $oc)" } else { Write-Output "KEY key-law 법령 LAW_OC" }
}
function Cmd-Ego {
  if (Have ego-browser) { Write-Output "OK ego-browser $(Where-Is ego-browser)"; return }
  # Windows 는 설치 스크립트가 없다 — 설치 파일을 받아 실행해야 한다(SmartScreen 이 막으면 「추가 정보」 → 「실행」)
  Write-Output "USER ego-browser https://lite.ego.app/ 에서 Windows 설치 파일을 받아 실행하고, 앱의 첫 실행 안내를 끝낸 뒤 새 PowerShell 을 열어 주세요. 외부 AI 전용으로 로그인 없는 「PES」 프로필을 만들어 두면 그것만 써요"
  try { Start-Process "https://lite.ego.app/" } catch { }
}

# ── 키를 단계별로 묻는다 ──
function Ask-Secret($p) {
  $s = Read-Host -Prompt $p -AsSecureString
  $b = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($s)
  try { ([Runtime.InteropServices.Marshal]::PtrToStringBSTR($b)) -replace '\s', '' } finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($b) }
}
function Ask-Yn($p) { $v = Read-Host -Prompt $p; return -not ($v -match '^[nN]') }
function Step($n, $title, $url, $lines) {
  Write-Host ""; Write-Host "[$n/4] $title" -ForegroundColor Cyan
  $i = 1; foreach ($s in $lines) { Write-Host "   $i. $s"; $i++ }
  Write-Host "   발급 페이지: $url"
  if (Ask-Yn "   발급 페이지를 브라우저로 열까요? [Y/n]") { try { Start-Process $url } catch { } }
}
function Reg-Kosis($k) {
  if (-not (Have kosis)) { Write-Host "   kosis 가 설치돼 있지 않아요 — 건너뛰어요"; return $true }
  $env:KOSIS_API_KEY = $k; & kosis s 인구 -n 1 *> $null; $ok = $LASTEXITCODE -eq 0; Remove-Item Env:\KOSIS_API_KEY -ErrorAction SilentlyContinue
  if ($ok) { & kosis config add-key $k *> $null; $ok = $LASTEXITCODE -eq 0 }
  if ($ok) { Write-Host "   ✓ 등록 · 확인했어요 ($(Mask $k))" -ForegroundColor Green } else { Write-Host "   ✗ 키 확인에 실패했어요 — 키를 다시 확인해 주세요" -ForegroundColor Red }
  return $ok
}
function Reg-Vworld($k) {
  if (-not (Have vworld)) { Write-Host "   vworld 가 설치돼 있지 않아요 — 건너뛰어요"; return $true }
  & vworld config add-key $k --alias pes *> $null
  $v = ""
  try {
    $t = & vworld config test-keys 2>$null | Out-String
    $j = $t.Substring($t.IndexOf("{")) | ConvertFrom-Json
    foreach ($r in $j.results) { $s = "$($r.key)"; if ($s.Length -ge 8 -and $s.Substring(0, 4) -eq $k.Substring(0, 4) -and $s.Substring($s.Length - 4) -eq $k.Substring($k.Length - 4)) { $v = "$($r.verdict)" } }
  } catch { }
  if ($v -eq "유효") { Write-Host "   ✓ 등록 · 확인했어요 ($(Mask $k))" -ForegroundColor Green; return $true }
  & vworld config remove-key $k *> $null
  Write-Host "   ✗ 키 확인에 실패했어요($v) — 키 · 등록 도메인을 다시 확인해 주세요" -ForegroundColor Red; return $false
}
function Reg-Sgis($id, $sec) {
  if (-not (Have sgis)) { Write-Host "   sgis 가 설치돼 있지 않아요 — 건너뛰어요"; return $true }
  $cfg = Join-Path $env:USERPROFILE ".sgis\config.yaml"; $bak = "$cfg.bak-pes"
  if (Test-Path $cfg) { Copy-Item $cfg $bak -Force }
  & sgis config set-credential $id $sec *> $null
  & sgis code stage *> $null
  if ($LASTEXITCODE -eq 0) { Write-Host "   ✓ 등록 · 확인했어요 ($(Mask $id))" -ForegroundColor Green; Remove-Item $bak -ErrorAction SilentlyContinue; return $true }
  if (Test-Path $bak) { Move-Item $bak $cfg -Force }
  Write-Host "   ✗ 키 확인에 실패했어요 — 서비스 ID · 보안 Key 를 다시 확인해 주세요" -ForegroundColor Red; return $false
}
function Reg-Law($k) {
  if (-not (Have korean-law)) { Write-Host "   korean-law 가 설치돼 있지 않아요 — 건너뛰어요"; return $true }
  $env:LAW_OC = $k; $o = & korean-law search_law --query 민법 2>&1 | Out-String; Remove-Item Env:\LAW_OC -ErrorAction SilentlyContinue
  if ($o -notmatch "검색 결과") { Write-Host "   ✗ 키 확인에 실패했어요 — LAW_OC 를 다시 확인해 주세요" -ForegroundColor Red; return $false }
  New-Item -ItemType Directory -Force -Path (Split-Path $Settings) | Out-Null
  $j = if (Test-Path $Settings) { Copy-Item $Settings "$Settings.bak-pes" -Force; Get-Content $Settings -Raw -Encoding UTF8 | ConvertFrom-Json } else { [pscustomobject]@{} }
  if (-not $j.env) { $j | Add-Member -NotePropertyName env -NotePropertyValue ([pscustomobject]@{}) -Force }
  $j.env | Add-Member -NotePropertyName LAW_OC -NotePropertyValue $k -Force
  [IO.File]::WriteAllText($Settings, ($j | ConvertTo-Json -Depth 20), (New-Object Text.UTF8Encoding($false)))
  Write-Host "   ✓ 등록 · 확인했어요 ($(Mask $k)) — $Settings 의 env.LAW_OC" -ForegroundColor Green; return $true
}
function Cmd-Keys {
  Write-Host "PES 외부 AI가 쓸 API 키를 하나씩 넣어요. 키는 화면에 보이지 않고, 이 컴퓨터의 각 도구 설정에만 저장돼요."
  Write-Host "없는 키는 그냥 Enter 로 건너뛰면 돼요."
  $n = Kosis-N
  Step 1 "KOSIS (통계청 국가통계포털) API 키" "https://kosis.kr/openapi/" @("kosis.kr/openapi 접속 → 로그인(회원가입)", "상단 '활용신청' → 활용 목적 입력 후 신청", "마이페이지 → 인증키 확인 → 복사")
  if ($n -gt 0) { Write-Host "   이미 ${n}개 등록돼 있어요 — 더 넣지 않으려면 Enter" }
  while ($true) { $k = Ask-Secret "   KOSIS 키"; if (-not $k) { break }; if (Reg-Kosis $k) { break } }
  $n = Vworld-N
  Step 2 "VWorld (국가공간정보) API 키" "https://www.vworld.kr/dev/v4dv_apikey_s001.do" @("vworld.kr 로그인(회원가입)", "오픈API → 인증키 발급", "서비스 URL 없이(무도메인) · 활용 API 전체 선택 → 발급된 키 복사")
  if ($n -gt 0) { Write-Host "   이미 ${n}개 등록돼 있어요 — 더 넣지 않으려면 Enter" }
  while ($true) { $k = Ask-Secret "   VWorld 키"; if (-not $k) { break }; if (Reg-Vworld $k) { break } }
  Step 3 "SGIS (통계지리정보서비스) 서비스 ID · 보안 Key" "https://sgis.mods.go.kr/developer/html/newOpenApi/guide/guide/getApiKey.html" @("SGIS 개발지원센터 로그인(회원가입)", "인증키 신청 → 서비스 등록", "발급된 서비스 ID(consumer key)와 보안 Key(consumer secret) 복사")
  if (Sgis-Ok) { Write-Host "   이미 등록돼 있어요 — 바꾸지 않으려면 Enter" }
  while ($true) { $id = Ask-Secret "   SGIS 서비스 ID"; if (-not $id) { break }; $sec = Ask-Secret "   SGIS 보안 Key"; if (-not $sec) { break }; if (Reg-Sgis $id $sec) { break } }
  Step 4 "법령 API (국가법령정보 공동활용) — LAW_OC" "https://open.law.go.kr/LSO/openApi/cuAskList.do" @("open.law.go.kr 로그인(회원가입)", "OPEN API → 활용 신청 (법령·판례 등 체크)", "승인 후 이메일 ID 앞부분(@ 앞)이 LAW_OC 값")
  if (Law-Oc) { Write-Host "   이미 등록돼 있어요 — 바꾸지 않으려면 Enter" }
  while ($true) { $k = Ask-Secret "   LAW_OC"; if (-not $k) { break }; if (Reg-Law $k) { break } }
  Write-Host ""; Write-Host "등록 상태:"
  Cmd-Check | Where-Object { $_ -match '^(OK|KEY) key-' } | ForEach-Object { Write-Host ("  " + ($_ -replace '^OK ', '✓ ' -replace '^KEY ', '· 없음 ')) }
  New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.pes" | Out-Null
  Set-Content -Path "$env:USERPROFILE\.pes\worker-keys-asked" -Value (Get-Date -Format o)
}

switch ($Cmd) {
  "install" { Cmd-Install }
  "keys" { Cmd-Keys }
  "ego" { Cmd-Ego }
  "check" { Cmd-Check }
  default { Write-Output "사용: tools.ps1 install|keys|ego|check"; exit 2 }
}
