# PES 외부 AI채팅(worker) 끄기 · 지우기 — Windows (PowerShell)
#   irm https://raw.githubusercontent.com/clazic/pes-worker/main/uninstall.ps1 | iex
# 작업 스케줄러 「PES Worker」 · 시작프로그램 바로가기 · worker 파일(%USERPROFILE%\.pes\worker)을 지운다.
# 연결 토큰(%USERPROFILE%\.config\pes-worker)은 $env:PES_WORKER_PURGE='1' 일 때만 지운다. 조사 도구 · API 키 · Node.js · Claude Code 는 그대로 둔다.
try { [Console]::OutputEncoding = [Text.Encoding]::UTF8 } catch { }
Unregister-ScheduledTask -TaskName "PES Worker" -Confirm:$false -ErrorAction SilentlyContinue
Remove-Item (Join-Path ([Environment]::GetFolderPath("Startup")) "PES Worker.lnk") -ErrorAction SilentlyContinue
Get-CimInstance Win32_Process -Filter "Name='wscript.exe'" -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -like "*pes*run.vbs*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -like "*worker.mjs*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Remove-Item (Join-Path $env:USERPROFILE ".pes\worker") -Recurse -Force -ErrorAction SilentlyContinue
if ($env:PES_WORKER_PURGE -eq "1") { Remove-Item (Join-Path $env:USERPROFILE ".config\pes-worker") -Recurse -Force -ErrorAction SilentlyContinue; Write-Host "연결 토큰도 지웠어요" }
Write-Host "지웠어요. 다시 설치: irm https://raw.githubusercontent.com/clazic/pes-worker/main/install.ps1 | iex"
