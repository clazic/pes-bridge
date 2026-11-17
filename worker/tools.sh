#!/bin/sh
# PES 외부 AI worker 의 조사 도구 — 모두 GitHub clazic 저장소에서 받는다.
#   kosis       clazic/kosis        릴리스 실행 파일 + 스킬 (scripts/install.sh)
#   vworld      clazic/vworld       릴리스 실행 파일 + 스킬 (scripts/install.sh)
#   sgis        clazic/sgis         릴리스 실행 파일 + 스킬 (scripts/install.sh)
#   korean-law  clazic/korean-law-mcp  소스를 받아 빌드(npm ci · tsc) → npm 전역 설치
# 문서 변환 · 브라우저(clazic 이 아닌 원본):
#   kordoc      chrisryugj/kordoc   npm 패키지 kordoc(그 저장소가 배포) → HWP · HWPX · PDF · 오피스 → Markdown
#   ego-browser ego lite 앱(lite.ego.app) — macOS 는 공식 DMG 를 받아 설치, Windows 는 설치 파일을 직접 받아야 한다
# 사용:
#   sh tools.sh install   빠진 도구만 설치(묻지 않음 — worker 가 켜질 때 부른다)
#   sh tools.sh keys      API 키를 단계별로 물어 각 도구에 등록하고 확인한다(터미널에서)
#   sh tools.sh ego       ego lite 가 없으면 설치하고 앱을 연다(첫 실행 안내는 사람이 끝낸다 — 터미널에서)
#   sh tools.sh check     줄마다 "<OK|MISSING|KEY> <이름> <설명>"
# 키 값은 화면에 보이지 않게 받고, 출력에는 앞뒤 4자리만 보인다.
set -u
RT="$HOME/.pes/runtime"
PATH="$RT/bin:$RT/node/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"; export PATH
RAW=https://raw.githubusercontent.com/clazic
SETTINGS="$HOME/.claude/settings.json"
have() { command -v "$1" >/dev/null 2>&1; }
mask() { printf '%s' "$1" | awk '{ n = length($0); print (n > 8 ? substr($0, 1, 4) "…" substr($0, n - 3) : "****") }'; }

# ── 키 상태 ──
kosis_n() { have kosis || { echo 0; return; }; kosis config key-list 2>/dev/null | grep -cE '^[[:space:]]*\[[0-9]+\]'; }
vworld_n() { have vworld || { echo 0; return; }; vworld config list-keys 2>/dev/null | grep -o '"masked"' | wc -l | tr -d ' '; }
sgis_ok() { have sgis && sgis config get 2>/dev/null | grep -E '^consumer_key:' | grep -qv '미설정'; }
law_oc() { [ -f "$SETTINGS" ] && node -e 'try{const d=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));process.stdout.write(d.env?.LAW_OC||"")}catch{}' "$SETTINGS" 2>/dev/null; }

install_one() {
  case "$1" in
    kosis)  curl -fsSL "$RAW/kosis/main/scripts/install.sh" | KOSIS_TARGET=claude KOSIS_CLAUDE_SCOPE=global sh ;;
    vworld) curl -fsSL "$RAW/vworld/main/scripts/install.sh" | VWORLD_SCOPE=user bash ;;
    sgis)   curl -fsSL "$RAW/sgis/main/scripts/install.sh" | SGIS_TARGET=claude SGIS_CLAUDE_SCOPE=global sh ;;
    korean-law)
      have npm || { echo "npm 이 없어요 — bootstrap.sh 로 Node.js 를 먼저 준비해 주세요"; return 1; }
      T=$(mktemp -d) && (
        cd "$T" && curl -fsSL https://codeload.github.com/clazic/korean-law-mcp/tar.gz/refs/heads/main | tar -xz --strip-components 1 &&
        npm ci --ignore-scripts --no-audit --no-fund --loglevel=error && npm run build --silent && npm pack --silent >/dev/null &&
        npm install -g ./korean-law-mcp-*.tgz --ignore-scripts --no-audit --no-fund --loglevel=error
      ); r=$?; rm -rf "$T"; return $r ;;
    kordoc)
      have npm || { echo "npm 이 없어요 — bootstrap.sh 로 Node.js 를 먼저 준비해 주세요"; return 1; }
      npm install -g kordoc@latest --no-audit --no-fund --loglevel=error ;;
  esac
}
TOOLS="kosis vworld sgis korean-law kordoc"

