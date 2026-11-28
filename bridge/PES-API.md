# Rust 브리지가 PES 서버에 바라는 것 (티켓 22 설계)

Rust 브리지(`bridge/`)는 PC 에서 조사한 원본을 PES 근거 기록으로 올린다. 지금 PES 서버(pes-web)에는 그 길이 없다.
이 문서는 서버에 더할 것을 적는다. 브리지 시험은 가짜 PES(`bridge/tests/round.rs`)가 이 꼴대로 답해서 돌린다.
pes-web 은 이번 티켓에서 고치지 않았다.

## 공통: 기기 키 서명

- 브리지는 계정 폴더의 `device.json`(ECDSA P-256)으로 서명한다. Node 브리지 Vault 와 같은 파일, 같은 꼴이다.
- 먼저 `POST /mcp/device {pub, ts, sig}`(이미 있는 길)로 연결 토큰에 기기 키를 묶는다.
- 서명한 요청 몸통은 `{ts, nonce, r, sig}` 이다.
  - `r` 은 실제 내용을 JSON 글로 바꾼 것이다.
  - `sig` 은 `"<purpose>\n<ts>\n<nonce>\n<r>"` 에 대한 서명이다. 꼴은 r‖s 64바이트를 base64 로 바꾼 것이다(WebCrypto 와 같다).
  - `/mcp/tool-usage` 와 같은 꼴이다.
- 이렇게 하는 까닭: 연결 토큰은 AI 프로세스도 갖고 있다(MCP 연결). 그래서 토큰만으로는 근거 기록을 만들지 못하게 한다.
- 서버가 할 일: `ts` 는 ±5분 안이어야 하고, `nonce` 는 다시 쓰면 거절한다. 그 연결에 묶인 기기 키로 검증한다.

## 1. `POST /mcp/evidence` — PC 에서 한 조회 하나를 근거 기록으로 (purpose `pes-evidence`)

`r` 의 꼴:

```json
{
  "slug": "프로젝트", "stage": "research",
  "tool": "pc_kosis | pc_vworld | pc_sgis | pc_korean_law | pc_fetch | pc_kordoc | pc_ego_read",
  "kind": "lookup | original | search",
  "args": {"cli": "kosis", "args": ["data", "--tbl", "DT_1"]},
  "asked_at": "2026-11-28T00:00:00.000Z",
  "status": "ok | empty | fail", "error": null,
  "title": "…", "url": "받은 최종 주소 또는 cli:<명령>",
  "via": "pc", "route": "pc", "engine": "claude-code", "run": "실행 번호 앞 6자",
  "parts": [{"url": "…", "at": "…", "status": 200, "mime": "text/html; charset=utf-8", "body": "받은 글 그대로"}],
  "binary": {"url": "…", "at": "…", "status": 200, "mime": "application/pdf", "b64": "바이트 그대로"},
  "requery": {"주소": "…"}
}
```

- `kind` 는 브리지가 내는 제안이다. 정하는 것은 서버다(`evidenceKind` 와 같은 규칙).
- `parts` 와 `binary` 는 하나만 온다. UTF-8 이 아닌 글(EUC-KR 등)과 문서 파일은 `binary` 로 온다.

답: `200 {"evidence_id": "ev_…"}`. 그 밖의 답은 모두 실패다. 브리지는 그 도구 호출을 실패로 돌려주고 결과를 AI 에 주지 않는다.

서버가 할 일:
1. 서명을 확인한다.
2. `mayWrite(project, uid)` 로 권한을 본다. 권한이 없으면 403.
3. `recordEvidence` 를 부른다.
   - 값: `caller: 'external_ai'` · `via: 'pc'` · `route: 'pc'` · `tool` · `args` · `parts` / `binary`.
   - 한도는 `RAW_MAX` 25MB 다. 비밀값은 지운다.
4. `markSeen` 처럼 본 범위를 남긴다. 브리지는 `read_more` 로 끝까지 준다. 정확한 범위를 따로 보내는 길은 아래 「남은 것」에 있다.
5. 작업 로그 `evidence_record` 를 남긴다.

표시: 근거 화면에 「PC 에서 연 기록(via pc)」을 따로 보인다. 다시 조회할 주소 · 조건은 `requery` 다.

## 2. `POST /mcp/bridge-log` — 브리지 동작을 서비스 작업 로그로 (purpose `pes-bridge-log`)

`r` = `{"events": [{at, action, project, stage, target, how, why_kind, why, result, detail}]}`

`action` 의 값:

| 값 | 뜻 |
| --- | --- |
| `bridge_round_start` | 라운드 시작(엔진 · 판 · 모델 · 막은 길 요약) |
| `bridge_round_end` | 끝. 오류 · 올린 근거 번호 · 올리지 못한 수 · PES 밖 조사 흔적 |
| `bridge_round_refused` | 옛 판 AI 라서 거부함. 까닭 · 판 |
| `bridge_evidence_fail` | 근거 기록을 올리지 못함 |

서버가 할 일:
- `WorkAction` 에 위 값을 더하고, `actor: 'bridge'`(또는 `external_ai`)로 `workLog` 에 넣는다.
- 비밀값은 들어오지 않는다(브리지가 넣지 않는다). 그래도 `SECRET` 로 한 번 더 지운다.

## 3. 라운드 결과의 「PES 밖 조사 있음」 자리

브리지는 라운드가 끝날 때 아래 두 곳에 PES 밖 조사 흔적을 싣는다. 흔적은 허용 밖 셸 명령 · 내장 웹 가져오기 · 하위 에이전트 · 셸로 부른 ego-browser 다.
- `/mcp/beat` 의 끝 신호(`phase: done|error`)에 `outside: <수>` 를 싣는다.
- `bridge_round_end` 로그의 `detail.outside_research = {found, items:[{tool, command, why}]}` 에 싣는다.

서버가 할 일: 그 라운드(`stages` 의 라운드 기록)에 저장한다. 결과 화면 · 해당 항목에 「PES 밖 조사 있음」을 표시한다. 라운드는 실패로 하지 않는다(티켓 21 6절 6).

## 4. 크레딧

PC AI 라운드는 크레딧을 쓰지 않는다. 브리지는 크레딧 · 결제 길을 부르지 않는다(시험이 확인한다). 서버의 `externalStart` · `externalMerge` · `externalSubmit` 에도 `chargeRun` 이 없다(코드를 읽어 확인했다).

## 남은 것(정할 것)

- 본 범위: 브리지가 실행을 닫을 때 근거 번호마다 AI 에 준 글자 범위를 보낼 수 있다(`POST /mcp/evidence-seen`). 지금은 보내지 않는다.
- 큰 원본: 25MB 를 base64 로 담으면 JSON 이 33MB 쯤 된다. 나눠 올리기(`/mcp/upload` 처럼 바이트 그대로)가 필요한지 정해야 한다.
- 서버의 옛 판 브리지 거부(티켓 24): 연결할 때 보내는 `bridge_ver` 는 `rust-<판>` 이다.
