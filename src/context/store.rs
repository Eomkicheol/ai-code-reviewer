use crate::error::{Result, ReviewerError};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ReviewPattern {
    pub file_pattern: String,
    pub category: String,
    pub description: String,
    pub occurrence_count: i64,
}

#[derive(Clone)]
pub struct ContextStore {
    pool: SqlitePool,
}

impl ContextStore {
    pub async fn new(db_path: &str) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{db_path}?mode=rwc"))
            .await
            .map_err(|e| ReviewerError::Config(format!("db 연결 실패: {e}")))?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS review_patterns (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                repo TEXT NOT NULL,
                file_pattern TEXT NOT NULL,
                category TEXT NOT NULL,
                description TEXT NOT NULL,
                occurrence_count INTEGER NOT NULL DEFAULT 1,
                last_seen TEXT NOT NULL,
                UNIQUE(repo, file_pattern, category, description)
            )",
        )
        .execute(&pool)
        .await
        .map_err(|e| ReviewerError::Config(format!("db 마이그레이션 실패: {e}")))?;

        Ok(Self { pool })
    }

    pub async fn get_patterns(&self, repo: &str) -> Result<Vec<ReviewPattern>> {
        let rows = sqlx::query_as::<_, ReviewPattern>(
            "SELECT file_pattern, category, description, occurrence_count
             FROM review_patterns
             WHERE repo = ?
             ORDER BY occurrence_count DESC
             LIMIT 5",
        )
        .bind(repo)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ReviewerError::Config(format!("db 조회 실패: {e}")))?;

        Ok(rows)
    }

    pub async fn record_findings(
        &self,
        repo: &str,
        file_path: &str,
        category: &str,
        description: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        // 파일 경로에서 확장자 기반 패턴 추출
        let file_pattern = std::path::Path::new(file_path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!("*.{e}"))
            .unwrap_or_else(|| file_path.to_string());

        sqlx::query(
            "INSERT INTO review_patterns (repo, file_pattern, category, description, occurrence_count, last_seen)
             VALUES (?, ?, ?, ?, 1, ?)
             ON CONFLICT(repo, file_pattern, category, description)
             DO UPDATE SET occurrence_count = occurrence_count + 1, last_seen = excluded.last_seen",
        )
        .bind(repo)
        .bind(&file_pattern)
        .bind(category)
        .bind(description)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| ReviewerError::Config(format!("db 저장 실패: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_store_and_retrieve_patterns() {
        let store = ContextStore::new(":memory:").await.unwrap();

        store
            .record_findings(
                "owner/repo",
                "src/auth.rs",
                "security",
                "SQL injection risk",
            )
            .await
            .unwrap();
        store
            .record_findings(
                "owner/repo",
                "src/auth.rs",
                "security",
                "SQL injection risk",
            )
            .await
            .unwrap();
        store
            .record_findings("owner/repo", "src/lib.rs", "quality", "함수명 불명확")
            .await
            .unwrap();

        let patterns = store.get_patterns("owner/repo").await.unwrap();
        assert_eq!(patterns.len(), 2);
        assert_eq!(patterns[0].occurrence_count, 2); // 가장 많이 발생한 것이 먼저
    }

    #[tokio::test]
    async fn test_empty_repo_returns_empty_patterns() {
        let store = ContextStore::new(":memory:").await.unwrap();
        let patterns = store.get_patterns("unknown/repo").await.unwrap();
        assert!(patterns.is_empty());
    }

    #[tokio::test]
    async fn test_file_pattern_extracted_from_extension() {
        let store = ContextStore::new(":memory:").await.unwrap();

        store
            .record_findings("owner/repo", "src/main.rs", "quality", "긴 함수")
            .await
            .unwrap();

        let patterns = store.get_patterns("owner/repo").await.unwrap();
        assert_eq!(patterns[0].file_pattern, "*.rs");
    }
}