cmd_install() {
  for t in $TOOLS; do
    if have "$t"; then echo "OK $t $(command -v "$t")"; continue; fi
    echo "... $t 설치 중"
    if install_one "$t" >/dev/null 2>&1 && have "$t"; then echo "INSTALLED $t"; else echo "FAIL $t 설치하지 못했어요"; fi
  done
}

cmd_check() {
  for t in $TOOLS; do have "$t" && echo "OK $t $(command -v "$t")" || echo "MISSING $t"; done
  have ego-browser && echo "OK ego-browser $(command -v ego-browser)" || echo "MISSING ego-browser"
  n=$(kosis_n); [ "$n" -gt 0 ] 2>/dev/null && echo "OK key-kosis ${n}개" || echo "KEY key-kosis KOSIS API 키"
  n=$(vworld_n); [ "$n" -gt 0 ] 2>/dev/null && echo "OK key-vworld ${n}개" || echo "KEY key-vworld VWorld API 키"
  sgis_ok && echo "OK key-sgis 등록됨" || echo "KEY key-sgis SGIS 서비스 ID · 보안 Key"
  oc=$(law_oc); [ -n "$oc" ] && echo "OK key-law $(mask "$oc")" || echo "KEY key-law 법령 LAW_OC"
}

# ── ego lite(ego-browser) — 운영체제를 보고 설치한다. 첫 실행 안내(브라우저 가져오기 · 명령 등록)는 사람이 앱에서 끝낸다 ──
EGO_APP="ego lite.app"
ego_app() { for a in "/Applications/$EGO_APP" "$HOME/Applications/$EGO_APP"; do [ -d "$a" ] && { echo "$a"; return 0; }; done; return 1; }
cmd_ego() {
  if have ego-browser; then echo "OK ego-browser $(command -v ego-browser)"; return 0; fi
  case "$(uname -s)" in
    Darwin)
      if A=$(ego_app); then open "$A"; echo "USER ego-browser ego lite 는 설치돼 있어요 — 열린 앱에서 첫 실행 안내를 끝내면 ego-browser 명령이 생겨요"; return 0; fi
      case "$(uname -m)" in arm64) U=https://cdn.ego.app/setup/macos/arm64/egolite-fHoqgZ74bOEM.dmg ;; *) U=https://cdn.ego.app/setup/macos/x64/egolite-fHoqgZ74bOEM.dmg ;; esac
      T=$(mktemp -d); M="$T/mnt"; mkdir -p "$M"
      if curl -fL --retry 3 -s -o "$T/ego.dmg" "$U" && hdiutil attach "$T/ego.dmg" -nobrowse -readonly -mountpoint "$M" >/dev/null 2>&1; then
        S=$(find "$M" -maxdepth 2 -type d -iname "$EGO_APP" | head -n 1)
        D=/Applications; [ -w "$D" ] || { D="$HOME/Applications"; mkdir -p "$D"; } # 관리자 권한 없이: 쓸 수 없으면 내 Applications
        if [ -n "$S" ] && ditto "$S" "$D/$EGO_APP"; then
          xattr -dr com.apple.quarantine "$D/$EGO_APP" >/dev/null 2>&1 || true
          echo "INSTALLED ego-browser $D/$EGO_APP"; open "$D/$EGO_APP"
          echo "USER ego-browser 열린 ego lite 에서 첫 실행 안내를 끝내 주세요(ego-browser 명령이 ~/.local/bin 에 생겨요). 외부 AI 전용으로 로그인 없는 「PES」 프로필을 만들어 두면 그것만 써요"
        else echo "FAIL ego-browser 설치 파일에서 앱을 찾지 못했어요 — https://lite.ego.app/ 에서 직접 받아 주세요"; fi
        hdiutil detach "$M" -quiet >/dev/null 2>&1
      else echo "FAIL ego-browser 내려받지 못했어요 — https://lite.ego.app/ 에서 직접 받아 주세요"; fi
      rm -rf "$T" ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
      # Windows 는 설치 스크립트가 없다 — 설치 파일을 받아 실행해야 한다(SmartScreen 이 막으면 「추가 정보」 → 「실행」)
      echo "USER ego-browser Windows 예요 — https://lite.ego.app/ 에서 설치 파일을 받아 실행하고, 첫 실행 안내를 끝낸 뒤 새 터미널을 열어 주세요"
      (cmd.exe /c start https://lite.ego.app/ >/dev/null 2>&1 || true) ;;
    *) echo "USER ego-browser 이 운영체제($(uname -s))용 설치 파일은 없어요 — https://lite.ego.app/ 를 확인해 주세요" ;;
  esac
}

