use crate::review::context::Language;

pub fn extract_dep_candidates(
    source_code: &str,
    language: &Language,
    current_file: &str,
) -> Vec<String> {
    match language {
        Language::Rust => rust_candidates(source_code, current_file),
        Language::TypeScript | Language::Svelte => ts_candidates(source_code, current_file),
        Language::Python => python_candidates(source_code, current_file),
        Language::Go => go_candidates(source_code, current_file),
        Language::Kotlin => kotlin_candidates(source_code),
        Language::Swift => vec![], // 시스템 모듈 위주라 스킵
        Language::Unknown(_) => vec![],
    }
}

// ── Rust ──────────────────────────────────────────────────────────────
// `use crate::auth::verify;` → src/auth/verify.rs, src/auth.rs
// `mod auth;` → src/auth.rs, src/auth/mod.rs
fn rust_candidates(code: &str, current_file: &str) -> Vec<String> {
    let src_root = current_file
        .split('/')
        .next()
        .filter(|&p| p == "src")
        .map(|_| "src/")
        .unwrap_or("src/");

    let mut candidates = Vec::new();

    for line in code.lines() {
        let line = line.trim();

        // use crate::some::module;
        if let Some(rest) = line.strip_prefix("use crate::") {
            let module_path = rest
                .split('{')
                .next()
                .unwrap_or(rest)
                .split(';')
                .next()
                .unwrap_or(rest)
                .trim()
                .replace("::", "/");
            // 마지막 세그먼트 제거 (함수/타입명일 수 있음)
            let parts: Vec<&str> = module_path.split('/').collect();
            if parts.len() >= 2 {
                let dir_path = parts[..parts.len() - 1].join("/");
                candidates.push(format!("{src_root}{dir_path}.rs"));
                candidates.push(format!("{src_root}{dir_path}/mod.rs"));
            }
            candidates.push(format!("{src_root}{module_path}.rs"));
            candidates.push(format!("{src_root}{module_path}/mod.rs"));
        }

        // mod some_module;
        if let Some(rest) = line.strip_prefix("mod ") {
            let module_name = rest.split(';').next().unwrap_or("").trim();
            if !module_name.is_empty() && !module_name.contains('{') {
                let dir = std::path::Path::new(current_file)
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or("");
                if dir.is_empty() {
                    candidates.push(format!("{module_name}.rs"));
                } else {
                    candidates.push(format!("{dir}/{module_name}.rs"));
                    candidates.push(format!("{dir}/{module_name}/mod.rs"));
                }
            }
        }
    }

    candidates.sort();
    candidates.dedup();
    candidates.truncate(8);
    candidates
}

// ── TypeScript / Svelte ───────────────────────────────────────────────
// `import { foo } from './utils'` → utils.ts, utils.tsx, utils/index.ts
fn ts_candidates(code: &str, current_file: &str) -> Vec<String> {
    let current_dir = std::path::Path::new(current_file)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("");

    let mut candidates = Vec::new();

    for line in code.lines() {
        let line = line.trim();
        // import ... from './...' 또는 '../...'
        if !line.starts_with("import") {
            continue;
        }
        let Some(from_pos) = line.find("from '") else {
            continue;
        };
        let rest = &line[from_pos + 6..];
        let Some(end) = rest.find('\'') else {
            continue;
        };
        let import_path = &rest[..end];

        if !import_path.starts_with('.') {
            continue; // 외부 패키지 스킵
        }

        let resolved = resolve_relative(current_dir, import_path);
        candidates.push(format!("{resolved}.ts"));
        candidates.push(format!("{resolved}.tsx"));
        candidates.push(format!("{resolved}/index.ts"));
        candidates.push(format!("{resolved}.svelte"));
    }

    candidates.sort();
    candidates.dedup();
    candidates.truncate(8);
    candidates
}

// ── Python ────────────────────────────────────────────────────────────
// `from .models import User` → same_dir/models.py
// `from ..auth import verify` → parent_dir/auth.py
fn python_candidates(code: &str, current_file: &str) -> Vec<String> {
    let current_dir = std::path::Path::new(current_file)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("");

    let mut candidates = Vec::new();

    for line in code.lines() {
        let line = line.trim();
        if !line.starts_with("from .") {
            continue;
        }

        // from .models import ... → models
        // from ..auth import ...  → ../auth
        let rest = &line["from ".len()..];
        let module_part = rest.split(" import").next().unwrap_or("").trim();

        // 점 개수 = 상위 디렉토리 수
        let dot_count = module_part.chars().take_while(|&c| c == '.').count();
        let module_name = &module_part[dot_count..];

        let mut base = current_dir.to_string();
        for _ in 1..dot_count {
            base = std::path::Path::new(&base)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .to_string();
        }

        if module_name.is_empty() {
            candidates.push(format!("{base}/__init__.py"));
        } else {
            candidates.push(format!("{base}/{module_name}.py"));
            candidates.push(format!("{base}/{module_name}/__init__.py"));
        }
    }

    candidates.sort();
    candidates.dedup();
    candidates.truncate(8);
    candidates
}

