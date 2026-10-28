# focus-return-core

‘목표, 집중’의 UI 독립 Rust 라이브러리. 타이머 상태 기계, 검증된 데이터 모델, 복구 가능한 로컬 저장을 제공한다. Tauri·UI·시스템 시계·스레드에 의존하지 않는다. 의존성은 `serde`, `serde_json` 두 개뿐이며 둘 다 Tauri 2가 이미 사용한다.

> 상태: 코드와 테스트는 작성됐지만 이 환경에 cargo/rustc가 없어 **컴파일·테스트를 한 번도 실행하지 않았다.** 자세한 내용은 `../BACKEND_STATUS.md`.

## 구성
| 모듈 | 내용 |
| --- | --- |
| `timer` | `Timer`: idle/running/paused/completed, `revision`, `runId`, `durationMs`, `remainingMs`, `targetEndAt`. 명령: `start`, `pause`, `resume`, `finish_early`, `reset`, `note_return`, `tick` |
| `model` | `Minutes`(1~120 정수, 기본 30, 프리셋 10/30/45), `TaskName`(trim, 비면 ‘무제’), `NextAction`, `DistractionText`, `Settings`, `TaskDraft`, `Distraction`, `SessionRecord`, `ActiveRun`, `AppData`(schemaVersion 1) |
| `storage` | `Store`: 호출자가 준 디렉터리에 JSON 저장·불러오기, 백업, 손상 파일 보존 |
| `error` | `ValidationError`, `TimerError`, `StorageError` — 메시지에 사용자 입력 내용을 넣지 않음 |

## 실행 명령
Rust MSVC toolchain 설치 후 `backend/`에서:
```
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets   # 선택
```
`Cargo.lock`은 아직 없다. 첫 빌드 때 생성되며 앱 저장소에 함께 커밋한다. 테스트는 OS 임시 폴더 아래 `focus-return-core-test-<pid>-<n>-<name>` 폴더만 만들고 끝나면 지운다.

## 타이머 계약
- 모든 함수가 `now: EpochMs`(Unix epoch ms, 벽시계)를 인자로 받는다. 코어는 시계를 읽지 않고, 스레드·polling을 만들지 않는다.
- 남은 시간은 언제나 `targetEndAt - now`로 계산한다. tick이 늦거나 불규칙해도 종료 시각은 그대로다.
- 모든 호출(거부된 명령 포함)마다 `revision`이 증가한다. UI는 마지막으로 반영한 값보다 큰 `revision`만 적용한다. 프로세스가 재시작되면 revision은 0부터 다시 시작한다(UI도 함께 새로 뜬다).
- `pause`/`resume`/`finish_early`/`reset`/`note_return`은 `RunId`를 받는다. 초기화나 새 시작 뒤에 도착한 이전 실행 대상 명령은 `TimerError::StaleRun`으로 거부된다. 초기화된 실행의 종료 시각이 지나도 아무것도 완료되지 않는다.
- 끝난 실행은 `Step.ended`로 **실행당 정확히 한 번** 반환된다(완료/조기 종료/초기화). 명령이 거부되던 순간 만료가 발견되면, 그 완료는 다음 성공 호출(보통 `tick`)의 `Step`에 실린다.
- `next_deadline()`이 `None`이면 저절로 바뀔 것이 없으므로 깨울 필요가 없다.
- 결과: `Completed`만 완료 세션으로 센다. `EndedEarly`(조기 종료·‘5분 새로 시작’ 전), `Reset`, `Interrupted`(재시작 복구)는 세지 않는다.
- `timerRunMs`는 일시정지를 뺀 **타이머 실행 시간**이다. 실제 집중 시간으로 표시하지 않는다.

### 시계 변경 정책
- **앞으로 이동**(절전·최대 절전 복귀, 시각 앞당김): 경과 시간으로 인정한다. 절전과 시각 변경을 구분할 수 없기 때문이다. 이미 지난 종료는 복귀 후 첫 관측에서 한 번만 처리되며 `endedAt`은 원래 종료 시각이다. 절전 중 알림은 약속하지 않는다.
- **뒤로 이동**(실행 중): 이전 관측 이후 뒤로 간 만큼 `targetEndAt`도 당겨 남은 시간이 늘지 않게 한다. 이후 다시 앞으로 고쳐지면 그 이동은 경과로 인정된다(위 규칙).
- 일시정지 중에는 시계 변경이 남은 시간에 영향을 주지 않는다.
- 기록의 `endedAt`은 `startedAt`보다 앞서지 않도록 보정한다.

## 향후 Tauri 2 연결 방법
`src-tauri/Cargo.toml`에 경로 의존성으로 추가한다.
```toml
focus-return-core = { path = "../backend" }
```
권장 구조(이 crate 밖, Tauri 쪽 코드):
1. `setup`에서 `app.path().app_data_dir()`로 `Store::new(dir)` 생성 → `store.load(now)`.
   - `Err` → 기본·미니 창 모두에 오류와 ‘다시 시도’를 표시한다. 조용히 기본값으로 저장하지 않는다(`NewerSchema`일 때 특히).
   - `report.needs_user_notice()` → 백업 복구/초기화 사실을 알린다.
