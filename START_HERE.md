# Claude Code에 전달할 요청

최신 우선 지시: TASK_002_SCREEN_FIRST.md. 전체 시안 완료를 기다리지 말고 첫 기본 화면 파일부터 저장·공개한다.

현재 단계는 1단계 화면 시안이다. Tauri MVP는 다음 단계의 최종 목표이며 지금 구현할 범위가 아니다. 아래 내용이 이전의 전체 MVP 착수 지시를 대체한다.

```text
D:\vibe_coding\ai_fit\day09_pc_app\focus-return 폴더에서 Windows용 Tauri 2 앱 ‘Success Timer’을 구현해 줘.
먼저 상위 AGENTS.md와 현재 폴더 CLAUDE.md, PRODUCT.md, DEVELOPMENT.md, ACCEPTANCE.md를 읽어.
사용자는 시작한 뒤 다른 일로 자주 새기 때문에, 작업 한 줄을 계속 보여주고 딴생각을 맡긴 뒤 쉽게 복귀하는 흐름이 핵심이야.
지금은 preview/index.html에 외부 의존성 없는 HTML/CSS/JavaScript 화면 시안을 먼저 구현해 줘.
작업 한 줄(선택, 비우면 ‘무제’), 10·30·45분·직접 선택(기본 30분), 시작·일시정지, 딴생각 맡기기, 원래 작업 복귀, 미니 화면 전환을 조작 가능하게 만들어 줘.
화면에 ‘화면 미리보기 — 실제 데스크톱 타이머 아님’을 표시해. 시안 타이머는 브라우저에서 동작해도 된다. Rust 원본 상태 요구는 다음 Tauri 단계에 적용돼.
이번 단계는 도구 설치·Git 작업 없이 프로젝트 내부 파일만 작성해. Tauri 구현은 화면 검토 후 별도 지시로 진행해.
기존 nextTick 폴더는 수정하지 마. GitHub 생성·push·배포는 하지 마.
완료 후 변경 파일, 실제 검증, 미검증 항목을 IMPLEMENTATION_STATUS.md에 기록해 줘. 아직 구현되지 않은 네이티브 기능을 명시해.
```

기획과 검토는 Codex, 구현은 Claude Code 담당이다. 같은 폴더에서 두 구현 세션을 동시에 실행하지 않는다. Codex가 실행한 CLI 작업과 VS Code 대화는 별도 세션이므로 자동으로 대화가 공유된다고 가정하지 않는다.

