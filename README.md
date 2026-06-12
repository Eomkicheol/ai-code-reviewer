# AI Code Reviewer

GitHub PR에 자동으로 보안·품질 리뷰 코멘트를 달아주는 GitHub App입니다.
Claude / OpenAI / Gemini 중 원하는 LLM을 선택해 사용할 수 있습니다.

## 기능

- PR 오픈/업데이트 시 자동 diff 분석
- 라인별 인라인 코멘트 (GitHub PR 리뷰 코멘트)
- 보안 리뷰: OWASP Top 10, SQL injection, 하드코딩 시크릿, 취약한 암호화
- 품질 리뷰: 네이밍 규칙, 함수 복잡도, 코드 중복
- PR 요약 + 머지 전 체크리스트 자동 생성
- `/review` 댓글로 수동 리뷰 트리거
- `@reviewer <질문>` 으로 AI에게 코드 관련 질문 가능
- 파일 간 의존성 분석 (import 추적으로 관련 파일 컨텍스트 포함)
- SQLite 기반 저장소별 반복 패턴 학습
- 지원 언어: Rust, TypeScript, Python, Go, Kotlin, Swift, Svelte
- 저장소별 `.reviewbot.yml`로 리뷰 범위 커스터마이징
- HMAC-SHA256 Webhook 서명 검증 (위조 방지, 타이밍 어택 방지)

## 빠른 시작

### 1. 빌드

```bash
git clone <repo-url>
cd reviewer
cargo build --release
```

### 2. 환경변수 설정

```bash
cp .env.example .env
# .env 파일을 열어 값 입력
```

| 변수 | 필수 | 설명 |
|------|------|------|
| `GITHUB_TOKEN` | 필수 | GitHub Personal Access Token (`pull_requests:write`, `contents:read`) |
| `GITHUB_WEBHOOK_SECRET` | 필수 | GitHub App Webhook 시크릿 |
| `CLAUDE_API_KEY` | claude 사용 시 | Anthropic API 키 |
| `OPENAI_API_KEY` | openai 사용 시 | OpenAI API 키 |
| `GEMINI_API_KEY` | gemini 사용 시 | Google AI Studio API 키 |
| `GITHUB_API_URL` | 선택 | GitHub API 엔드포인트 (기본값: `https://api.github.com`, GitHub Enterprise용) |
| `DB_PATH` | 선택 | SQLite 데이터베이스 경로 (기본값: `reviewer.db`) |

### 3. 서버 실행

```bash
source .env   # 또는 direnv 사용
RUST_LOG=info ./target/release/reviewer
# 기본 포트: 3000
# Webhook 엔드포인트: POST http://localhost:3000/webhook
```

### 4. GitHub App 설정

1. [GitHub Developer Settings](https://github.com/settings/apps) -> **New GitHub App**
2. **Webhook URL**: `https://your-domain.com/webhook`
   - 로컬 테스트: [ngrok](https://ngrok.com) 사용 -> `ngrok http 3000`
3. **Webhook Secret**: `.env`의 `GITHUB_WEBHOOK_SECRET`와 동일하게 입력
4. **Repository permissions**:
   - Pull requests -> **Read & write**
   - Contents -> **Read-only**
5. **Subscribe to events**: `Pull request` + `Issue comment` 체크
6. App 설치 후 원하는 저장소에 적용

### 5. 로컬 테스트 (ngrok)

```bash
# 터미널 1: 서버 실행
RUST_LOG=debug ./target/release/reviewer

# 터미널 2: ngrok으로 외부 노출
ngrok http 3000

# ngrok이 출력한 URL을 GitHub App Webhook URL에 입력
# 예: https://abc123.ngrok.io/webhook
```

## 저장소별 설정 (.reviewbot.yml)

저장소 루트에 `.reviewbot.yml`을 추가하면 리뷰 범위를 커스터마이징할 수 있습니다.
파일이 없으면 기본값(보안+품질 모두 활성, Claude)으로 동작합니다.

```yaml
provider:
  name: claude          # claude | openai | gemini
  model: claude-sonnet-4-6

reviewers:
  security:
    enabled: true
    severity_threshold: warning   # info | warning | critical 이상만 코멘트
  quality:
    enabled: true

ignore:
  paths:
    - "*.test.rs"      # 테스트 파일 제외
    - "migrations/**"  # DB 마이그레이션 제외
  max_file_size_kb: 500
```

## 아키텍처

```
GitHub Webhook (pull_request / issue_comment)
  └→ Axum Handler (HMAC-SHA256 서명 검증)
       └→ tokio::spawn (비동기 파이프라인)
            ├→ ConfigLoader      — .reviewbot.yml 파싱 (없으면 기본값)
            ├→ DiffFetcher       — PR diff + 의존 파일 스니펫 수집
            ├→ ContextStore      — SQLite 과거 패턴 조회/저장
            └→ ReviewEngine
                 ├→ SecurityReviewer → LlmProvider (Claude/OpenAI/Gemini)
                 └→ QualityReviewer  → LlmProvider (Claude/OpenAI/Gemini)
                      ├→ CommentPoster — GitHub PR 인라인 코멘트 게시
                      └→ PrSummary    — 요약 + 체크리스트 PR Review 게시

issue_comment → /review 트리거 → 위 파이프라인 재실행
issue_comment → @reviewer 질문 → AI 답변 PR 댓글 게시
```

## 보안

| 위협 | 대응 |
|------|------|
| Webhook 위조 | HMAC-SHA256 서명 검증 + constant-time 비교 (타이밍 어택 방지) |
| API 키 노출 | 모든 시크릿 환경변수 전용, `.env` `.gitignore` 처리 |
| Prompt Injection | 사용자 코드를 `<code>` 태그로 시스템 프롬프트와 분리 |
| SSRF | GitHub API 호출 시 도메인 화이트리스트 적용 |
| 최소 권한 | GitHub 토큰: PR 쓰기 + 컨텐츠 읽기만 요청 |

## 테스트

```bash
# 전체 테스트 (75개)
cargo test

# 통합 테스트만
cargo test --test integration

# E2E 시뮬레이션 (wiremock 기반)
cargo test --test simulation
```

## 환경변수 참고

`.env.example` 파일 참조.
