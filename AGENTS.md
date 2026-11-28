# pes-bridge 에이전트 지시

## Rust 작성 규칙

`bridge/`(pes-bridge) · `pes-bridge-update` 등 Rust 코드를 쓸 때 지킨다.

- `cargo check`와 `cargo clippy` 경고가 0개가 될 때까지 고친다(`cargo clippy --all-targets`).
- `unwrap`, `expect`, `panic!`을 쓰지 않는다. 실패하면 이유를 알리고 다음 작업으로 넘어간다. 크레이트 lints 가 `unwrap_used` · `expect_used` · `panic` 을 막는다.
- 넘어가면 안 되는 실패가 있다. 그때는 그 작업을 중단하고, 프로그램은 계속 다음 일을 받는다.
  - 업데이트 서명 확인이 실패하면 설치하지 않는다.
  - 권한 확인이나 근거 기록 저장이 실패하면 성공으로 처리하지 않는다.
- `unsafe`를 쓰지 않는다(`unsafe_code = "forbid"`). 꼭 필요하면 이유를 적어 목록으로 보여 준다.
- `clone`으로 오류를 피하지 않고 빌림으로 고친다. 빌림으로 풀 수 없는 `clone` 도 있다(여러 작업이 함께 쓰는 값 등). 그런 것은 바로 위에 `// clone: 이유` 를 적는다.
- 다 되면 `cargo test`까지 실행하고 결과를 보여 준다.

## 시험

- `cargo test` 는 실제 실행 파일을 띄운다. 가짜 PES(`bridge/tests/round.rs`)와 가짜 AI(`bridge/tests/fake/fake-ai.mjs`, Node)로 라운드 한 바퀴를 돈다.
- 실제 AI · 기관 API 는 부르지 않는다. 실제 AI 로 시험하려면 사용자 승인을 받는다.