# ── 키를 단계별로 묻는다 ──
TTY=/dev/tty
say() { printf '%s\n' "$*" > "$TTY"; }
ask_secret() { printf '%s' "$1" > "$TTY"; stty -echo < "$TTY" 2>/dev/null; IFS= read -r v < "$TTY"; stty echo < "$TTY" 2>/dev/null; printf '\n' > "$TTY"; printf '%s' "$v" | tr -d '[:space:]'; }
ask_yn() { printf '%s' "$1" > "$TTY"; IFS= read -r v < "$TTY"; case "$v" in [nN]*) return 1 ;; *) return 0 ;; esac; }
open_url() { if have open; then open "$1" >/dev/null 2>&1; elif have xdg-open; then xdg-open "$1" >/dev/null 2>&1; fi; }
step() { # $1 번호 $2 제목 $3 발급 주소, 나머지: 발급 순서
  sn=$1; title=$2; url=$3; shift 3 # sh 에는 지역 변수가 없다 — 키 개수 n 을 덮지 않게 다른 이름
  say ""; say "[$sn/4] $title"
  i=1; for s in "$@"; do say "   $i. $s"; i=$((i + 1)); done
  say "   발급 페이지: $url"
  ask_yn "   발급 페이지를 브라우저로 열까요? [Y/n] " && open_url "$url"
}

reg_kosis() {
  have kosis || { say "   kosis 가 설치돼 있지 않아요 — 건너뛰어요"; return; }
  if KOSIS_API_KEY="$1" kosis s 인구 -n 1 >/dev/null 2>&1 && kosis config add-key "$1" >/dev/null 2>&1; then say "   ✓ 등록 · 확인했어요 ($(mask "$1"))"; return 0; fi
  say "   ✗ 키 확인에 실패했어요 — 키를 다시 확인해 주세요"; return 1
}
reg_vworld() {
  have vworld || { say "   vworld 가 설치돼 있지 않아요 — 건너뛰어요"; return; }
  vworld config add-key "$1" --alias pes >/dev/null 2>&1
  m=$(mask "$1")
  v=$(vworld config test-keys 2>/dev/null | node -e 'let t="";process.stdin.on("data",d=>t+=d).on("end",()=>{try{const k=process.argv[1];for(const r of JSON.parse(t.slice(t.indexOf("{"))).results??[]){const s=String(r.key??"");if(s.slice(0,4)===k.slice(0,4)&&s.slice(-4)===k.slice(-4))process.stdout.write(String(r.verdict??""))}}catch{}})' "$1")
  if [ "$v" = 유효 ]; then say "   ✓ 등록 · 확인했어요 ($m)"; return 0; fi
  vworld config remove-key "$1" >/dev/null 2>&1
  say "   ✗ VWorld 확인 결과 유효하지 않아요 — 발급 직후면 몇 분 뒤 다시 해 주세요"; return 1
}
reg_sgis() {
  have sgis || { say "   sgis 가 설치돼 있지 않아요 — 건너뛰어요"; return; }
  c="$HOME/.sgis/config.yaml"; [ -f "$c" ] && cp "$c" "$c.bak-pes"
  if sgis config set-credential "$1" "$2" >/dev/null 2>&1 && sgis code stage >/dev/null 2>&1; then say "   ✓ 등록 · 확인했어요 (서비스 ID $(mask "$1"))"; return 0; fi
  if [ -f "$c.bak-pes" ]; then mv "$c.bak-pes" "$c"; else rm -f "$c"; fi # 확인 못 한 키는 남기지 않는다
  say "   ✗ SGIS 인증에 실패했어요 — 서비스 ID와 보안 Key를 다시 확인해 주세요"; return 1
}
reg_law() {
  if have korean-law && ! LAW_OC="$1" korean-law search_law --query 민법 2>/dev/null | grep -q '검색 결과'; then say "   ✗ 법령 API 접속을 확인하지 못했어요 — 값을 다시 확인해 주세요"; return 1; fi
  mkdir -p "$(dirname "$SETTINGS")"; [ -f "$SETTINGS" ] && cp "$SETTINGS" "$SETTINGS.bak-pes"
  node -e 'const f=require("fs"),p=process.argv[1];let d={};try{d=JSON.parse(f.readFileSync(p,"utf8"))}catch{};d.env=d.env||{};d.env.LAW_OC=process.argv[2];f.writeFileSync(p,JSON.stringify(d,null,2)+"\n")' "$SETTINGS" "$1" \
    && say "   ✓ Claude Code 설정(env)에 등록했어요 ($(mask "$1"))"
}

