# PES 외부 AI채팅 (pes-worker)

[PES(정책 연구보고서 작성 서비스)](https://pes.seliq.kr)의 **외부 AI채팅**을 내 컴퓨터에서 돌리는 프로그램입니다.

PES 프로젝트의 작업 AI를 「외부 AI」로 고르면, 글쓰기·조사·채팅 답을 PES 서버가 아니라 **내 컴퓨터의 Claude Code**가 합니다. 이 프로그램은 컴퓨터에 조용히 떠 있다가 PES에서 일이 생기면 알림을 받아 처리합니다. Claude 앱이나 터미널 창을 열어 둘 필요가 없습니다.

- 조사는 이 컴퓨터의 도구로 합니다: 통계 **kosis**, 공간정보 **vworld**, 통계지리 **sgis**, 법령 **korean-law**, 문서 변환 **kordoc**(HWP·HWPX·PDF·오피스 → Markdown), 브라우저 **ego lite**(자바스크립트로 그리는 원문·첨부 받기)
- 일하는 동안 PES 화면에 「외부 AI가 일하는 중이에요 · 지금 항목을 읽는 중」처럼 실제 상태가 보입니다(멈추면 30초 안에 「멈춤」으로 바뀜)

## 원클릭 설치

### macOS

터미널(응용 프로그램 → 유틸리티 → 터미널)에 붙여 넣고 Enter:

```sh
curl -fsSL https://raw.githubusercontent.com/clazic/pes-worker/main/install.sh | zsh
```

### Windows

PowerShell(시작 → 「PowerShell」 검색)에 붙여 넣고 Enter:

```powershell
irm https://raw.githubusercontent.com/clazic/pes-worker/main/install.ps1 | iex
```

관리자 권한은 필요 없습니다. 다시 실행해도 됩니다 — 없는 것만 설치하고 worker 는 최신판으로 바꿉니다(**업데이트도 같은 명령**).

## 설치가 하는 일

| 단계 | macOS | Windows |
| --- | --- | --- |
| 1. worker 받기 | `~/.pes/worker` | `%USERPROFILE%\.pes\worker` |
| 2. 실행 환경 | Node.js 26+ · Python 3.12 · Claude Code 가 없으면 `~/.pes/runtime` 에 받음(sudo · Homebrew 없이, Node 는 SHA-256 확인) | Git(Git Bash) · Node.js 26+ 가 없으면 winget 으로 설치(`pes-scripts/bootstrap.ps1`) |
| 3. Claude Code | 없으면 공식 설치기(`claude.ai/install.sh`), 로그인 안 돼 있으면 로그인 창 | 없으면 공식 설치기(`claude.ai/install.ps1`), 로그인 안 돼 있으면 로그인 창 |
| 4. 조사 도구 | kosis · vworld · sgis · korean-law · kordoc (`worker/tools.sh`) | 같은 도구 (`worker/tools.ps1`) |
| 5. 브라우저 ego lite | 없으면 공식 DMG 를 받아 설치하고 앱을 엶 | 설치 스크립트가 없어 내려받기 페이지(lite.ego.app)를 엶 |
| 6. API 키 | KOSIS → VWorld → SGIS → 법령 LAW_OC 를 하나씩 물음 | 같음 |
| 7. 자동 실행 | launchd `kr.seliq.pes-worker` (로그인하면 켜지고, 꺼지면 다시 켬) | 작업 스케줄러 「PES Worker」 (로그인하면 창 없이 켜지고, 꺼지면 5초 뒤 다시 켬) |

조사 도구는 모두 GitHub 저장소에서 받습니다 — kosis·vworld·sgis·korean-law 는 [clazic](https://github.com/clazic), kordoc 은 원본 [chrisryugj/kordoc](https://github.com/chrisryugj/kordoc)(npm 패키지).

## 처음 한 번 직접 할 일

1. **Claude Code 로그인** — 설치 중 로그인 창이 열리면 로그인합니다. worker 는 이 로그인(내 Claude 구독)으로 일하고, 사용량도 이 계정에서 나갑니다.
2. **PES 「허용」** — worker 가 처음 켜지면 브라우저에 PES 「허용」 화면이 열립니다. PES 에 로그인한 계정으로 허용을 누르면 끝입니다(토큰은 `~/.config/pes-worker/token.json` 에 두고 스스로 갱신).
3. **API 키** — 설치 중 단계별로 묻습니다. 발급 페이지를 브라우저로 열어 주고, 키는 화면에 보이지 않게 받아 실제 호출로 확인한 뒤 각 도구 설정에만 저장합니다. 없는 키는 Enter 로 건너뛰고 나중에 넣어도 됩니다.
4. **ego lite** — 앱의 첫 실행 안내를 끝내고, 로그인하지 않은 새 프로필을 **「PES」** 라는 이름으로 만들어 두면 worker 는 그 프로필로만 원문을 엽니다(로그인된 내 계정이 쓰이지 않게).

## 사용법

1. [pes.seliq.kr](https://pes.seliq.kr) 에서 새 프로젝트를 만들 때 「외부 AI 사용자 모드」 또는 「외부 AI 자동 모드」를 고릅니다. (이미 있는 프로젝트는 맨 위 「작업 AI」를 「외부 AI」로)
2. 그다음은 PES 화면에서 평소처럼 하면 됩니다 — 라운드 글쓰기, 점검 질문 답(자동 모드), 채팅 답을 이 컴퓨터의 worker 가 합니다.
3. 컴퓨터가 **켜져 있고 잠자기 상태가 아니며 사용자로 로그인돼 있어야** 일합니다. 오래 맡길 때는 잠자기를 꺼 두세요(macOS: 시스템 설정 → 배터리 → 전원 어댑터에서 「디스플레이가 꺼졌을 때 자동으로 잠자지 않기」).

같은 PES 계정으로 worker 를 두 대 켜 두면 둘 다 같은 일을 받습니다 — **한 대만** 켜 두세요.

## 관리 명령

| 하는 일 | macOS | Windows (PowerShell) |
| --- | --- | --- |
| 로그 보기 | `tail -f ~/Library/Logs/pes-worker.log` | `Get-Content "$env:LOCALAPPDATA\pes-worker\worker.log" -Wait -Tail 20` |
| 다시 켜기 | `launchctl kickstart -k gui/$(id -u)/kr.seliq.pes-worker` | `Stop-ScheduledTask "PES Worker"; Start-ScheduledTask "PES Worker"` |
| 끄기 | `launchctl bootout gui/$(id -u)/kr.seliq.pes-worker` | `Stop-ScheduledTask "PES Worker"` |
| API 키 다시 넣기 | `sh ~/.pes/worker/worker/tools.sh keys` | `powershell -ExecutionPolicy Bypass -File "$env:USERPROFILE\.pes\worker\worker\tools.ps1" keys` |
| 준비 상태 점검(연결 없이) | `node ~/.pes/worker/worker/worker.mjs --check` | `node "$env:USERPROFILE\.pes\worker\worker\worker.mjs" --check` |
| 도구 · 키 상태 | `sh ~/.pes/worker/worker/tools.sh check` | `powershell -ExecutionPolicy Bypass -File "$env:USERPROFILE\.pes\worker\worker\tools.ps1" check` |
| 업데이트 | 설치 명령을 다시 실행 | 설치 명령을 다시 실행 |
| 지우기 | `curl -fsSL https://raw.githubusercontent.com/clazic/pes-worker/main/uninstall.sh \| sh` | `irm https://raw.githubusercontent.com/clazic/pes-worker/main/uninstall.ps1 \| iex` |

로그에 「PES 알림에 붙었어요」가 보이면 연결된 것입니다. 「키가 없어요: …」가 보이면 그 키를 넣어 주세요. 지우기는 worker 와 자동 실행만 지웁니다(조사 도구 · API 키 · Node.js · Claude Code 는 그대로, 연결 토큰까지 지우려면 `PES_WORKER_PURGE=1`).

## 모델 바꾸기

- 라운드 글쓰기 · 점검: Claude Code 의 기본 모델(`claude` 에서 `/model` 로 고르면 `~/.claude/settings.json` 의 `model` 에 저장)을 따릅니다.
- 채팅 답: 빠른 답을 위해 `sonnet` 을 씁니다(`worker/worker.mjs` 의 `runChats`).

## 안전

- 키 · 토큰은 이 컴퓨터에만 둡니다: PES 연결 토큰 `~/.config/pes-worker/`(본인만 읽기), 조사 도구 키는 각 도구 설정, LAW_OC 는 `~/.claude/settings.json` 의 `env`.
- worker 의 Claude Code 는 허용한 도구만 씁니다: PES MCP · 웹 검색/읽기 · 스킬 · 파일 읽기, 그리고 `kosis` · `vworld` · `sgis` · `korean-law` · `kordoc` · `ego-browser` 로 시작하는 명령 하나씩(파이프 · `&&` · `;` 없이).
- **ego-browser 주의**: `ego-browser nodejs` 는 자바스크립트를 실행하므로 이 허용은 명령 제한이 아니라 규칙(공개 페이지 읽기만, 로그인 · 글쓰기 · 양식 제출 금지, 첨부는 `~/.pes/docs` 에만 저장)에 기댑니다. 반드시 로그인 없는 「PES」 프로필을 만들어 두세요.
- 받은 원문 파일과 변환한 Markdown 은 `~/.pes/docs` 에 둡니다.

## 저장소 구성

```
install.sh · install.ps1        원클릭 설치(macOS · Windows)
uninstall.sh · uninstall.ps1    끄기 · 지우기
worker/worker.mjs               worker 본체(Node, 의존성 없음)
worker/bootstrap.sh             macOS 실행 환경 준비(Node · Python · Claude Code)
worker/tools.sh · tools.ps1     조사 도구 설치 · 키 등록 · 상태
pes-scripts/                    clazic/pes 의 scripts/ 를 그대로 옮긴 것(Windows 기본 도구 설치에 bootstrap.ps1 을 쓴다)
.github/workflows/              sync-pes-scripts(clazic/pes scripts 자동 반영) · test-install(macOS · Windows 설치 시험)
```

### clazic/pes 자동 반영

[clazic/pes 의 scripts/](https://github.com/clazic/pes/tree/main/scripts) 가 바뀌면 GitHub Actions(`sync-pes-scripts`)가 **30분마다** 확인해 `pes-scripts/` 에 그대로 옮기고 커밋합니다. Actions 화면에서 「Run workflow」로 바로 돌릴 수도 있고, clazic/pes 에서 `repository_dispatch`(`pes-scripts-changed`)를 보내면 즉시 반영됩니다. 설치는 늘 이 저장소의 최신 `main` 을 받으므로, 설치 명령을 다시 실행하면 바뀐 것이 컴퓨터에도 들어갑니다.

## 문제 해결

- **「키를 받지 못했어요」가 반복** — 브라우저의 PES 「허용」을 누르지 않았거나 10분이 지났습니다. 다시 켜면 허용 화면이 다시 열립니다.
- **「오류로 멈췄어요 — usage limit」** — Claude 사용 한도입니다. 초기화된 뒤 다시 켜면 밀린 일을 이어서 합니다.
- **Windows 에서 kosis 등이 안 잡힘** — 새 PowerShell 창에서 다시 실행하세요(PATH 는 새 창부터 반영). `tools.ps1 check` 로 확인합니다.
- **ego-browser 를 안 씀** — ego lite 첫 실행 안내를 끝냈는지, 「PES」 프로필이 있는지 확인하고 worker 를 다시 켜세요(켤 때 프로필을 찾습니다).

라이선스: MIT
