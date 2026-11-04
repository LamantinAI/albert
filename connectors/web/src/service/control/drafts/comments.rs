use toml_edit::Item;

pub(super) fn comments(item: &Item, path: &[&str]) -> Vec<String> {
    let Some((key, rest)) = path.split_first() else {
        return Vec::new();
    };
    let Some(table) = item.as_table_like() else {
        return Vec::new();
    };
    let Some(child) = table.get(key) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    if let Some(key) = table.key(key) {
        for decor in [key.leaf_decor(), key.dotted_decor()] {
            for raw in [decor.prefix(), decor.suffix()].into_iter().flatten() {
                result.extend(
                    raw.as_str()
                        .unwrap_or_default()
                        .lines()
                        .map(str::trim)
                        .filter(|line| line.starts_with('#'))
                        .map(str::to_owned),
                );
            }
        }
    }
    if rest.is_empty() {
        if let Some(table) = child.as_table() {
            for raw in [table.decor().prefix(), table.decor().suffix()]
                .into_iter()
                .flatten()
            {
                result.extend(
                    raw.as_str()
                        .unwrap_or_default()
                        .lines()
                        .map(str::trim)
                        .filter(|line| line.starts_with('#'))
                        .map(str::to_owned),
                );
            }
        }
        if let Some(value) = child.as_value() {
            for raw in [value.decor().prefix(), value.decor().suffix()]
                .into_iter()
                .flatten()
            {
                result.extend(
                    raw.as_str()
                        .unwrap_or_default()
                        .lines()
                        .map(str::trim)
                        .filter(|line| line.starts_with('#'))
                        .map(str::to_owned),
                );
            }
        }
    } else {
        result.extend(comments(child, rest));
    }
    result
}

pub(super) fn retain_removed_comments(source: &str, content: &mut String, comments: Vec<String>) {
    for comment in comments {
        let original = source
            .lines()
            .filter(|line| line.trim_end().ends_with(&comment))
            .count();
        let retained = content
            .lines()
            .filter(|line| line.trim_end().ends_with(&comment))
            .count();
        for _ in retained..original {
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&comment);
            content.push('\n');
        }
    }
}
