# PES 브리지 (pes-bridge)

[PES(정책 연구보고서 작성 서비스)](https://pes.seliq.kr)와 **내 컴퓨터의 Claude Code**를 잇는 프로그램입니다(PES 화면에서는 「PES → 외부AI · Claude Code」로 보입니다).

PES 프로젝트의 작업 AI를 「외부 AI」로 고르면, 글쓰기·조사·채팅 답을 PES 서버가 아니라 **내 컴퓨터의 Claude Code**가 합니다. 이 프로그램은 컴퓨터에 조용히 떠 있다가 PES에서 일이 생기면 알림을 받아 처리합니다. Claude 앱이나 터미널 창을 열어 둘 필요가 없습니다.

- 조사는 이 컴퓨터의 도구로 합니다: 통계 **kosis**, 공간정보 **vworld**, 통계지리 **sgis**, 법령 **korean-law**, 문서 변환 **kordoc**(HWP·HWPX·PDF·오피스 → Markdown), 브라우저 **ego lite**(자바스크립트로 그리는 원문·첨부 받기)
- 일하는 동안 PES 화면에 「외부 AI가 일하는 중이에요 · 지금 항목을 읽는 중」처럼 실제 상태가 보입니다. AI 출력이 3분 넘게 끊기면 신호를 멈춰 화면이 「멈춤」으로 바뀌고, 10분 동안 응답이 없거나 한 번 실행이 상한(라운드 2시간 · 채팅 · 다듬기 · 내 자료 다듬기 20분)을 넘으면 그 실행을 끝내고 오류로 알립니다(다음 일이 막히지 않게)

## 원클릭 설치

### macOS

터미널(응용 프로그램 → 유틸리티 → 터미널)에 붙여 넣고 Enter:

```sh
curl -fsSL https://raw.githubusercontent.com/clazic/pes-bridge/main/install.sh | zsh
```

### Windows

PowerShell(시작 → 「PowerShell」 검색)에 붙여 넣고 Enter:

```powershell
irm https://raw.githubusercontent.com/clazic/pes-bridge/main/install.ps1 | iex
```

관리자 권한은 필요 없습니다. 다시 실행해도 됩니다 — 없는 것만 설치하고 worker 는 최신판으로 바꿉니다(**업데이트도 같은 명령**).

## 설치가 하는 일

| 단계 | macOS | Windows |
| --- | --- | --- |
| 1. worker 받기 | `~/.pes/bridge` | `%USERPROFILE%\.pes\bridge` |
| 2. 실행 환경 | Node.js 26+ · Python 3.12 · Claude Code 가 없으면 `~/.pes/runtime` 에 받음(sudo · Homebrew 없이, Node 는 SHA-256 확인) | Git(Git Bash) · Node.js 26+ 가 없으면 winget 으로 설치(`pes-scripts/bootstrap.ps1`) |
| 3. Claude Code | 없으면 공식 설치기(`claude.ai/install.sh`), 로그인 안 돼 있으면 로그인 창 | 없으면 공식 설치기(`claude.ai/install.ps1`), 로그인 안 돼 있으면 로그인 창 |
| 4. 조사 도구 | kosis · vworld · sgis · korean-law · kordoc (`worker/tools.sh`) | 같은 도구 (`worker/tools.ps1`) |
| 5. 브라우저 ego lite | 없으면 공식 DMG 를 받아 설치하고 앱을 엶 | 설치 스크립트가 없어 내려받기 페이지(lite.ego.app)를 엶 |
| 6. API 키 | KOSIS → VWorld → SGIS → 법령 LAW_OC 를 하나씩 물음 | 같음 |
| 7. 자동 실행 | launchd `kr.seliq.pes-bridge` (로그인하면 켜지고, 꺼지면 다시 켬) | 작업 스케줄러 「PES Bridge」 (로그인하면 창 없이 켜지고, 꺼지면 5초 뒤 다시 켬) |

조사 도구는 모두 GitHub 저장소에서 받습니다 — kosis·vworld·sgis·korean-law 는 [clazic](https://github.com/clazic), kordoc 은 원본 [chrisryugj/kordoc](https://github.com/chrisryugj/kordoc)(npm 패키지).

## 처음 한 번 직접 할 일

1. **Claude Code 로그인** — 설치 중 로그인 창이 열리면 로그인합니다. worker 는 이 로그인(내 Claude 구독)으로 일하고, 사용량도 이 계정에서 나갑니다.
2. **PES 「허용」** — worker 가 처음 켜지면 브라우저에 PES 「허용」 화면이 열립니다. PES 에 로그인한 계정으로 허용을 누르면 끝입니다(토큰은 `~/.config/pes-bridge/accounts/a1/token.json` 에 두고 스스로 갱신).
3. **API 키** — 설치 중 단계별로 묻습니다. 발급 페이지를 브라우저로 열어 주고, 키는 화면에 보이지 않게 받아 실제 호출로 확인한 뒤 각 도구 설정에만 저장합니다. 없는 키는 Enter 로 건너뛰고 나중에 넣어도 됩니다.
4. **ego lite** — 앱의 첫 실행 안내를 끝내고, 로그인하지 않은 새 프로필을 **「PES」** 라는 이름으로 만들어 두면 worker 는 그 프로필로만 원문을 엽니다(로그인된 내 계정이 쓰이지 않게).

## 사용법

1. [pes.seliq.kr](https://pes.seliq.kr) 에서 새 프로젝트를 만들 때 「외부 AI 사용자 모드」 또는 「외부 AI 자동 모드」를 고릅니다. (이미 있는 프로젝트는 맨 위 「작업 AI」를 「외부 AI」로)
2. 그다음은 PES 화면에서 평소처럼 하면 됩니다 — 라운드 글쓰기, 점검 질문 답(자동 모드), 채팅 답을 이 컴퓨터의 worker 가 합니다.
3. 컴퓨터가 **켜져 있고 잠자기 상태가 아니며 사용자로 로그인돼 있어야** 일합니다. 오래 맡길 때는 잠자기를 꺼 두세요(macOS: 시스템 설정 → 배터리 → 전원 어댑터에서 「디스플레이가 꺼졌을 때 자동으로 잠자지 않기」).

같은 PES 계정으로 worker 를 두 대 켜 두면 둘 다 같은 일을 받습니다 — **한 대만** 켜 두세요.

### 쓸 AI 고르기 — Claude Code · Codex · OpenCode

브리지는 **프로젝트마다** PES 에서 고른 AI로 일합니다 — 프로젝트 화면 맨 위 「작업 AI」 옆 「외부 AI 종류」에서 고르고, 따로 고르지 않은 프로젝트는 「외부 AI」 창의 기본 AI가 맡습니다. 일을 받을 때마다 PES 가 일마다 맡을 AI를 알려 주므로, 바꾸면 다음 일부터 그 AI가 합니다. 한 번에 받은 일이 여러 AI 몫이면 AI별로 묶어 차례로 돌리고, 각 AI에게는 자기 몫 프로젝트만 맡깁니다.

| AI | 실행 | PES 연결 | 조사 도구 · 권한 |
|---|---|---|---|
| Claude Code | `claude -p --restricted` | `--mcp-config`(계정 폴더 `mcp.json`) | 이름으로 묶은 도구만(`--tools` · `--allowedTools`), 내 설정 · 훅 · CLAUDE.md 는 읽지 않음, 파일 읽기는 작업 폴더 · `~/.pes/docs` · 조사 도구 사용법 폴더만 |
| Codex | `codex exec --json` | `-c mcp_servers.pes.url=…` + 토큰은 환경 변수 `PES_MCP_TOKEN` | 작업 폴더(`~/.pes/work/<계정>`) · `~/.pes/docs` 만 쓰기, 인터넷 허용, 확인 없이(`approval_policy=never`), 사용자 설정 · 규칙 · 훅은 읽지 않음, 홈 폴더 읽기 막음(권한 프로필 — 「안전」 참고) |
| OpenCode | `opencode run --standalone --format json --auto`(떠 있는 백그라운드 서비스 말고 이 실행만의 서버 — 그래야 계정 설정 · 키를 읽는다) | 계정 폴더 `opencode-round.json` · `opencode-chat.json` · `opencode-polish.json`(`OPENCODE_CONFIG`, 토큰은 `{env:PES_MCP_TOKEN}`, `oauth: false`) | 셸은 조사 도구(kosis · vworld · sgis · korean-law · kordoc · ego-browser)만, 파일 고치기 막음, 작업 폴더 밖 읽기 막음, 내 설정 폴더는 싣지 않음 |

설치 스크립트가 Codex(`@openai/codex`) · OpenCode(`opencode-ai`)를 확인하고 없으면 받습니다 — macOS · Linux 는 `~/.pes/runtime`(npm `--prefix`), Windows 는 npm 전역(`%APPDATA%\npm`). 설치 뒤에 지운 경우에도 그 AI를 고른 계정이 일을 받으면 브리지가 다시 받습니다. **처음 한 번 로그인은 사람이 합니다**: `codex login` · `opencode auth login`. 준비 상태는 `node ~/.pes/bridge/worker/worker.mjs --check` 로 봅니다.

### 한 컴퓨터에 PES 계정 여러 개

브리지 하나가 여러 PES 계정의 일을 함께 받습니다. 계정마다 `~/.config/pes-bridge/accounts/<번호>/` 에 토큰을 따로 두고, 알림 연결 · 실행을 따로 합니다(한 계정이 라운드를 쓰는 동안 다른 계정의 채팅에도 답합니다). 각 계정은 **자기 프로젝트만** 받습니다(관리자 계정도 남의 프로젝트는 받지 않습니다).

```bash
node ~/.pes/bridge/worker/worker.mjs --add      # 계정 더하기 — 브라우저에서 붙일 계정으로 PES 에 로그인한 뒤 「허용」
node ~/.pes/bridge/worker/worker.mjs --list     # 붙은 계정 보기
node ~/.pes/bridge/worker/worker.mjs --remove a2   # 계정 떼기(번호 또는 이메일)
```

켜져 있는 브리지가 30초 안에 새 계정을 붙이고, 뗀 계정은 연결을 끊습니다. 같은 계정을 두 번 붙이면 하나만 씁니다. 예전 설치의 `~/.config/pes-bridge/token.json` 은 켤 때 첫 계정(`a1`)으로 옮깁니다. Windows 는 `node "%USERPROFILE%\.pes\bridge\worker\worker.mjs" --add`.

## 관리 명령

| 하는 일 | macOS | Windows (PowerShell) |
| --- | --- | --- |
| 로그 보기 | `tail -f ~/Library/Logs/pes-bridge.log` | `Get-Content "$env:LOCALAPPDATA\pes-bridge\worker.log" -Wait -Tail 20` |
| 다시 켜기 | `launchctl kickstart -k gui/$(id -u)/kr.seliq.pes-bridge` | `Stop-ScheduledTask "PES Bridge"; Start-ScheduledTask "PES Bridge"` |
| 끄기 | `launchctl bootout gui/$(id -u)/kr.seliq.pes-bridge` | `Stop-ScheduledTask "PES Bridge"` |
| API 키 다시 넣기 | `sh ~/.pes/bridge/worker/tools.sh keys` | `powershell -ExecutionPolicy Bypass -File "$env:USERPROFILE\.pes\bridge\worker\tools.ps1" keys` |
| 준비 상태 점검(연결 없이) | `node ~/.pes/bridge/worker/worker.mjs --check` | `node "$env:USERPROFILE\.pes\bridge\worker\worker.mjs" --check` |
| 도구 · 키 상태 | `sh ~/.pes/bridge/worker/tools.sh check` | `powershell -ExecutionPolicy Bypass -File "$env:USERPROFILE\.pes\bridge\worker\tools.ps1" check` |
| 업데이트 | 스스로(아래) · 바로 하려면 설치 명령을 다시 실행 | 스스로(아래) · 바로 하려면 설치 명령을 다시 실행 |
| 지우기 | `curl -fsSL https://raw.githubusercontent.com/clazic/pes-bridge/main/uninstall.sh \| sh` | `irm https://raw.githubusercontent.com/clazic/pes-bridge/main/uninstall.ps1 \| iex` |

**스스로 업데이트** — 브리지는 켤 때와 30분마다 이 저장소 `main` 의 최신 커밋을 확인하고, 깔린 것과 다르면 쉬는 중(라운드 · 채팅 · 다듬기가 없을 때)에 받아 `~/.pes/bridge` 에 덮어쓴 뒤 스스로 다시 켜집니다. 받은 판은 덮어쓰기 전에 한 번 띄워 보고(`--version`), 뜨지 않으면 예전 판으로 계속합니다. 깔린 커밋은 `~/.pes/bridge/.rev` 에 적고, PES 「PES 브리지 컴퓨터」의 판에 앞 7자가 붙습니다(예: `2026.11.26-update+c00af43`). 끄려면 환경변수 `PES_BRIDGE_NO_UPDATE=1`. 설치 스크립트가 하는 일(자동 실행 등록 · Node.js 같은 실행 환경)이 바뀐 판은 스스로 못 바꾸니 설치 명령을 다시 실행해 주세요. 이 기능이 없는 예전 판(2026.11.26-device 까지)은 설치 명령을 한 번 다시 실행해야 그 뒤부터 스스로 업데이트합니다.

로그에 「PES 알림에 붙었어요」가 보이면 연결된 것입니다. 「키가 없어요: …」가 보이면 그 키를 넣어 주세요. 지우기는 worker 와 자동 실행만 지웁니다(조사 도구 · API 키 · Node.js · Claude Code 는 그대로, 연결 토큰까지 지우려면 `PES_BRIDGE_PURGE=1`).

## 모델 바꾸기

- 단계별 모델: PES 「외부 AI」 창의 「단계별 모델」에서 AI(Claude Code · Codex · OpenCode)마다 기획 · 조사 · 분석·종합 · 집필 · 검증 · 산출 단계의 모델을 고릅니다. 브리지는 PES 에 붙을 때 이 컴퓨터에서 쓸 수 있는 모델 목록을 올리고(Claude Code 시작 응답 · `codex debug models` · `opencode models`, 6시간에 한 번 다시 읽음), 일마다 붙어 오는 `model` 로 돌립니다(`--model` · `-m` · 설정 `model`). 단계별 모델을 고른 계정은 한 번 실행이 지금 단계까지만 하고, 다음 단계는 그 단계의 모델로 새로 띄웁니다.
- Codex 의 「기본」 모델: `~/.codex/config.toml` 의 `model` 입니다. 브리지는 Codex 를 내 설정을 읽지 않게(`--ignore-user-config`) 띄우므로 그 값을 읽어 `-m` 으로 넘깁니다(없으면 Codex 자체 기본 모델).
- 단계별 AI: 같은 자리의 「단계별 AI」에서 단계마다 맡을 AI를 섞어 고를 수 있습니다(프로젝트에 AI를 따로 고르지 않았을 때). 브리지는 이 컴퓨터에 붙어 있는 AI(설치 + 로그인)를 붙을 때와 한 시간마다 알리고, PES 화면은 그 AI만 고르게 합니다.
- 단계별 effort(생각 깊이): 같은 자리에서 단계마다 고릅니다. Claude Code 는 `--effort`, Codex 는 `-c model_reasoning_effort=`, OpenCode 는 `--model 모델#값` 으로 넘깁니다. OpenCode 는 모델마다 되는 값이 달라, 안 되는 값이면(`Variant unavailable`) effort 없이 다시 돌립니다.
- 고르지 않은 단계(「기본」)의 라운드 글쓰기 · 점검: Claude Code 의 기본 모델(`claude` 에서 `/model` 로 고르면 `~/.claude/settings.json` 의 `model` 에 저장)을 따릅니다 — 브리지가 그 값을 읽어 `--model` 로 넘깁니다.
- 채팅 답 · 「한글로 열기」 다듬기: 빠른 답을 위해 `sonnet` 을 씁니다(`worker/worker.mjs` 의 `agent`).

## 이 컴퓨터 알리기 · 허용(PES 「PES 브리지 컴퓨터」)
브리지는 PES 알림에 붙기 전에 이 컴퓨터 정보를 PES 에 알립니다(`POST /mcp/computer`). PES 는 붙은 시각 · 끊긴 시각과 함께 남기고, 관리자와 본인만 봅니다.
- 보내는 것: 컴퓨터 이름, 로그인 사용자명, MAC 주소(실제 네트워크 장치만), OS · 판 · CPU 종류, 브리지 판 · Node 판, 깔린 AI, 계정 번호(a1 …), 기기 ID(하드웨어 UUID 의 sha256 앞 32자 — UUID 자체는 보내지 않음. UUID 를 못 읽는 컴퓨터는 `~/.config/pes-bridge/machine-id` 에 처음 한 번 만든 무작위 ID — 다시 설치해도 남고 `PES_BRIDGE_PURGE=1` 지우기 때만 지움. 컴퓨터 이름 · MAC 은 ID 에 쓰지 않아 이름을 바꿔도 같은 컴퓨터로 잡힘). 접속 IP 는 PES 가 봅니다.
- 쓸 수 있는지: Max 이상 요금제와 관리자는 바로, Starter · Pro 는 PES 관리자가 이 컴퓨터를 허용해야 씁니다. 막혀 있으면 로그에 「이 컴퓨터는 지금 PES 를 쓸 수 없어요」가 남고 10분마다 다시 확인합니다. 관리자가 막으면 붙어 있던 연결도 끊깁니다(4003).

## 내 자료 기억(PES 「AI 기억」 창)
브리지를 쓰면 원문 읽기 · 조사 · 글쓰기는 이 컴퓨터(내 구독)가 하고, **「내 자료 기억」만 PES 서버(m1 pes-rag)를 씁니다** — 여러 컴퓨터 · 프로젝트에서 같은 내 자료를 찾게 하려고요.
- 라운드: 새로 조사하기 전에 PES MCP `my_docs_search` 로 내가 예전에 읽은 자료를 먼저 찾고, 이 컴퓨터에서 읽은 1차 자료는 `keep_source` 로 맡깁니다(요약 · 법령 · 기관 · 주제 · 관계를 함께 달아 더 잘 찾게). 채팅 답에서도 `my_docs_search` 를 씁니다.
- 쉴 때 다듬기: PES 「AI 기억」 창에서 「외부 AI가 쉴 때 다듬기」를 켜면, 라운드 · 채팅 · 다듬기가 하나도 돌지 않을 때(라운드가 끝나고 1분 뒤 · 붙은 뒤 2분 · 한 시간마다) 아직 다듬지 않은 내 자료에 주석을 답니다(`rag_pending_docs` → `rag_doc` → `rag_annotate`, 빠른 모델). 내 구독을 쓰므로 하루 20개까지만(PES 가 셉니다), 한 번 실행에 6개까지 · 20분 상한(`PES_BRIDGE_ENRICH_MAX_MIN`).
- 내 자료는 나만 씁니다. PES 연결 토큰으로 사람을 가르고, pes-rag 토큰은 이 컴퓨터에 오지 않습니다.

## 멈춤 감지 시간 바꾸기

환경 변수(분)로 바꿉니다: `PES_BRIDGE_IDLE_MIN`(응답 없음, 기본 10) · `PES_BRIDGE_ROUND_MAX_MIN`(라운드 상한, 기본 120) · `PES_BRIDGE_CHAT_MAX_MIN`(채팅 답 상한, 기본 60 — 붙인 문서의 그림을 하나씩 다 보므로) · `PES_BRIDGE_POLISH_MAX_MIN`(기본 20).

## 안전

- **조사 도구 키는 PES 에 등록한 것을 씁니다**(PES 「도구」). 브리지가 라운드를 시작할 때 PES 서버에서 키를 받아 **메모리에만** 두고, `127.0.0.1` 에 연 중계가 조사 도구 요청에 키를 끼워 기관 서버로 보냅니다. AI 와 조사 도구 프로그램에는 키가 가지 않습니다 — 중계 주소와 그 실행에만 통하는 토큰만 갑니다. 규약은 [RELAY.md](RELAY.md), 코드는 `worker/relay.mjs`.
  - 키를 받는 길: 계정 폴더의 기기 키(`device.json`, 본인만 읽기)로 서명한 요청만 서버가 받고, 요청마다 만든 일회용 키로 암호화해 돌려줍니다. 연결 토큰만으로는 받지 못합니다.
  - 중계는 도구마다 정해진 기관 주소로만 보내고, 응답에 키가 섞여 오면 지웁니다. 실행이 끝나면 토큰이 죽고, 로그에 「조사 도구 사용: kosis 키1 12회 …」가 남습니다. 같은 수를 PES 서버에도 올려, PES 「도구」 창의 키마다 「오늘 · 이번 달」 사용량으로 보입니다.
  - 이 컴퓨터의 조사 도구에 따로 넣은 키는 **PES 일에는 쓰지 않습니다.** 터미널에서 `kosis` · `vworld` · `sgis` · `korean-law` 를 직접 쓸 때만 씁니다. (조사 도구가 중계를 모르는 옛 판이면 그 도구는 지금처럼 이 컴퓨터의 키로 돕니다 — 새 판으로 올리세요.)
- 키 · 토큰은 이 컴퓨터에만 둡니다: PES 연결 토큰 `~/.config/pes-bridge/`(본인만 읽기), 조사 도구 키는 각 도구 설정, LAW_OC 는 `~/.claude/settings.json` 의 `env`.
- worker 의 Claude Code 는 허용한 도구만 씁니다: PES MCP · 웹 검색/읽기 · 스킬 · 파일 읽기, 그리고 `kosis` · `vworld` · `sgis` · `korean-law` · `kordoc` · `ego-browser` 로 시작하는 명령 하나씩(파이프 · `&&` · `;` 없이).
- Claude Code 는 `--restricted` 로 돕니다(Claude Code 2.1 의 새 판부터 — 없는 판이면 예전 방식으로 돌고 로그에 업데이트하라고 남깁니다): 내 `~/.claude` 설정 · 훅 · 플러그인 · CLAUDE.md 가 브리지 일에 섞이지 않고, 파일 읽기는 작업 폴더(`~/.pes/work/<계정>`) · `~/.pes/docs` · 조사 도구 사용법 폴더(`~/.claude/skills/kosis` · `vworld` · `sgis` · ego-browser) 안으로만 됩니다. 토큰이 있는 계정 폴더는 읽지 못합니다.
- 어느 AI든 작업 폴더는 토큰이 없는 빈 폴더(`~/.pes/work/<계정>`)입니다. 법령 값 `LAW_OC` 는 브리지가 `~/.claude/settings.json` 에서 읽어 환경 변수로 넘깁니다(Codex · OpenCode 도 법령 조회가 됩니다).
- Codex 는 셸 명령을 이름으로 묶지 못합니다(「조사 도구 명령만」은 규칙에 기댑니다). 대신 셸이 닿는 곳을 좁힙니다(macOS · Linux, 권한 프로필이 있는 판): 내 홈 폴더는 읽지 못하고 작업 폴더 · `~/.pes/docs` · 조사 도구 폴더(`~/.kosis` · `~/.sgis` · `~/.vworld` · `~/.local` · `~/.pes/runtime` · 사용법 폴더)만 엽니다. PES 토큰은 셸 환경에서 빼고, 내 규칙(`~/.codex/rules`) · 훅은 싣지 않습니다. 채팅 · 다듬기 갈래는 인터넷도 끕니다. 이 샌드박스 안에서는 ego-browser 가 앱을 찾지 못해 돌지 않습니다. Windows 와 옛 판은 예전 방식(작업 폴더 밖 쓰기만 막힘)입니다.
- OpenCode 는 내 설정 폴더(`~/.config/opencode` 의 MCP 서버 · 플러그인 · 명령)를 싣지 않고 모델 · 제공자 설정만 옮겨 옵니다. 작업 폴더 밖 파일은 `~/.pes/docs` · 조사 도구 사용법 폴더만 읽고, 채팅 · 다듬기 갈래는 셸 · 웹 읽기를 모두 막습니다.
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
