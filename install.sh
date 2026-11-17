#!/bin/zsh
# PES 외부 AI채팅(worker) 원클릭 설치 — macOS
#   curl -fsSL https://raw.githubusercontent.com/clazic/pes-worker/main/install.sh | zsh
#
# 하는 일(다시 실행해도 된다 — 없는 것만 설치하고, worker 파일은 최신판으로 바꾼다)
#   1. 이 저장소를 ~/.pes/worker 에 받는다
#   2. Node.js 26+ · Python 3.10+ · Claude Code 가 없으면 ~/.pes/runtime 에 받는다(sudo · Homebrew 없이, worker/bootstrap.sh)
#   3. Claude Code 로그인이 안 돼 있으면 로그인 창을 연다
#   4. 조사 도구(kosis · vworld · sgis · korean-law · kordoc)를 설치한다(worker/tools.sh)
#   5. 브라우저 ego lite 가 없으면 설치하고 앱을 연다
#   6. API 키(KOSIS · VWorld · SGIS · LAW_OC)를 단계별로 묻는다(처음 · 빠진 키가 있을 때)
#   7. launchd 에 등록해 로그인하면 켜지고, 꺼지면 다시 켜지게 한다
# 환경변수: PES_WORKER_REPO(기본 clazic/pes-worker) · PES_WORKER_SRC(받지 않고 이 폴더를 쓴다 — 시험용)
#           PES_WORKER_TEST=1(시험: 로그인 · 키 · ego lite · launchd 등록을 건너뛴다)
set -e
[[ "$(uname -s)" == Darwin ]] || { echo "이 스크립트는 macOS 용이에요. Windows 는 install.ps1 을 쓰세요(README 참고)."; exit 1; }
REPO=${PES_WORKER_REPO:-clazic/pes-worker}
DEST="$HOME/.pes/worker"
LABEL=kr.seliq.pes-worker
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
TEST=${PES_WORKER_TEST:-}
tty_ok() { [[ -z $TEST ]] && ( : < /dev/tty ) 2>/dev/null; } # 터미널을 실제로 열 수 있을 때만 묻는다(curl | zsh 여도 /dev/tty 로 묻는다)

echo "[1/7] worker 받기 → $DEST"
mkdir -p "$DEST"
if [[ -n ${PES_WORKER_SRC:-} ]]; then
  rsync -a --delete --exclude .git "$PES_WORKER_SRC/" "$DEST/"
else
  T=$(mktemp -d)
  curl -fsSL "https://codeload.github.com/$REPO/tar.gz/refs/heads/main" | tar -xz -C "$T" --strip-components 1
  rsync -a --delete "$T/" "$DEST/"
  rm -rf "$T"
fi
HERE="$DEST/worker"

echo "[2/7] 실행 환경(Node.js · Python · Claude Code)"
OUT="$(sh "$HERE/bootstrap.sh")"
print -r -- "$OUT" | grep -v '^PATH ' | sed 's/^/  /'
WPATH="$(print -r -- "$OUT" | sed -n 's/^PATH //p')"
NODE="$(PATH="$WPATH" command -v node)"
CLAUDE="$(PATH="$WPATH" command -v claude || true)"
[[ -n $NODE ]] || { echo "  Node.js 를 준비하지 못했어요 — 네트워크를 확인하고 다시 실행해 주세요."; exit 1; }

echo "[3/7] Claude Code 로그인"
if [[ -z $CLAUDE ]]; then echo "  claude 를 찾지 못했어요 — https://claude.com/claude-code 에서 설치한 뒤 다시 실행해 주세요."; [[ -n $TEST ]] || exit 1
elif PATH="$WPATH" "$CLAUDE" auth status 2>/dev/null | grep -q '"loggedIn": *true'; then echo "  로그인돼 있어요"
elif tty_ok; then echo "  로그인 창을 열어요 — 브라우저에서 로그인을 마쳐 주세요(worker 가 이 로그인으로 일해요)"; PATH="$WPATH" "$CLAUDE" auth login < /dev/tty > /dev/tty 2>&1 || echo "  로그인을 마치지 못했어요 — 나중에 터미널에서 claude 를 실행해 로그인해 주세요"
else echo "  로그인이 필요해요 — 터미널에서 claude 를 한 번 실행해 로그인해 주세요"; fi

echo "[4/7] 조사 도구"
PATH="$WPATH" sh "$HERE/tools.sh" install | sed 's/^/  /'

echo "[5/7] 브라우저(ego lite)"
if tty_ok; then PATH="$WPATH" sh "$HERE/tools.sh" ego | sed 's/^/  /'; else echo "  건너뜀(터미널이 아니에요)"; fi

echo "[6/7] API 키"
if tty_ok; then
  if [[ ! -f "$HOME/.pes/worker-keys-asked" ]] || PATH="$WPATH" sh "$HERE/tools.sh" check | grep -q '^KEY '; then PATH="$WPATH" sh "$HERE/tools.sh" keys; else echo "  모두 등록돼 있어요"; fi
else echo "  건너뜀 — 나중에: sh \"$HERE/tools.sh\" keys"; fi

echo "[7/7] 자동 실행(launchd) 등록"
if [[ -n $TEST ]]; then echo "  건너뜀(PES_WORKER_TEST)"; echo "완료(시험)"; exit 0; fi
mkdir -p "$HOME/Library/LaunchAgents" "$HOME/.config/pes-worker" "$HOME/Library/Logs"
cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key><array><string>$NODE</string><string>$HERE/worker.mjs</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>EnvironmentVariables</key><dict><key>PATH</key><string>$WPATH</string></dict>
  <key>StandardOutPath</key><string>$HOME/Library/Logs/pes-worker.log</string>
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/pes-worker.log</string>
</dict></plist>
EOF
launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST" 2>/dev/null || { sleep 2; launchctl bootstrap "gui/$(id -u)" "$PLIST"; }
echo "  등록했어요: $LABEL"
[[ -s "$HOME/.config/pes-worker/key" || -s "$HOME/.config/pes-worker/token.json" ]] || echo "  곧 브라우저에 PES 「허용」 화면이 열려요 — 허용을 누르면 외부 AI채팅이 스스로 연결해요."
echo ""
echo "완료. 로그: tail -f ~/Library/Logs/pes-worker.log   (「PES 알림에 붙었어요」가 보이면 연결된 거예요)"
