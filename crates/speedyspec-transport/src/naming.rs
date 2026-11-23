//! English naming helpers the transports' default mappings share.

/// The English plural of a resource type name.
pub fn plural(name: &str) -> String {
    let consonant_y = name.ends_with('y')
        && name
            .chars()
            .rev()
            .nth(1)
            .is_some_and(|c| !"aeiou".contains(c));
    if consonant_y {
        format!("{}ies", &name[..name.len() - 1])
    } else if ["s", "x", "ch", "sh"].iter().any(|end| name.ends_with(end)) {
        format!("{name}es")
    } else {
        format!("{name}s")
    }
}

/// `name` in PascalCase, its words split on `_`, `-` and `/`: `post_type` is `PostType`.
pub fn pascal(name: &str) -> String {
    name.split(['_', '-', '/'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect()
}
