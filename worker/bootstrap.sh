#!/bin/sh
# PES 외부 AI worker 실행 환경 준비 — 관리자 권한(sudo) · Homebrew 없이 ~/.pes/runtime 에 받는다(macOS · Linux).
#   Node.js 26 이상   worker 와 조사 도구 설치기(PES setup.ts) · korean-law(npm)가 쓴다. 없으면 nodejs.org 공식 파일을 받아 SHA-256 을 확인한다.
#   Python 3.10 이상  없으면 uv(astral.sh)로 Python 3.12 를 받는다.
#   Claude Code CLI   없으면 공식 설치기(claude.ai/install.sh)로 받는다. 처음 한 번 로그인은 사람이 해야 한다.
# 이미 있는 것은 그대로 쓴다. 출력: 줄마다 "<OK|INSTALLED|FAIL|USER> <이름> <설명>", 마지막 줄 "PATH <worker 가 쓸 PATH>".
# 사용: sh scripts/pes-worker/bootstrap.sh
set -u
RT="$HOME/.pes/runtime"; BIN="$RT/bin"
mkdir -p "$BIN"
PATH="$BIN:$RT/node/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"; export PATH

case "$(uname -s)" in Darwin) OS=darwin ;; Linux) OS=linux ;; *) echo "FAIL os $(uname -s) 는 지원하지 않아요"; exit 1 ;; esac
case "$(uname -m)" in arm64|aarch64) AR=arm64 ;; x86_64|amd64) AR=x64 ;; *) echo "FAIL arch $(uname -m) 는 지원하지 않아요"; exit 1 ;; esac
sha() { if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1"; else sha256sum "$1"; fi | awk '{print $1}'; }
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT

# ── Node.js 26+ ──
nodeok() { m=$(node --version 2>/dev/null | sed -n 's/^v\([0-9][0-9]*\)\..*/\1/p'); [ -n "$m" ] && [ "$m" -ge 26 ]; }
if nodeok; then echo "OK node $(node --version) ($(command -v node))"
else
  D=https://nodejs.org/dist/latest-v26.x
  if curl -fsSL "$D/SHASUMS256.txt" -o "$TMP/sums"; then
    F=$(awk -v s="-$OS-$AR.tar.gz" 'substr($2, length($2) - length(s) + 1) == s { print $2; exit }' "$TMP/sums")
    S=$(awk -v f="$F" '$2 == f { print $1 }' "$TMP/sums")
    if [ -n "$F" ] && curl -fsSL "$D/$F" -o "$TMP/$F" && [ "$(sha "$TMP/$F")" = "$S" ]; then
      rm -rf "$RT/node" && mkdir -p "$RT/node" && tar -xzf "$TMP/$F" -C "$RT/node" --strip-components 1
      nodeok && echo "INSTALLED node $(node --version) ($RT/node)" || echo "FAIL node 받았지만 실행하지 못했어요"
    else echo "FAIL node 내려받기 · 확인(SHA-256) 실패 — 네트워크를 확인해 주세요"; fi
  else echo "FAIL node nodejs.org 에 접속하지 못했어요"; fi
fi

# ── Python 3.10+ (macOS 의 /usr/bin/python3 는 개발 도구 설치 창을 띄울 수 있어 보지 않는다) ──
pyfind() { for p in "$BIN/python3" "$HOME/.local/bin/python3" /opt/homebrew/bin/python3 /usr/local/bin/python3; do
  [ -x "$p" ] && "$p" -c 'import sys; sys.exit(0 if sys.version_info >= (3, 10) else 1)' 2>/dev/null && { echo "$p"; return 0; }; done; return 1; }
if P=$(pyfind); then echo "OK python $("$P" --version 2>&1) ($P)"
else
  UV=$(command -v uv || true)
  if [ -z "$UV" ]; then
    curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR="$BIN" UV_NO_MODIFY_PATH=1 sh >/dev/null 2>&1
    UV="$BIN/uv"
  fi
  if [ -x "$UV" ] && "$UV" python install 3.12 >/dev/null 2>&1 && P=$("$UV" python find 3.12 2>/dev/null) && [ -x "$P" ]; then
    ln -sf "$P" "$BIN/python3" && ln -sf "$P" "$BIN/python"
    echo "INSTALLED python $("$BIN/python3" --version 2>&1) (uv)"
  else echo "FAIL python uv 로 Python 을 받지 못했어요"; fi
fi

# ── Claude Code CLI ──
if command -v claude >/dev/null 2>&1; then echo "OK claude $(claude --version 2>/dev/null | head -1) ($(command -v claude))"
else
  curl -fsSL https://claude.ai/install.sh | bash >/dev/null 2>&1
  if command -v claude >/dev/null 2>&1; then
    echo "INSTALLED claude $(claude --version 2>/dev/null | head -1)"
    echo "USER claude 터미널에서 claude 를 한 번 실행해 로그인해 주세요(worker 가 이 로그인으로 일해요)"
  else echo "FAIL claude Claude Code 를 설치하지 못했어요 — https://claude.com/claude-code"; fi
fi

echo "PATH $PATH"
