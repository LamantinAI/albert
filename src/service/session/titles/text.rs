//! Input minimization, never a credential guarantee for arbitrary natural text.
/// Only ordinary first-user text enters metadata inference. Headers, media,
/// code/credential lines and opaque tokens are omitted before the byte cap.
pub(super) fn excerpt(text: &str, bytes: usize) -> Option<String> {
    let text = if text.starts_with("[Message source;") {
        text.split_once('\n')?.1
    } else {
        text
    };
    let mut code = false;
    let mut words = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            code = !code;
            continue;
        }
        let lower = line.to_lowercase();
        let compact: String = lower
            .chars()
            .filter(|ch| !ch.is_whitespace() && *ch != '"' && *ch != '\'')
            .collect();
        let credential = [
            "password",
            "passwd",
            "secret",
            "token",
            "api_key",
            "api-key",
            "apikey",
            "access_token",
            "authorization",
        ]
        .iter()
        .any(|key| compact.contains(&format!("{key}=")) || compact.contains(&format!("{key}:")));
        if code
            || credential
            || line.trim_start().starts_with('/')
            || [
                "-----begin",
                "data:",
                "[image",
                "[voice",
                "(image",
                "(voice",
            ]
            .iter()
            .any(|pattern| lower.contains(pattern))
        {
            continue;
        }
        for word in line.split_whitespace() {
            let lower = word.to_lowercase();
            let opaque = ["sk-", "ghp_", "github_pat_", "xoxb-", "xoxp-", "eyj"]
                .iter()
                .any(|prefix| {
                    lower
                        .trim_matches(|ch: char| !ch.is_alphanumeric())
                        .starts_with(prefix)
                });
            if opaque || word.contains("://") || word.contains('@') || word.chars().count() > 48 {
                continue;
            }
            if words
                .iter()
                .map(|word: &String| word.len() + 1)
                .sum::<usize>()
                + word.len()
                > bytes
            {
                break;
            }
            words.push(word.to_owned());
        }
    }
    let text = words.join(" ");
    (text.chars().filter(|ch| ch.is_alphabetic()).count() >= 3).then_some(text)
}

pub(crate) fn normalize(text: &str, max_chars: usize) -> Option<String> {
    let text = text
        .lines()
        .find(|line| !line.trim().is_empty())?
        .trim()
        .trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == '`' || ch == '#' || ch == '*');
    let title: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|ch| !ch.is_control())
        .take(max_chars)
        .collect();
    let generic = [
        "untitled",
        "new chat",
        "conversation",
        "chat",
        "без названия",
        "новый чат",
    ]
    .contains(&title.to_lowercase().as_str());
    (title.chars().filter(|ch| ch.is_alphabetic()).count() >= 3
        && !title.contains("data:")
        && !generic)
        .then_some(title)
}