// ── Go ────────────────────────────────────────────────────────────────
// `import "github.com/user/repo/internal/auth"` → internal/auth/
// 패키지 경로의 마지막 세그먼트를 디렉토리로 간주
fn go_candidates(code: &str, _current_file: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut in_import_block = false;

    for line in code.lines() {
        let line = line.trim();
        if line == "import (" {
            in_import_block = true;
            continue;
        }
        if line == ")" {
            in_import_block = false;
            continue;
        }

        let import_str = if in_import_block {
            line.trim_matches('"')
        } else if let Some(rest) = line.strip_prefix("import \"") {
            rest.trim_end_matches('"')
        } else {
            continue;
        };

        // 외부 패키지에서 내부 경로 추출 (마지막 2~3 세그먼트)
        let parts: Vec<&str> = import_str.split('/').collect();
        if parts.len() >= 3 {
            // github.com/user/repo/pkg/sub → pkg/sub
            let internal: Vec<&str> = parts[3..].to_vec();
            if !internal.is_empty() {
                candidates.push(format!("{}/", internal.join("/")));
            }
        }
    }

    candidates.sort();
    candidates.dedup();
    candidates.truncate(5);
    candidates
}

// ── Kotlin ────────────────────────────────────────────────────────────
// `import com.example.app.auth.UserRepository` → app/auth/UserRepository.kt
fn kotlin_candidates(code: &str) -> Vec<String> {
    let mut candidates = Vec::new();

    for line in code.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("import ") else {
            continue;
        };
        let fqn = rest
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches(';');
        if fqn.is_empty() {
            continue;
        }

        // com.example.app.auth.UserRepository → app/auth/UserRepository.kt
        let parts: Vec<&str> = fqn.split('.').collect();
        if parts.len() >= 3 {
            // 앞의 2개(com.example) 건너뛰고 나머지를 경로로
            let path = parts[2..].join("/");
            candidates.push(format!("{path}.kt"));
        }
    }

    candidates.sort();
    candidates.dedup();
    candidates.truncate(5);
    candidates
}

fn resolve_relative(base_dir: &str, import_path: &str) -> String {
    if base_dir.is_empty() {
        return import_path.to_string();
    }
    let full = format!("{base_dir}/{import_path}");
    // 단순 경로 정규화: /../ 처리
    let mut parts: Vec<&str> = Vec::new();
    for segment in full.split('/') {
        match segment {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::context::Language;

    #[test]
    fn test_rust_use_crate_candidates() {
        let code = "use crate::auth::verify;\nuse crate::config::Settings;";
        let result = extract_dep_candidates(code, &Language::Rust, "src/main.rs");
        assert!(
            result.iter().any(|p| p.contains("auth")),
            "should contain auth path"
        );
        assert!(
            result.iter().any(|p| p.contains("config")),
            "should contain config path"
        );
    }

    #[test]
    fn test_rust_mod_candidates() {
        let code = "mod auth;\nmod config;";
        let result = extract_dep_candidates(code, &Language::Rust, "src/main.rs");
        assert!(result
            .iter()
            .any(|p| p == "src/auth.rs" || p == "src/auth/mod.rs"));
    }

    #[test]
    fn test_ts_relative_import_candidates() {
        let code = "import { foo } from './utils';\nimport { bar } from '../lib/helper';";
        let result = extract_dep_candidates(code, &Language::TypeScript, "src/app.ts");
        assert!(result.iter().any(|p| p.contains("utils")));
        assert!(result.iter().any(|p| p.contains("helper")));
    }

    #[test]
    fn test_ts_external_package_skipped() {
        let code = "import React from 'react';\nimport { useState } from 'react';";
        let result = extract_dep_candidates(code, &Language::TypeScript, "src/app.ts");
        assert!(result.is_empty(), "external packages should be skipped");
    }

    #[test]
    fn test_python_relative_import_candidates() {
        let code = "from .models import User\nfrom ..auth import verify";
        let result = extract_dep_candidates(code, &Language::Python, "app/views.py");
        assert!(result.iter().any(|p| p.contains("models")));
        assert!(result.iter().any(|p| p.contains("auth")));
    }

    #[test]
    fn test_unknown_language_returns_empty() {
        let code = "some code";
        let result = extract_dep_candidates(code, &Language::Unknown("xml".into()), "file.xml");
        assert!(result.is_empty());
    }
}