cmd_keys() {
  ( : < "$TTY" ) 2>/dev/null || { echo "키 입력은 터미널에서 해 주세요: sh $0 keys"; return 1; }
  say ""; say "PES 외부 AI 조사 도구 — API 키 등록"
  say "키는 이 컴퓨터의 각 도구 설정에만 저장돼요. 화면에는 보이지 않아요. 없는 키는 Enter 로 건너뛰세요."

  n=$(kosis_n)
  step 1 "KOSIS (통계청 국가통계포털) API 키" "https://kosis.kr/openapi/" "kosis.kr/openapi 접속 → 로그인(회원가입)" "상단 '활용신청' → 활용 목적 입력 후 신청" "마이페이지 → 인증키 확인 → 복사"
  [ "$n" -gt 0 ] 2>/dev/null && say "   이미 ${n}개 등록돼 있어요. 더 넣으려면 입력하세요."
  while k=$(ask_secret "   KOSIS 키: "); [ -n "$k" ]; do reg_kosis "$k" && break; done

  n=$(vworld_n)
  step 2 "VWorld (국가공간정보) API 키" "https://www.vworld.kr/dev/v4dv_apikey_s001.do" "vworld.kr 로그인(회원가입)" "오픈API → 인증키 발급" "서비스 URL 없이(무도메인) · 활용 API 전체 선택 → 발급된 키 복사"
  [ "$n" -gt 0 ] 2>/dev/null && say "   이미 ${n}개 등록돼 있어요. 더 넣으려면 입력하세요."
  while k=$(ask_secret "   VWorld 키: "); [ -n "$k" ]; do reg_vworld "$k" && break; done

  step 3 "SGIS (통계지리정보서비스) 서비스 ID · 보안 Key" "https://sgis.mods.go.kr/developer/html/newOpenApi/guide/guide/getApiKey.html" "SGIS 개발지원센터 로그인(회원가입)" "인증키 신청 → 서비스 등록" "발급된 서비스 ID(consumer key)와 보안 Key(consumer secret) 복사"
  sgis_ok && say "   이미 등록돼 있어요. 바꾸려면 입력하세요."
  while k=$(ask_secret "   SGIS 서비스 ID: "); [ -n "$k" ]; do
    s=$(ask_secret "   SGIS 보안 Key: "); [ -n "$s" ] || break
    reg_sgis "$k" "$s" && break
  done

  oc=$(law_oc)
  step 4 "법령 API (국가법령정보 공동활용) — LAW_OC" "https://open.law.go.kr/LSO/openApi/cuAskList.do" "open.law.go.kr 로그인(회원가입)" "OPEN API → 활용 신청 (법령·판례 등 체크)" "승인 후 이메일 ID 앞부분(@ 앞)이 LAW_OC 값"
  [ -n "$oc" ] && say "   이미 등록돼 있어요($(mask "$oc")). 바꾸려면 입력하세요."
  while k=$(ask_secret "   LAW_OC: "); [ -n "$k" ]; do reg_law "$k" && break; done

  say ""; say "등록 상태:"; cmd_check | grep -E '^(OK|KEY) key-' | sed 's/^OK /  ✓ /; s/^KEY /  · 없음 /' > "$TTY"
  mkdir -p "$HOME/.pes" && date > "$HOME/.pes/worker-keys-asked"
}

case "${1:-check}" in
  install) cmd_install ;;
  keys) cmd_keys ;;
  check) cmd_check ;;
  ego) cmd_ego ;;
  *) echo "사용: sh $0 install|keys|ego|check"; exit 2 ;;
esac
