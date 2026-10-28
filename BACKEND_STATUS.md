# BACKEND_STATUS — TASK 003

작성: 2026-10-28, Claude Code. 범위: `backend/`와 이 파일만. `preview/`, `IMPLEMENTATION_STATUS.md`와 다른 문서는 수정하지 않았다.

## 한 줄 요약
UI 독립 Rust 라이브러리 `focus-return-core`의 코드·단위 테스트·README를 작성했다. **이 환경에서는 셸·cargo를 사용하지 않았기 때문에 컴파일과 테스트를 한 번도 실행하지 않았다.** 아래 테스트는 “작성됨”이지 “통과”가 아니다.

## 구현함 (파일 작성 완료)
| 파일 | 내용 |
| --- | --- |
| `backend/Cargo.toml` | 라이브러리 crate, edition 2021, rust-version 1.70. 의존성 `serde`(derive), `serde_json`만 사용 |
| `backend/src/lib.rs` | 모듈·재노출, `EpochMs = i64`, `#![forbid(unsafe_code)]` |
| `backend/src/timer.rs` | 결정적 타이머 상태 기계: idle/running/paused/completed, revision, runId, durationMs, remainingMs, targetEndAt. start/pause/resume/finish_early/reset/note_return/tick, `next_deadline`, 복구 마커 `active_run` |
| `backend/src/model.rs` | `Minutes`(1~120, 기본 30, 프리셋 10/30/45, 5분·3분 상수), `TaskName`(trim, 빈 값 ‘무제’), `NextAction`, `DistractionText`, `Settings`, `TaskDraft`, `Distraction`, `SessionRecord`(`timerRunMs` — 집중 시간이라 부르지 않음), `SessionOutcome`, `ActiveRun`, `AppData`(schemaVersion 1)와 검증, 중복 기록 방지, 중단 세션 복구, 오늘 요약, 최근 작업 |
| `backend/src/storage.rs` | 호출자 주입 디렉터리, 임시 파일+flush+rename 교체, 한 세대 백업, 손상 파일 바이트 보존(`create_new`), 백업 복구, 더 새로운 스키마 보호, 읽기 오류와 손상 구분 |
| `backend/src/error.rs` | 오류 타입. 메시지에 사용자 입력 내용을 넣지 않음 |
| `backend/README.md` | Tauri 2 연결 방법, 시계 변경 정책, 저장 방식·제한, 실행 명령 |
| `backend/.gitignore` | `/target` |

지시대로 구현하지 않은 것: 임의 작업 전환·중요도 정책, 백그라운드 스레드·polling, 시스템 시계 읽기, 외부 전송·로그, Tauri 코드.

## 작성한 단위 테스트 (미실행)
- 타이머(`timer.rs`, 18개): 시작 시 주입 시각 기준 종료 시각, 재시작 후 runId 연속, 일시정지·재개 남은 시간 보존, 잘못된 전이 거부, 지연·불규칙 tick, 실행당 완료 1회, 거부된 명령 중 발견된 완료의 다음 단계 전달, 절전 복귀 후 1회 완료(원래 종료 시각), 초기화 후 이전 실행 명령 무효화·이전 종료 시각 무시, 조기 종료 기록, 완료 후 새 시작, 시계 뒤로 이동, revision 단조 증가, 복귀 횟수, 중단 복구 마커, 휴식 3분, camelCase 직렬화.
- 모델(`model.rs`, 17개): 빈/공백/전각 공백 작업 → ‘무제’, trim, 길이·제어 문자 제한, 분 범위(0/1/120/121, "1.5", "-3", "abc"), 역직렬화 검증, 초안, 딴생각 건너뛰기·ID·완료·삭제, 세션 중복 기록 무시, 불일치 기록 거부, 구조 검증, 재시작 중단 복구(자동 재개 없음), step 적용, 오늘 요약, 최근 작업, 전체 기록 삭제, JSON 왕복.
- 저장(`storage.rs`, 13개): 첫 실행, 왕복, 디렉터리 생성, 기존 파일 교체와 백업, **저장 실패 후 원본 유지·재시도 성공**, 잘못된 데이터 저장 거부, **손상 본 파일 보존 후 백업 사용**, 둘 다 손상 시 기본값, 형식은 맞지만 값이 틀린 JSON, 더 새로운 스키마 미변경, 저장 직전 손상 보존, 보존 사본 비덮어쓰기, 읽기 오류를 손상으로 취급하지 않음.
- 파일 테스트는 OS 임시 폴더 아래 프로세스·카운터별 고유 폴더만 쓰고 종료 시 삭제한다.

## 실행한 검사
- 없음. 셸·설치·git을 사용하지 않는다는 지시에 따라 `cargo`, `rustc`를 호출하지 않았다. 이전 점검에서도 PATH에 없었다(TASK_003 기록).
- 수행한 것은 작성자의 코드 읽기 검토뿐이다(타입·대여 규칙·테스트 기대값 수동 확인). 이것은 컴파일 검증을 대신하지 않는다.

## 미검증 항목
- [ ] `cargo check` — 컴파일 오류가 있을 수 있다.
- [ ] `cargo test` — 위 48개 테스트 모두 미실행. Windows에서의 `rename` 교체, 디렉터리를 이용한 실패 주입 테스트 동작 미확인.
- [ ] `cargo fmt --check` — rustfmt를 돌리지 않아 줄바꿈 차이로 실패할 가능성이 높다. `cargo fmt` 한 번 실행 필요.
- [ ] `cargo clippy` — 미실행.
- [ ] `Cargo.lock` 미생성, 의존성 버전 미해결(serde 1.x, serde_json 1.x).
- [ ] 실제 Tauri 2 연결, 단일 인스턴스, 다른 프로세스가 파일을 잠근 상태의 저장 실패, 실제 절전 복귀, 성능 측정 — 모두 범위 밖·미검증.

## 다음 단계 제안
1. Rust MSVC toolchain이 있는 환경에서 `backend/`에서 `cargo fmt && cargo check && cargo test` 실행 후 결과를 이 파일에 기록.
2. 실패가 있으면 수정 후 재실행. 통과 전까지 ACCEPTANCE의 관련 항목을 체크하지 않는다.
3. 제품 결정 필요: 완료 알림·소리 기본값(현재 둘 다 켜짐, 임시).
