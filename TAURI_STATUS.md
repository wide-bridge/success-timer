# Tauri 2 연결 상태

## 준비한 것

- `src-tauri/`에 Tauri 2 Rust 앱 골격을 추가했다.
- 제품명과 Windows NSIS 현재 사용자 설치 설정을 `src-tauri/tauri.conf.json`에 넣었다.
- 기존 `preview/`를 프런트엔드 배포 디렉터리로 연결했다.
- `focus-return-core`를 경로 의존성으로 연결했다.
- 최소 capability와 `app_name` 경계 명령을 추가했다.

## 현재 검증 결과

- Rust MSVC toolchain을 설치했고 `rustc 1.99.1`, `cargo 1.99.1`을 확인했다.
- `cargo test --manifest-path backend/Cargo.toml`: 48개 테스트 모두 통과했다.
- `npm run tauri:dev`: Windows 개발 실행 파일이 빌드되고 실행됐다.
- `npm run tauri:build`: NSIS 설치 프로그램을 생성했다.
- 기준 버전: **Success Timer 0.1.4**
- 설치 파일: `src-tauri/target/release/bundle/nsis/Success Timer_0.1.4_x64-setup.exe`

## 아직 연결하지 않은 것

- 타이머·저장 명령은 아직 Tauri IPC에 연결하지 않았다. 현재 화면은 preview의 localStorage 어댑터로 동작한다.
- 트레이·알림·완료 소리는 아직 연결하지 않았다.

## 다음 구현 순서

1. `focus_return_core`의 Timer/Store를 `Mutex<AppState>`에 연결한다.
2. `get_state`, `start`, `pause`, `resume`, `reset`, `capture_eureka`, `finish` IPC를 추가한다.
3. preview의 localStorage adapter를 Tauri invoke/event adapter로 교체한다.
4. 트레이·알림·완료 소리와 절전 복귀를 연결하고 Windows 설치본을 다시 검증한다.
