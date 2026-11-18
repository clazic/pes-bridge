#!/bin/sh
# PES 브리지(pes-bridge) 끄기 · 지우기 — macOS
#   curl -fsSL https://raw.githubusercontent.com/clazic/pes-bridge/main/uninstall.sh | sh
# launchd 등록과 worker 파일(~/.pes/bridge)을 지운다. 연결 토큰(~/.config/pes-bridge)은 PES_BRIDGE_PURGE=1 일 때만 지운다.
# 조사 도구 · API 키 · Node.js · Claude Code 는 그대로 둔다(다른 데서도 쓴다).
LABEL=kr.seliq.pes-bridge
launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null && echo "껐어요: $LABEL" || echo "켜져 있지 않아요: $LABEL"
rm -f "$HOME/Library/LaunchAgents/$LABEL.plist"
rm -rf "$HOME/.pes/bridge"
# 예전 이름(pes-worker)도 함께 지운다
launchctl bootout "gui/$(id -u)/kr.seliq.pes-worker" 2>/dev/null; rm -f "$HOME/Library/LaunchAgents/kr.seliq.pes-worker.plist"; rm -rf "$HOME/.pes/worker"
[ "${PES_BRIDGE_PURGE:-}" = 1 ] && rm -rf "$HOME/.config/pes-bridge" && echo "연결 토큰도 지웠어요"
echo "지웠어요. 다시 설치: curl -fsSL https://raw.githubusercontent.com/clazic/pes-bridge/main/install.sh | zsh"
