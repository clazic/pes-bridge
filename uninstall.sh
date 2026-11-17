#!/bin/sh
# PES 외부 AI채팅(worker) 끄기 · 지우기 — macOS
#   curl -fsSL https://raw.githubusercontent.com/clazic/pes-worker/main/uninstall.sh | sh
# launchd 등록과 worker 파일(~/.pes/worker)을 지운다. 연결 토큰(~/.config/pes-worker)은 PES_WORKER_PURGE=1 일 때만 지운다.
# 조사 도구 · API 키 · Node.js · Claude Code 는 그대로 둔다(다른 데서도 쓴다).
LABEL=kr.seliq.pes-worker
launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null && echo "껐어요: $LABEL" || echo "켜져 있지 않아요: $LABEL"
rm -f "$HOME/Library/LaunchAgents/$LABEL.plist"
rm -rf "$HOME/.pes/worker"
[ "${PES_WORKER_PURGE:-}" = 1 ] && rm -rf "$HOME/.config/pes-worker" && echo "연결 토큰도 지웠어요"
echo "지웠어요. 다시 설치: curl -fsSL https://raw.githubusercontent.com/clazic/pes-worker/main/install.sh | zsh"
