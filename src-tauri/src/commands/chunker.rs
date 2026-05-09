/// Markdown-aware chunker. Splits a markdown document on heading-path × paragraph
/// boundaries, then sub-splits chunks longer than ~800 chars on sentence
/// punctuation so individual chunks fit comfortably in retrieval/embedding.

#[derive(Debug, Clone)]
pub struct Chunk {
    pub heading_path: Option<String>,
    pub content: String,
    pub token_count: usize,
}

const MAX_CHARS: usize = 800;
const TARGET_CHARS: usize = 400;

fn join_heading(h1: &Option<String>, h2: &Option<String>, h3: &Option<String>) -> Option<String> {
    let parts: Vec<&str> = [h1, h2, h3].iter().filter_map(|h| h.as_deref()).collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" / "))
    }
}

fn token_count(content: &str) -> usize {
    content.chars().filter(|c| !c.is_whitespace()).count() / 2 + 1
}

fn split_long(content: &str) -> Vec<String> {
    let chars: Vec<char> = content.chars().collect();
    if chars.len() <= MAX_CHARS {
        return vec![content.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut buf_len = 0usize;
    for c in chars {
        buf.push(c);
        buf_len += 1;
        let is_boundary = matches!(c, '。' | '.' | '！' | '!' | '？' | '?');
        if is_boundary && buf_len >= TARGET_CHARS {
            out.push(buf.trim().to_string());
            buf.clear();
            buf_len = 0;
        }
    }
    if !buf.trim().is_empty() {
        out.push(buf.trim().to_string());
    }
    if out.is_empty() {
        out.push(content.to_string());
    }
    out
}

fn flush(chunks: &mut Vec<Chunk>, heading: &Option<String>, paragraph: &mut String) {
    let text = paragraph.trim().to_string();
    paragraph.clear();
    if text.is_empty() {
        return;
    }
    for piece in split_long(&text) {
        if piece.trim().is_empty() {
            continue;
        }
        let tc = token_count(&piece);
        chunks.push(Chunk {
            heading_path: heading.clone(),
            content: piece,
            token_count: tc,
        });
    }
}

pub fn chunk_markdown(md: &str) -> Vec<Chunk> {
    let mut h1: Option<String> = None;
    let mut h2: Option<String> = None;
    let mut h3: Option<String> = None;

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut paragraph = String::new();

    for raw_line in md.lines() {
        let line = raw_line.trim_end();

        if let Some(rest) = line.strip_prefix("### ") {
            flush(&mut chunks, &join_heading(&h1, &h2, &h3), &mut paragraph);
            h3 = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            flush(&mut chunks, &join_heading(&h1, &h2, &h3), &mut paragraph);
            h2 = Some(rest.trim().to_string());
            h3 = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            flush(&mut chunks, &join_heading(&h1, &h2, &h3), &mut paragraph);
            h1 = Some(rest.trim().to_string());
            h2 = None;
            h3 = None;
            continue;
        }

        if line.trim().is_empty() {
            flush(&mut chunks, &join_heading(&h1, &h2, &h3), &mut paragraph);
            continue;
        }

        if !paragraph.is_empty() {
            paragraph.push('\n');
        }
        paragraph.push_str(line);
    }

    flush(&mut chunks, &join_heading(&h1, &h2, &h3), &mut paragraph);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_no_chunks() {
        let chunks = chunk_markdown("");
        assert!(chunks.is_empty());
    }

    #[test]
    fn paragraphs_under_one_heading() {
        let md = "# Game\n\nFirst paragraph.\n\nSecond paragraph.\n";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading_path.as_deref(), Some("Game"));
        assert_eq!(chunks[1].heading_path.as_deref(), Some("Game"));
        assert_eq!(chunks[0].content, "First paragraph.");
        assert_eq!(chunks[1].content, "Second paragraph.");
    }

    #[test]
    fn nested_heading_path() {
        let md = "# A\n\n## B\n\n### C\n\nbody text\n";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading_path.as_deref(), Some("A / B / C"));
    }

    #[test]
    fn heading_resets_deeper_levels() {
        let md = "# A\n\n## B\n\n### C\n\nfirst\n\n## D\n\nsecond\n";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading_path.as_deref(), Some("A / B / C"));
        assert_eq!(chunks[1].heading_path.as_deref(), Some("A / D"));
    }

    #[test]
    fn long_paragraph_is_split() {
        let sentence = "这是一个测试句子。".repeat(120); // > 800 chars
        let md = format!("# Heading\n\n{}\n", sentence);
        let chunks = chunk_markdown(&md);
        assert!(chunks.len() > 1, "expected long paragraph to split");
        for c in &chunks {
            assert!(c.content.chars().count() <= MAX_CHARS + 50);
        }
    }

    #[test]
    fn token_count_is_positive() {
        let chunks = chunk_markdown("# H\n\nhello world\n");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].token_count >= 1);
    }
}
