# Claude Code 개발 지침

## 현재 작업 우선순위
현재는 START_HERE.md의 1단계 화면 시안을 구현한다. PRODUCT.md와 DEVELOPMENT.md의 Tauri/Rust 구성은 다음 단계 목표다. 파일 작업만 허용하는 제한은 현재 시안 단계에만 적용한다. 최종 앱 요구 때문에 화면 시안을 중단하거나 환경 설치를 먼저 요구하지 않는다.

이 폴더는 ‘목표, 집중’의 독립 프로젝트다. 먼저 PRODUCT.md, DEVELOPMENT.md, ACCEPTANCE.md를 읽는다. 사용자는 기획을 Codex에, 구현을 Claude Code에 맡겼다. 현재 단계는 개발 준비이며 실제 구현 여부는 파일과 검증 결과로 판단한다.

- 작업 경계: D:\vibe_coding\ai_fit\day09_pc_app\focus-return. 상위 AGENTS.md 지침 준수.
- ../nextTick은 참조용 원본이다. 수정하거나 .git·호스팅 설정을 복제하지 않는다. 원본 이미지·브랜딩·코드를 무단으로 그대로 재배포하지 않는다.
- 상위 폴더에 Git 저장소를 만들지 않는다. GitHub 생성·push·배포는 요청 범위에 없다.
- 기술: Windows, Tauri 2, React, TypeScript, Vite, Rust. Electron 및 Node sidecar를 런타임에 추가하지 않는다.
- 먼저 환경 점검 후 가능한 로컬 구현을 진행한다. 전역 도구 설치·로그인 필요 시 정확한 상황을 보고하고 자격 증명을 요구하거나 기록하지 않는다.
- 타이머 원본 상태는 Rust가 관리한다. 프런트엔드 타이머를 실제 데스크톱 타이머로 사용하지 않는다.
- 브라우저 미리보기 어댑터는 별도로 만들 수 있으나 미리보기임을 표시한다. 실제 Tauri 실패를 조용히 브라우저 모드로 대체하지 않는다.
- 구현 완료를 주장하기 전에 실행한 검사와 미실행 항목을 구분한다. 로컬 웹 화면 확인을 Windows 설치본 검증으로 표현하지 않는다.
- 커밋 요청을 받으면 사용자 KH Cha <253035002+wide-bridge@users.noreply.github.com>를 주 작성자로 유지하고 실제 기여자만 표시한다. 공개 이력 재작성·강제 push 금지.
