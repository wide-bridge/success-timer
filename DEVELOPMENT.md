# 개발 설계와 환경

현재 단계: 외부 의존성 없는 preview/index.html 화면 시안. 아래 환경 준비와 아키텍처는 후속 Tauri 단계에 적용한다. 시안 제작에는 Node 업그레이드나 Rust 설치가 필요 없다.

## 현재 확인 사항
2026-10-28 셸 기준 Node 20.10.0, npm 10.2.3. 원본 Vite는 Node ^20.19.0 또는 >=22.12.0을 요구하므로 현 Node는 부족하다. 이 프로젝트는 Node 24 기준으로 준비한다.
rustc와 cargo는 PATH에서 발견되지 않았다. claude도 PATH 및 사용자 .local/bin·AppData/npm 경로에서 발견되지 않았다. 다른 환경/IDE에서 설치됐을 가능성은 남아 있다. C++ 개발 도구 및 WebView2 설치 여부는 아직 검증하지 않았다.
현재는 기획·인수인계 문서만 준비한 상태다. 앱 실행, 의존성 설치, Rust 컴파일 및 설치 프로그램 생성은 완료되지 않았다.

## 환경 준비 순서
1. Node 24와 npm 확인.
2. Rust MSVC toolchain, Microsoft C++ Build Tools의 Desktop development with C++, WebView2 확인.
3. Claude Code를 사용하는 환경에서 이 폴더를 열고 START_HERE.md의 요청을 전달.
4. Tauri 2 + React TS + Vite 프로젝트를 현재 폴더의 문서를 보존하며 생성. 의존성 버전을 확인하고 lockfile 커밋 대상에 포함.
5. npm scripts: dev, build, typecheck, test, tauri를 제공. Windows NSIS 설치 프로그램을 첫 배포 대상으로 한다.

공식 근거: https://v2.tauri.app/start/prerequisites/ , https://v2.tauri.app/start/frontend/vite/ , https://v2.tauri.app/plugin/notification/ , https://v2.tauri.app/distribute/windows-installer/
Windows 알림은 설치본에서 확인해야 한다. 오프라인 사용과 최초 설치의 오프라인 가능 여부는 별개이며 WebView2 포함 방식에 따라 설치 용량이 달라진다.

## 아키텍처
UI → 타입이 있는 TimerPort 어댑터 → Tauri invoke/event → Rust TimerService 및 로컬 저장.
상태: idle / running / paused / completed. UI 복귀 패널은 타이머 상태와 독립. 휴식은 kind=break인 별도 세션.
Snapshot: revision, runId, status, kind, task, nextAction, durationMs, remainingMs, targetEndAt, updatedAt.
명령: get_state, start, pause, resume, reset, finish_early, capture_distraction, return_to_task, update_settings.
모든 응답·이벤트는 revision 비교 후 반영. 복귀 이벤트와 완료 이벤트 중복을 runId 및 이벤트 ID로 방지한다.

## 미니 창 이동

집중 중 `미니 보기`는 Windows 화면 오른쪽 아래의 작은 Tauri 창으로 전환한다. 미니 패널에는 `data-tauri-drag-region`을 사용해 목표·시간·진행률 영역을 마우스로 끌어 창 위치를 옮길 수 있게 한다. 일시정지와 기본 보기 버튼은 드래그 영역 안에서도 클릭 동작을 유지한다.

## 타이머 정책
- 종료 시각 기준 계산. 절전 중 지난 시간은 경과에 포함하고 복귀 후 만료를 한 번 처리한다. 절전 상태에서 알림을 울린다고 약속하지 않는다.
- 일시정지는 남은 시간을 보존한다. 시스템 시각 변경을 감지할 경우 타이머 정책을 명시하고 테스트한다.
- 창 숨김은 타이머를 멈추지 않는다. 트레이를 사용할 수 없으면 닫기로 숨기지 않고 안전한 종료 흐름을 제공한다.
- 프로세스 종료 후 자동 재개하지 않는다. 실행 중 세션 메타데이터를 저장해 재시작 시 중단 사실과 재시작 선택을 표시한다.
- 화면은 최대 초당 1회 상태 동기화로 시작한다. 숨김 시 UI 이벤트를 줄이되 완료 처리는 Rust에서 계속한다. 유휴 상태 무의미한 polling 금지.

## 저장·권한
앱 데이터 디렉터리의 버전 있는 JSON 또는 검증된 Store 방식. 임시 파일 교체·저장 직렬화·실패 후 재시도 지원. 손상된 데이터는 백업하고 사용자에게 알린다.
Distraction: id, sessionId, text, capturedAt, resolvedAt. Session: id, task, kind, startedAt, endedAt, outcome, elapsedMs, returnCount.
입력 길이 제한, 스키마 검증, 최소 Tauri capabilities와 CSP 적용. 기록을 콘솔·분석 서비스로 전송하지 않는다.

## 원본에서 다시 만들지 않을 문제
- 설정 저장 Promise가 한번 reject되면 이후 저장도 막히는 구조.
- 최초 로딩 실패 및 미니 모드에서 오류 메시지를 숨기는 구조.
- 일부 명령 응답만 revision 검사를 생략하는 구조.
- 닫으면 숨기지만 복원할 트레이가 없는 구조.
- 고정 높이와 overflow:hidden으로 작은 화면에서 설정이 잘리는 구조.
