use crate::{
    error::{Result, ReviewerError},
    review::context::{DiffHunk, DiffLine, DiffLineKind},
};

pub fn parse_diff(diff: &str) -> Result<Vec<DiffHunk>> {
    let mut hunks = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;
    let mut current_line: u32 = 0;

    for raw_line in diff.lines() {
        if raw_line.starts_with("@@") {
            // 이전 헝크 저장
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }
            // "@@ -old_start,old_count +new_start,new_count @@" 파싱
            let start_line = parse_hunk_header(raw_line).ok_or_else(|| {
                ReviewerError::DiffParse(format!("invalid hunk header: {raw_line}"))
            })?;
            current_line = start_line;
            current_hunk = Some(DiffHunk {
                start_line,
                lines: Vec::new(),
            });
        } else if let Some(ref mut hunk) = current_hunk {
            let (kind, content) = if let Some(stripped) = raw_line.strip_prefix('+') {
                (DiffLineKind::Added, stripped.to_string())
            } else if let Some(stripped) = raw_line.strip_prefix('-') {
                (DiffLineKind::Removed, stripped.to_string())
            } else {
                (
                    DiffLineKind::Context,
                    raw_line.get(1..).unwrap_or(raw_line).to_string(),
                )
            };
            // Removed 라인은 새 파일에 존재하지 않으므로 번호를 증가시키지 않는다.
            // Added/Context 라인만 새 파일의 줄 번호를 점유한다.
            let advances = !matches!(kind, DiffLineKind::Removed);
            hunk.lines.push(DiffLine {
                number: current_line,
                kind,
                content,
            });
            if advances {
                current_line += 1;
            }
        }
    }

    if let Some(hunk) = current_hunk {
        hunks.push(hunk);
    }
    Ok(hunks)
}

fn parse_hunk_header(header: &str) -> Option<u32> {
    // "@@ -10,6 +10,4 @@" → 새 파일 시작 라인(10) 추출
    let plus_part = header.split('+').nth(1)?;
    let num_str = plus_part.split(',').next()?.split(' ').next()?;
    num_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::context::DiffLineKind;

    #[test]
    fn test_parse_basic_diff() {
        let diff = "\
@@ -10,6 +10,10 @@ fn authenticate(user: &str) -> bool {
 fn authenticate(user: &str) -> bool {
+    let token = format!(\"{}\", email);
-    hash == password
 }";
        let hunks = parse_diff(diff).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].start_line, 10);
        let added: Vec<_> = hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Added)
            .collect();
        assert_eq!(added.len(), 1);
        assert!(added[0].content.contains("token"));
    }

    #[test]
    fn test_parse_multiple_hunks() {
        let diff = "\
@@ -1,3 +1,4 @@
 line1
+added1
 line2
@@ -10,3 +11,4 @@
 line10
+added2
 line11";
        let hunks = parse_diff(diff).unwrap();
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].start_line, 1);
        assert_eq!(hunks[1].start_line, 11);
    }

    #[test]
    fn test_empty_diff_returns_empty() {
        let hunks = parse_diff("").unwrap();
        assert!(hunks.is_empty());
    }

    #[test]
    fn test_removed_line_does_not_advance_counter() {
        // @@ +1,2 @@ → 새 파일에는 2줄(context + context), removed는 없음
        let diff = "\
@@ -1,3 +1,2 @@
 context_before
-removed_line
 context_after";
        let hunks = parse_diff(diff).unwrap();
        let lines = &hunks[0].lines;

        let after = lines.iter().find(|l| l.content == "context_after").unwrap();
        // removed_line이 카운터를 소모했다면 after.number == 3이 됨 (버그)
        assert_eq!(
            after.number, 2,
            "removed line must not advance the new-file line counter"
        );
    }

    #[test]
    fn test_added_and_context_lines_get_correct_numbers() {
        let diff = "\
@@ -5,4 +5,5 @@
 line5
+new_line
 line6
-old_line
 line7";
        let hunks = parse_diff(diff).unwrap();
        let lines = &hunks[0].lines;

        let line5 = lines.iter().find(|l| l.content == "line5").unwrap();
        let new_line = lines.iter().find(|l| l.content == "new_line").unwrap();
        let line6 = lines.iter().find(|l| l.content == "line6").unwrap();
        let line7 = lines.iter().find(|l| l.content == "line7").unwrap();

        assert_eq!(line5.number, 5);
        assert_eq!(new_line.number, 6);
        assert_eq!(line6.number, 7);
        // old_line은 카운터를 소모하지 않으므로 line7은 8이 됨
        assert_eq!(line7.number, 8);
    }
}
