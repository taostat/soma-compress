//! Rust port of the SOMA OpenClaw compressor (https://github.com/DendriteHQ/SOMA-OpenClaw-compressor, MIT).

use std::collections::BTreeMap;

const KEEP_FRACTION: f64 = 0.60;
const MIN_PASSTHROUGH_CHARS: usize = 16_000;
const MAX_KEEP_CHARS: usize = 32_000;
const CMP_START: &str = "[[CMP]]";
const CMP_END: &str = "[[/CMP]]";

const ERROR_MARKERS: &[&str] = &[
    "traceback (most recent call last)",
    "assertionerror",
    "error:",
    "exception:",
    "failed",
    "failures=",
    "errors=",
    "fatal:",
];

fn is_word_char(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

fn is_ascii_word_boundary(text: &str, start: usize, end: usize) -> bool {
    let before_is_word = text[..start].chars().next_back().is_some_and(is_word_char);
    let after_is_word = text[end..].chars().next().is_some_and(is_word_char);
    !before_is_word && !after_is_word
}

fn contains_bounded_word(text: &str, word: &str) -> bool {
    text.match_indices(word)
        .any(|(start, _)| is_ascii_word_boundary(text, start, start + word.len()))
}

fn starts_with_keyword(text: &str, keyword: &str) -> bool {
    text.strip_prefix(keyword).is_some_and(|rest| {
        rest.chars()
            .next()
            .is_none_or(|character| !is_word_char(character))
    })
}

fn has_import_pattern(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("import") else {
        return false;
    };
    let mut characters = rest.chars();
    if !characters.next().is_some_and(char::is_whitespace) {
        return false;
    }
    while characters.clone().next().is_some_and(char::is_whitespace) {
        characters.next();
    }
    characters.next().is_some_and(is_word_char)
}

fn has_from_import_pattern(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("from") else {
        return false;
    };
    let mut characters = rest.chars();
    if !characters.next().is_some_and(char::is_whitespace) {
        return false;
    }
    while characters.clone().next().is_some_and(char::is_whitespace) {
        characters.next();
    }

    let mut module_characters = 0;
    while characters
        .clone()
        .next()
        .is_some_and(|character| is_word_char(character) || character == '.')
    {
        characters.next();
        module_characters += 1;
    }
    if module_characters == 0 || !characters.next().is_some_and(char::is_whitespace) {
        return false;
    }
    while characters.clone().next().is_some_and(char::is_whitespace) {
        characters.next();
    }

    let remaining: String = characters.collect();
    starts_with_keyword(&remaining, "import")
}

fn has_struct_pattern(line: &str) -> bool {
    let stripped = line.trim_start();
    if has_import_pattern(stripped) || has_from_import_pattern(stripped) {
        return true;
    }
    if stripped.starts_with('@') {
        return stripped.chars().nth(1).is_some_and(is_word_char);
    }
    starts_with_keyword(stripped, "raise") || starts_with_keyword(stripped, "except")
}

fn has_test_line_pattern(line: &str) -> bool {
    let stripped = line.trim_start();

    for prefix in ["FAILED", "ERROR", "FAIL", "XFAIL"] {
        if let Some(rest) = stripped.strip_prefix(prefix) {
            let mut characters = rest.chars();
            if matches!(characters.next(), Some(':') | Some(' '))
                && characters
                    .next()
                    .is_some_and(|character| !character.is_whitespace())
            {
                return true;
            }
        }
    }

    if let Some(marker_start) = stripped.find(".py::") {
        let before = &stripped[..marker_start];
        let after = &stripped[marker_start + ".py::".len()..];
        if !before.is_empty()
            && before.chars().all(|character| !character.is_whitespace())
            && after
                .chars()
                .next()
                .is_some_and(|character| !character.is_whitespace())
        {
            return true;
        }
    }

    for prefix in ["FAIL", "ERROR"] {
        let Some(rest) = stripped.strip_prefix(prefix) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(": test") else {
            continue;
        };
        let mut characters = rest.chars();
        while characters
            .clone()
            .next()
            .is_some_and(|character| !character.is_whitespace())
        {
            characters.next();
        }
        if characters.next() == Some(' ')
            && characters.next() == Some('(')
            && characters.any(|character| character == ')')
        {
            return true;
        }
    }

    contains_bounded_word(line, "AssertionError")
}

fn has_declaration_signature(line: &str, keyword: &str) -> bool {
    line.match_indices(keyword).any(|(start, _)| {
        let end = start + keyword.len();
        is_ascii_word_boundary(line, start, end)
            && line[end..].chars().next().is_some_and(char::is_whitespace)
            && line[end..]
                .chars()
                .find(|character| !character.is_whitespace())
                .is_some_and(is_word_char)
    })
}

fn has_call_signature(line: &str) -> bool {
    let characters: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        if is_word_char(characters[index]) && (index == 0 || !is_word_char(characters[index - 1])) {
            let mut cursor = index + 1;
            while cursor < characters.len() && is_word_char(characters[cursor]) {
                cursor += 1;
            }
            while cursor < characters.len() && characters[cursor].is_whitespace() {
                cursor += 1;
            }
            if cursor < characters.len() && characters[cursor] == '(' {
                cursor += 1;
                while cursor < characters.len() && characters[cursor] != ')' {
                    cursor += 1;
                }
                if cursor < characters.len() {
                    cursor += 1;
                    while cursor < characters.len() && characters[cursor].is_whitespace() {
                        cursor += 1;
                    }
                    if cursor < characters.len() && characters[cursor] == ':' {
                        return true;
                    }
                }
            }
        }
        index += 1;
    }
    false
}

