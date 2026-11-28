# Rust 브리지가 PES 서버에 바라는 것 (티켓 22 설계)

Rust 브리지(`bridge/`)는 PC 에서 조사한 원본을 PES 근거 기록으로 올린다.
1 · 2 · 3절과 `/mcp/evidence-seen` 은 pes-web 브랜치 ticket-22-server(17ca3b0, `src/bridgeapi.ts`)에 있다. **5절(나눠 올리기)은 아직 서버에 없다** — 서버에 더할 꼴이다.
브리지 시험은 가짜 PES(`bridge/tests/round.rs`)가 이 꼴대로 답해서 돌린다.

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
  "upload": {"file_id": "…", "sha256": "…", "size": 1500000, "mime": "text/html; charset=utf-8", "url": "…", "at": "…", "status": 200},
  "seen": {"chars": 120000, "ranges": [[0, 40000]], "pages": 1},
  "requery": {"주소": "…"}
}
```

- `kind` 는 브리지가 내는 제안이다. 정하는 것은 서버다(`evidenceKind` 와 같은 규칙).
- `parts` 와 `binary` 는 하나만 온다. UTF-8 이 아닌 글(EUC-KR 등)과 문서 파일은 `binary` 로 온다.
- 원본이 1MB 를 넘으면 `parts` 첫 글 · `binary` 대신 `upload` 가 온다(5절). 그때 `parts` 에는 덧붙은 작은 글(조사 CLI 의 stderr 등)만 남는다.
- `seen` 은 AI 에게 준 첫 쪽 범위다. 글자 수는 브리지가 AI 에게 준 글(HTML 은 읽을 글로 바꾼 것) 기준이다. 실패 기록 · 문서 파일(글을 주지 않음)에는 없다.
- `route` 는 브리지가 `pc` 로 보내지만, 서버는 `direct` 로 저장하고 `via = 'pc'` 로 구분한다(17ca3b0).

답: `200 {"evidence_id": "ev_…", "sha256": "upload 일 때 그 파일의 지문"}`. 그 밖의 답은 모두 실패다. 브리지는 그 도구 호출을 실패로 돌려주고 결과를 AI 에 주지 않는다.

서버가 할 일:
1. 서명을 확인한다.
2. `mayWrite(project, uid)` 로 권한을 본다. 권한이 없으면 403.
3. `recordEvidence` 를 부른다.
   - 값: `caller: 'external_ai'` · `via: 'pc'` · `route: 'pc'` · `tool` · `args` · `parts` / `binary`.
   - 한도는 `RAW_MAX` 25MB 다. 비밀값은 지운다.
4. `seen` 이 있으면 본 범위로 남긴다(`addEvidenceSeen`). 이어 읽기 범위는 1-1절.
5. 작업 로그 `evidence_record` 를 남긴다.

표시: 근거 화면에 「PC 에서 연 기록(via pc)」을 따로 보인다. 다시 조회할 주소 · 조건은 `requery` 다.

## 1-1. `POST /mcp/evidence-seen` — 이어 읽기로 더 준 범위 (purpose `pes-evidence-seen`)

- `r` = `{"slug", "evidence_id", "chars", "ranges": [[시작, 끝]], "pages"?}`. 브리지는 `read_more` 로 한 쪽을 줄 때마다 보낸다.
- 실패해도 브리지는 그 도구 결과를 실패로 하지 않는다. 글은 이미 AI 에게 주는 중이고 원본은 올라가 있다. 그 범위만 PES 에 「본 범위」로 남지 않아, 그 부분을 근거로 달면 PES 검사에 걸린다. 브리지는 이 컴퓨터의 로그에 남긴다.

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

## 5. 큰 원본 나눠 올리기 (사용자 결정 2026-11-28 — **서버에 더할 것**)

원본(글이면 첫 글의 UTF-8 바이트, 파일이면 바이트)이 **1MB 이하**면 지금처럼 `/mcp/evidence` 한 번에 보낸다. **넘으면** 바이트 그대로 조각으로 나눠 올린 뒤, `/mcp/evidence` 에 파일 번호와 지문만 보낸다.
서버의 큰 파일 올리기(`src/uploads.ts`, R2 멀티파트: 시작 → 조각 PUT → complete → sha256 → `files` 장부)를 다시 쓴다. 다른 점은 인증뿐이다(쿠키 대신 연결 토큰 + 기기 키 서명).

| 차례 | 요청 | 몸통 · 머리글 | 답 |
| --- | --- | --- | --- |
| 1 시작 | `POST /mcp/evidence-upload` (서명, purpose `pes-evidence-upload`) | `r = {slug, name, mime, size, sha256}` | `{id, part_size, parts}` |
| 2 조각 | `PUT /mcp/evidence-upload/:id/part/:n` | 본문 = 조각 바이트. 머리글 `Authorization: Bearer` · `x-pes-ts` · `x-pes-nonce` · `x-pes-part-sha256` · `x-pes-sig`. 서명 글은 `"pes-evidence-part\n<ts>\n<nonce>\n<id>\n<n>\n<조각 sha256>"` | `{n, etag}` |
| 3 마침 | `POST /mcp/evidence-upload/:id/complete` (서명, purpose `pes-evidence-upload-complete`) | `r = {id}` | `{file_id, size, sha256}` — 서버가 이어 붙인 것으로 계산한 지문 |
| 4 근거 | `POST /mcp/evidence` | `r.upload = {file_id, sha256, size, mime, url, at, status}` | `{evidence_id, sha256}` |

브리지가 하는 것:
- 서버가 준 `part_size` 대로 자른다. 조각은 실패하면 한 번 더 보낸다.
- 3의 `sha256` 과 4의 `sha256` 이 자기가 계산한 값과 다르면 그 도구 호출을 실패로 돌려준다. 결과는 AI 에 주지 않는다.

서버가 할 일:
- 1: 서명 · 권한(`mayWriteProject` + 브리지가 맡은 라운드 `workingRound`)을 본다. `size ≤ RAW_MAX(25MB)` 여야 한다. `uploads` 줄을 만든다(`purpose = 'evidence'`, `owner_id` = 토큰 주인, 하루 만료).
  - `part_size` 는 R2 규칙대로 5MB 이상이다(지금 `PART_SIZE` 10MB 를 그대로 써도 된다).
- 2: 그 연결이 만든 올리기만 받는다. 머리글 서명 · 조각 지문(`x-pes-part-sha256` = 본문의 sha256)을 확인한다. 크기 규칙은 `uploads.ts` 와 같다(마지막 말고는 `part_size`).
- 3: `complete` 뒤 `ledgerFromR2` 로 `files` 장부에 넣는다(`tool: 'evidence'`, `use: 'source'` — `/api/files/:id` 로 열리지 않게). 이어 붙인 sha256 이 1의 `sha256` 과 다르면 지우고 실패로 답한다.
- 4: `r.upload.file_id` 가 그 연결이 올리고 아직 근거에 쓰지 않은 `evidence` 파일이어야 한다. 장부의 sha256 이 `r.upload.sha256` 과 같아야 한다.
  - `recordEvidence` 에 원본으로 그 파일을 잇는다(`raw_file_id`, `raw_sha256`, `raw_size`, `raw_mime` — 다시 저장하지 않는다). 덧붙은 `parts`(stderr 등)는 기록 줄의 작은 덧붙임으로 둔다.
  - 답에 `sha256` 을 싣는다.
- 비밀값 지우기: 원본은 바이트 그대로라 서버가 고치지 않는다. 브리지가 조사 CLI 출력에서 이 컴퓨터의 키를 지운 뒤 올린다. 서버의 키 지우기(`userSecrets`)가 필요하면, 글 원본(`text/*` · JSON)만 조각을 받을 때 바꾸지 말고 complete 뒤에 검사해 걸리면 실패로 해야 한다(지문이 바뀌기 때문). 정할 것.

## 남은 것(정할 것)

- 서버의 옛 판 브리지 거부(티켓 24): 연결할 때 보내는 `bridge_ver` 는 `rust-<판>` 이다.
- 5절 큰 글 원본의 서버 쪽 비밀값 검사 방법.