2. `data.recover_interrupted_run()` → `Some(record)`이면 “이전 세션이 중단됐어요” 안내와 다시 시작 선택지. 자동 재개 금지.
3. `Timer::new(data.last_run_id)`; `Mutex<(Timer, AppData)>`를 `State`로 관리.
4. 명령 `get_state`, `start`, `pause`, `resume`, `reset`, `finish_early`, `capture_distraction`, `return_to_task`, `update_settings`는 `SystemTime::now()`를 ms로 바꿔 넘기고, 받은 `Step`을 `data.apply_timer_step(&timer, &step)`에 적용한 뒤 `store.save`하고 `snapshot`을 반환·emit 한다.
5. 저장은 한 곳(전용 작업 또는 mutex 안)에서 직렬화한다. 실패하면 오류를 UI에 보내고, 다음 변경이나 ‘다시 시도’에서 그대로 `save`를 다시 호출하면 된다(실패 상태가 남지 않음).
6. 완료 처리: 실행 중일 때만 `next_deadline()`까지 한 번 잠드는 작업(tokio sleep 등)을 두고 깨어나면 `tick`. 절전 복귀·창 표시 이벤트에서도 `tick`. UI 갱신은 최대 초당 1회, 창이 숨겨지면 줄이되 완료 처리는 계속한다. idle에서는 아무 작업도 돌리지 않는다.
7. `Step.ended`가 `Completed`일 때만 완료 알림·소리(설정에 따라).

`StartRequest`, `TimerSnapshot`, `Step`, 모델 타입은 `serde` camelCase로 직렬화되므로 그대로 invoke 응답에 쓸 수 있다.

## 저장 방식
디렉터리(호출자 주입) 안의 파일:
- `focus-return.json` — 현재 데이터(`schemaVersion: 1`, 사람이 읽을 수 있는 JSON).
- `focus-return.json.bak` — 직전에 유효했던 데이터(한 세대 전).
- `focus-return.json.tmp`, `focus-return.json.bak.tmp` — 원자적 교체용 임시 파일.
- `focus-return.{primary|backup}.corrupt-<ms>[-n].json` — 손상 파일의 바이트 그대로의 사본.

저장 순서: 데이터 검증 → 현재 파일을 읽어 유효하면 백업으로 원자적 복사(실패해도 저장은 계속, `SaveReport.backup_error`로 보고) → 임시 파일 작성·`sync_all`·닫기 → `rename`으로 교체. 대상 파일을 먼저 지우거나 자르지 않는다. Windows의 `std::fs::rename`은 기존 파일을 교체한다. 다른 프로세스(백신·동기화 도구 등)가 대상 파일을 공유 삭제 없이 열고 있으면 교체가 실패하고, 원본은 그대로 남으며 오류가 반환된다. 다음 저장이 그대로 재시도한다.

불러오기: 본 파일 유효 → 사용. 손상(JSON 아님, 버전 없음/구버전, 스키마 불일치, 검증 실패) → 사본 보존(`create_new`로 덮어쓰지 않음) 후 본 파일 제거 → 백업 시도 → 그것도 없거나 손상이면 기본값. 읽기 자체가 실패(잠김·권한)하면 손상으로 보지 않고 아무것도 바꾸지 않은 채 오류를 반환한다. 더 새로운 `schemaVersion`이면 불러오기·저장 모두 거부하고 파일을 건드리지 않는다.

입력 제한: 할 일·다음 행동 200자, 딴생각 500자, 한 줄(제어 문자 금지, 자르지 않고 오류), 세션 100,000개, 딴생각 10,000개.

### 제한 사항
- 한 프로세스, 한 `Store`에서 저장을 직렬화한다는 전제다. 파일 잠금이나 다중 인스턴스 동시 쓰기 조정은 없다(Tauri 단일 인스턴스 플러그인으로 막는 것을 권장).
- Windows에서는 디렉터리 flush를 하지 않는다(std에 이식 가능한 방법이 없음). 전원 차단 직후의 rename 내구성은 파일 시스템에 맡긴다. 임시 파일 자체는 `sync_all` 한다.
- 크래시로 남은 `.tmp`는 복구 원본으로 쓰지 않는다(완전한지 알 수 없음). 다음 저장이 덮어쓴다.
- 손상 사본은 자동으로 지우지 않는다. 정리 UI는 후속 과제다.
- 같은 스키마 버전 안의 알 수 없는 JSON 필드는 무시되고 다음 저장에서 사라진다. 필드를 추가할 때는 `SCHEMA_VERSION`을 올리고 마이그레이션을 `storage::classify`에 추가한다.
- 저장 파일은 암호화하지 않는다(로컬 전용, 외부 전송 없음).
- `Settings`의 완료 알림·소리 기본값(둘 다 켜짐)은 임시값이며 제품 결정이 필요하다.