fn has_signature_pattern(line: &str) -> bool {
    has_declaration_signature(line, "def")
        || has_declaration_signature(line, "class")
        || has_declaration_signature(line, "function")
        || line
            .match_indices("=>")
            .any(|(start, _)| line[start + 2..].trim_start().starts_with('{'))
        || has_call_signature(line)
}

fn has_diff_pattern(line: &str) -> bool {
    let stripped = line.trim_start();
    stripped.starts_with(['+', '-'])
        || stripped.starts_with("@@")
        || stripped.starts_with("diff --git")
        || stripped.starts_with("---")
        || stripped.starts_with("+++")
}

fn is_path_character(character: char) -> bool {
    is_word_char(character) || matches!(character, '.' | '-')
}

fn has_extension(segment: &str) -> bool {
    let mut extensions = segment.split('.');
    let _ = extensions.next();
    extensions.any(|extension| {
        extension
            .chars()
            .take(4)
            .take_while(|character| character.is_ascii_alphabetic())
            .next()
            .is_some()
    })
}

fn has_path_pattern(line: &str) -> bool {
    for run in line.split(|character: char| !is_path_character(character) && character != '/') {
        let mut segment_count = 0;
        for segment in run.split('/') {
            if segment.is_empty() {
                segment_count = 0;
                continue;
            }
            if segment_count > 0 && has_extension(segment) {
                return true;
            }
            segment_count += 1;
        }
    }
    false
}

fn line_is_pinned(line: &str) -> bool {
    if line.trim().is_empty() {
        return false;
    }
    let lowercase = line.to_ascii_lowercase();
    ERROR_MARKERS
        .iter()
        .any(|marker| lowercase.contains(marker))
        || has_test_line_pattern(line)
        || has_signature_pattern(line)
        || has_struct_pattern(line)
        || has_diff_pattern(line)
        || has_path_pattern(line)
}

fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, character) in line.char_indices() {
        if is_word_char(character) {
            if start.is_none() {
                start = Some(index);
            }
        } else if let Some(token_start) = start.take() {
            tokens.push(line[token_start..index].to_lowercase());
        }
    }
    if let Some(token_start) = start {
        tokens.push(line[token_start..].to_lowercase());
    }
    tokens
}

fn line_scores(lines: &[&str]) -> Vec<f64> {
    if lines.is_empty() {
        return Vec::new();
    }

    let mut term_frequencies = Vec::with_capacity(lines.len());
    let mut document_frequency = BTreeMap::<String, usize>::new();
    for line in lines {
        let mut counts = BTreeMap::<String, usize>::new();
        for token in tokenize(line) {
            let count = counts.entry(token).or_insert(0);
            *count = count.saturating_add(1);
        }
        for token in counts.keys() {
            let frequency = document_frequency.entry(token.clone()).or_insert(0);
            *frequency = frequency.saturating_add(1);
        }
        term_frequencies.push(counts);
    }

    // For document d and term t, use smooth TF-IDF exactly as follows:
    //   idf(t) = ln((1 + N) / (1 + df(t))) + 1
    //   weight(d,t) = tf(d,t) * idf(t)
    //   normalized(d,t) = weight(d,t) / sqrt(sum_u weight(d,u)^2)
    //   score(d) = sum_t normalized(d,t)
    // where N is the number of input lines and df(t) is the number of lines
    // containing t. Tokens are lower-cased Unicode `\w+` runs, and each line
    // is one document. A zero-token line has score zero.
    let document_count = lines.len() as f64;
    term_frequencies
        .iter()
        .map(|counts| {
            let mut weighted_sum = 0.0;
            let mut norm_squared = 0.0;
            for (token, count) in counts {
                let frequency = match document_frequency.get(token) {
                    Some(value) => *value as f64,
                    None => 0.0,
                };
                let idf = ((1.0 + document_count) / (1.0 + frequency)).ln() + 1.0;
                let weight = *count as f64 * idf;
                weighted_sum += weight;
                norm_squared += weight * weight;
            }
            if norm_squared > 0.0 {
                weighted_sum / norm_squared.sqrt()
            } else {
                0.0
            }
        })
        .collect()
}

fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut boundary = index.min(text.len());
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn ceil_char_boundary(text: &str, index: usize) -> usize {
    let mut boundary = index.min(text.len());
    while boundary < text.len() && !text.is_char_boundary(boundary) {
        boundary += 1;
    }
    boundary
}

fn cmp_block(inner: &str) -> String {
    let inner = inner.trim_matches('\n');
    if inner.is_empty() {
        return format!("{CMP_START}{CMP_END}");
    }
    format!("{CMP_START}\n{inner}\n{CMP_END}")
}

fn truncate_text(value: &str, head: usize, tail: usize) -> String {
    if value.len() <= head.saturating_add(tail).saturating_add(80) {
        return value.to_owned();
    }
    let head_end = floor_char_boundary(value, head);
    let tail_start = ceil_char_boundary(value, value.len().saturating_sub(tail));
    let head_text = value.get(..head_end).map_or("", |part| part);
    let tail_text = value.get(tail_start..).map_or("", |part| part);
    format!("{head_text}\n{}\n{tail_text}", cmp_block(""))
}

fn extractive_compress(text: &str, target_chars: usize) -> (String, bool) {
    if text.len() <= target_chars {
        return (text.to_owned(), false);
    }

    let lines: Vec<&str> = text.split('\n').collect();
    let mut keep = vec![false; lines.len()];
    let mut used = 0_usize;
    for (index, line) in lines.iter().enumerate() {
        if line_is_pinned(line) {
            keep[index] = true;
            used = used.saturating_add(line.len().saturating_add(1));
        }
    }

    let remaining: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (!keep[index] && !line.trim().is_empty()).then_some(index))
        .collect();
    if used < target_chars && !remaining.is_empty() {
        let candidate_lines: Vec<&str> = remaining.iter().map(|&index| lines[index]).collect();
        let scores = line_scores(&candidate_lines);
        let mut ranked: Vec<(usize, f64)> = remaining.into_iter().zip(scores).collect();
        ranked.sort_by(|(left_index, left_score), (right_index, right_score)| {
            right_score
                .total_cmp(left_score)
                .then_with(|| left_index.cmp(right_index))
        });
        for (index, _) in ranked {
            if used >= target_chars {
                break;
            }
            keep[index] = true;
            used = used.saturating_add(lines[index].len().saturating_add(1));
        }
    }

    if keep.iter().all(|line_is_kept| *line_is_kept) {
        return (text.to_owned(), false);
    }

    let mut output_parts: Vec<&str> = Vec::new();
    let mut previous: Option<usize> = None;
    for (index, line) in lines.iter().enumerate() {
        if !keep[index] {
            continue;
        }
        // A gap before the FIRST kept line also gets an ellipsis (reference
        // starts prev at -1), so elision at the head stays visible.
        if previous.map_or(index > 0, |previous_index| {
            index > previous_index.saturating_add(1)
        }) {
            output_parts.push("…");
        }
        output_parts.push(line);
        previous = Some(index);
    }
    if previous.is_none_or(|previous_index| previous_index.saturating_add(1) < lines.len()) {
        output_parts.push("…");
    }

    let result = output_parts.join("\n");
    if result.len() >= text.len() {
        let head = target_chars.saturating_mul(3) / 4;
        let tail = target_chars / 4;
        return (truncate_text(text, head, tail), true);
    }
    (result, true)
}

fn proportional_cap(length: usize) -> usize {
    let proportional = (length as f64 * KEEP_FRACTION) as usize;
    proportional.clamp(MIN_PASSTHROUGH_CHARS, MAX_KEEP_CHARS)
}

