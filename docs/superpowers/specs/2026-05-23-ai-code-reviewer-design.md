# AI 코드 리뷰어 시스템 설계

## 개요

GitHub App 형태의 AI 코드 리뷰 시스템. PR이 열리면 자동으로 diff를 분석해 보안 취약점·버그·코드 품질 이슈를 라인별 인라인 코멘트로 게시한다. LLM 프로바이더(Claude/OpenAI/Gemini)는 저장소별 `.reviewbot.yml` 설정으로 선택 가능하다.

## 기술 스택

- **언어**: Rust
- **웹 프레임워크**: Axum
- **HTTP 클라이언트**: reqwest
- **설정 파싱**: serde + serde_yaml
- **에러 처리**: thiserror + anyhow
- **테스트**: cargo test + wiremock (HTTP mock)

## 아키텍처 — 파이프라인 방식

```
GitHub Webhook (PR 이벤트)
  └→ Axum Webhook Handler (HMAC-SHA256 검증)
       └→ DiffFetcher      (GitHub API로 diff 수집)
            └→ ConfigLoader (저장소 .reviewbot.yml 로드)
                 └→ ReviewEngine
                 │    ├→ SecurityReviewer (OWASP + 로직 버그)
                 │    └→ QualityReviewer  (네이밍, 복잡도, 중복)
                 │         └→ LlmProvider trait (Claude | OpenAI | Gemini)
                 └→ CommentPoster (GitHub API 인라인 코멘트)
```

## 핵심 Trait 설계

```rust
trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String>;
    fn model_name(&self) -> &str;
}

trait Reviewer: Send + Sync {
    fn name(&self) -> &str;
    async fn review(&self, ctx: &ReviewContext) -> Result<Vec<ReviewComment>>;
}
```

## 핵심 데이터 모델

```rust
struct ReviewContext {
    repo: RepoInfo,
    file_path: String,
    language: Language,
    diff_hunks: Vec<DiffHunk>,
    config: ReviewConfig,
}

struct DiffHunk {
    start_line: u32,
    lines: Vec<DiffLine>,
}

struct ReviewComment {
    path: String,
    line: u32,
    severity: Severity,   // Critical | Warning | Info
    category: Category,   // Security | Bug | Quality
    body: String,
}
```

## 설정 파일 (.reviewbot.yml)

```yaml
provider:
  name: claude          # claude | openai | gemini
  model: claude-sonnet-4-6

reviewers:
  security:
    enabled: true
    severity_threshold: warning
    owasp_categories:
      - injection
      - auth
      - crypto
      - secrets
  quality:
    enabled: true
    checks:
      - naming
      - complexity
      - duplication

ignore:
  paths:
    - "*.test.rs"
    - "migrations/**"
  max_file_size_kb: 500
```

API 키는 절대 설정 파일에 포함하지 않으며 환경변수로만 주입한다.

## 보안 설계

| 위협 | 대응 |
|---|---|
| Webhook 위조 | HMAC-SHA256 서명 검증, constant-time 비교로 타이밍 어택 방지 |
| 시크릿 노출 | 모든 API 키 환경변수 전용 (`GITHUB_APP_PRIVATE_KEY`, `CLAUDE_API_KEY` 등) |
| Prompt Injection | diff를 시스템 프롬프트와 분리, `<code>` 태그로 격리 |
| SSRF | GitHub API URL 화이트리스트 (`api.github.com`만 허용) |
| Rate Limit 남용 | 커밋 SHA 기반 멱등성 체크 (중복 리뷰 방지) |
| 최소 권한 | GitHub App 권한: `pull_requests: write`, `contents: read`만 요청 |

## 에러 처리

- 도메인별 에러 타입: `WebhookError`, `DiffError`, `LlmError`, `GithubError`
- LLM 호출 실패: 지수 백오프 재시도 (최대 3회)
- 부분 실패 허용: 파일 하나 실패해도 나머지 계속 진행

## 테스트 전략 (TDD)

- **단위 테스트**: DiffParser, ConfigLoader, SecurityReviewer, HMAC 검증
- **통합 테스트**: Mock LlmProvider + wiremock으로 전체 파이프라인 E2E
- **보안 테스트**: Prompt injection 시도, 위조 Webhook 서명 거부

## 프로젝트 구조

```
reviewer/
├── src/
│   ├── main.rs
│   ├── webhook/        # Axum 핸들러, HMAC 검증
│   ├── diff/           # GitHub diff 파싱
│   ├── config/         # .reviewbot.yml 로드
│   ├── review/         # ReviewEngine, Reviewer trait
│   │   ├── security.rs
│   │   └── quality.rs
│   ├── llm/            # LlmProvider trait + 구현체
│   │   ├── claude.rs
│   │   ├── openai.rs
│   │   └── gemini.rs
│   └── github/         # GitHub API 클라이언트
├── tests/
│   ├── fixtures/       # 테스트용 diff 픽스처
│   └── integration/
├── .reviewbot.yml      # 예시 설정
└── README.md
```