/// Compress an oversized text value, returning `None` when it should pass through unchanged.
#[must_use]
pub fn compress(text: &str) -> Option<String> {
    if text.len() <= MIN_PASSTHROUGH_CHARS || text.contains(CMP_START) {
        return None;
    }

    let cap = proportional_cap(text.len());
    let inner_cap = cap
        .saturating_sub(CMP_START.len() + CMP_END.len() + 2)
        .max(256);
    let (inner, _) = extractive_compress(text, inner_cap);
    let wrapped = cmp_block(&inner);
    (wrapped.len() < text.len()).then_some(wrapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const VARIED_PARAGRAPH: &str = concat!(
        "Every ordinary record describes a calm brown river under a pale morning sky.\n",
        "Repeated context carries durable meaning while low-value details vary by turn.\n",
        "Stable ordering keeps identical requests aligned with warm provider caches.\n",
        "Short observations join this paragraph without adding special diagnostics or paths.\n",
    );

    const MIXED_PARAGRAPH: &str = concat!(
        "The gateway keeps a stable account of the request and its surrounding context.\n",
        "A small helper reads the value, trims whitespace, and returns the same result.\n",
        "let normalized_value = value.trim().to_owned();\n",
        "2026-08-25T12:34:56Z INFO request completed with a warm cache entry\n",
        "The next ordinary paragraph preserves enough varied language for ranking.\n",
    );

    fn repeated_to_len(target_len: usize, paragraph: &str) -> String {
        assert!(!paragraph.is_empty());
        let mut text = String::with_capacity(target_len);
        while text.len() < target_len {
            text.push_str(paragraph);
        }
        text.truncate(floor_char_boundary(&text, target_len));
        text
    }

    fn compressed_body(output: &str) -> &str {
        let Some(body) = output.strip_prefix("[[CMP]]\n") else {
            return output;
        };
        body.strip_suffix("\n[[/CMP]]").map_or(body, |body| body)
    }

    #[test]
    fn input_at_or_below_passthrough_floor_returns_none() {
        let text = "x".repeat(MIN_PASSTHROUGH_CHARS);
        assert!(compress(&text).is_none());
    }

    #[test]
    fn lorem_input_compresses_and_is_idempotent() {
        let mut text = String::new();
        while text.len() < 60_000 {
            text.push_str(
                "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\n",
            );
        }
        text.truncate(floor_char_boundary(&text, 60_000));
        assert_eq!(text.len(), 60_000);

        let compressed = compress(&text);
        assert!(compressed.is_some(), "60k lorem input was not compressed");
        let output = compressed.as_ref().map_or("", |value| value.as_str());
        assert!(output.len() < text.len());
        assert!(output.contains(CMP_START));
        assert!(compress(output).is_none());
    }

    #[test]
    fn multibyte_input_around_byte_caps_does_not_panic() {
        let mut text = String::new();
        while text.len() <= MAX_KEEP_CHARS {
            text.push_str("😀 lorem ipsum dolor sit amet, consectetur adipiscing elit\n");
        }
        let _ = compress(&text);

        let emoji_text = "😀".repeat(10_000);
        let truncated = truncate_text(&emoji_text, 16_001, 16_001);
        assert!(truncated.contains(CMP_START));
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());
    }

    #[test]
    fn compression_is_deterministic_across_threads() {
        let text = repeated_to_len(60_000, MIXED_PARAGRAPH);
        assert!(text.len() > 53_000);

        let mut outputs = vec![compress(&text), compress(&text)];
        let threaded = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4).map(|_| scope.spawn(|| compress(&text))).collect();
            handles
                .into_iter()
                .map(|handle| {
                    let result = handle.join();
                    assert!(result.is_ok(), "compression worker panicked");
                    result.ok().flatten()
                })
                .collect::<Vec<_>>()
        });
        outputs.extend(threaded);

        assert!(
            outputs[0].is_some(),
            "oversized mixed input did not compress"
        );
        let expected = outputs[0].as_deref();
        assert!(outputs.iter().all(|output| output.as_deref() == expected));
    }

    #[test]
    fn compression_never_inflates_across_size_sweep() {
        for length in [
            15_999, 16_000, 16_001, 20_000, 26_700, 40_000, 53_400, 80_000,
        ] {
            let text = repeated_to_len(length, VARIED_PARAGRAPH);
            assert_eq!(text.len(), length);
            let output = compress(&text);

            if length <= MIN_PASSTHROUGH_CHARS {
                assert!(output.is_none(), "{length} bytes should pass through");
            }
            if let Some(output) = output {
                assert!(
                    output.len() < text.len(),
                    "compressed {length}-byte input inflated to {} bytes",
                    output.len()
                );
            }
        }
    }

    #[test]
    fn pinned_lines_survive_a_large_log() {
        let mut text = String::new();
        for index in 0..4_000 {
            match index {
                137 => text.push_str(concat!(
                    "Traceback (most recent call last):\n",
                    "  File \"service/worker.py\", line 42, in handler\n",
                    "  File \"service/processor.py\", line 17, in process\n",
                    "AssertionError: boom\n",
                )),
                811 => text.push_str("FAILED test_alpha\n"),
                1_711 => text.push_str("tests/test_x.py::test_beta\n"),
                2_333 => text.push_str("django/utils/html.py:236\n"),
                2_811 => text.push_str("import inspect\n"),
                3_011 => text.push_str("from functools import wraps\n"),
                3_211 => text.push_str("def handler(event):\n"),
                3_511 => text.push_str("@@ -1,4 +1,6 @@\n+added line\n-removed line\n"),
                _ => {}
            }
            text.push_str(&format!(
                "INFO filler record {index:04} carries routine context words only\n"
            ));
        }
        assert!(text.len() > 53_000);

        let compressed = compress(&text);
        assert!(compressed.is_some(), "large log did not compress");
        let output = compressed.unwrap_or_default();
        for expected in [
            "Traceback (most recent call last):",
            "  File \"service/worker.py\", line 42, in handler",
            "  File \"service/processor.py\", line 17, in process",
            "AssertionError: boom",
            "FAILED test_alpha",
            "tests/test_x.py::test_beta",
            "django/utils/html.py:236",
            "import inspect",
            "from functools import wraps",
            "def handler(event):",
            "@@ -1,4 +1,6 @@",
            "+added line",
            "-removed line",
        ] {
            assert!(
                output.lines().any(|line| line == expected),
                "pinned line was lost: {expected}"
            );
        }
    }

    #[test]
    fn ellipses_mark_head_middle_and_tail_elision() {
        let mut text = String::new();
        for index in 0..4_000 {
            if index == 0 {
                text.push_str("low-value filler line\n");
                continue;
            }
            match index {
                700 => text.push_str("FAILED head_anchor\n"),
                2_000 => text.push_str("django/utils/html.py:236\n"),
                3_300 => text.push_str("def handler(event):\n"),
                _ => {}
            }
            text.push_str(&format!(
                "routine filler record {index:04} contains ordinary context words and no diagnostic signal\n"
            ));
        }
        assert!(text.len() > 53_000);

        let compressed = compress(&text);
        assert!(compressed.is_some(), "ellipsis fixture did not compress");
        let output = compressed.unwrap_or_default();
        let body = compressed_body(&output);
        let lines: Vec<_> = body.lines().collect();

        assert_eq!(lines.first().copied(), Some("…"));
        assert!(lines
            .iter()
            .enumerate()
            .any(|(index, line)| { *line == "…" && index > 0 && index + 1 < lines.len() }));
        assert_eq!(lines.last().copied(), Some("…"));
    }

    #[test]
    fn any_existing_compression_marker_is_a_fixed_point() {
        let inputs = [
            "prefix [[CMP]] suffix".to_owned(),
            format!("{}[[CMP]]{}", "x".repeat(50_000), "y".repeat(50_000)),
            format!("[[CMP]]{}", "z".repeat(100_000)),
        ];

        for input in inputs {
            assert!(compress(&input).is_none());
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 16,
            ..ProptestConfig::default()
        })]

        #[test]
        fn arbitrary_unicode_input_preserves_compression_invariants(
            characters in prop::collection::vec(
                prop_oneof![
                    4 => Just('\n'),
                    8 => prop::char::range('a', 'z'),
                    3 => prop::char::range('0', '9'),
                    3 => prop::char::range('\u{00c0}', '\u{024f}'),
                    3 => prop::char::range('\u{0400}', '\u{04ff}'),
                    2 => prop::char::range('\u{1f600}', '\u{1f64f}'),
                    2 => Just(' '),
                    1 => Just('\t'),
                ],
                // The largest generated character is four bytes, so this is
                // bounded at roughly 100k input bytes while retaining varied
                // Unicode and newline coverage.
                0..=25_000,
            )
        ) {
            let input: String = characters.into_iter().collect();
            if let Some(output) = compress(&input) {
                prop_assert!(output.len() < input.len());
                let round_trip = std::str::from_utf8(output.as_bytes());
                prop_assert_eq!(round_trip.ok(), Some(output.as_str()));
                prop_assert_eq!(compress(&output), None);
            }
        }
    }

    #[test]
    fn ten_megabyte_single_line_completes() {
        let text = "x".repeat(10_000_000);
        let _ = compress(&text);
    }

    #[test]
    fn ten_megabytes_across_two_hundred_thousand_lines_completes() {
        let line = "ordinary filler context words for bounded work check 1234567890\n";
        let text = line.repeat(200_000);
        assert!(text.len() >= 10_000_000);
        assert_eq!(text.lines().count(), 200_000);
        let _ = compress(&text);
    }
}
