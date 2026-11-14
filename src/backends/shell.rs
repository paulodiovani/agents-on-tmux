/// Builds a POSIX shell command line that runs `words` as a single argv.
pub fn join<S: AsRef<str>>(words: &[S]) -> String {
    let quoted: Vec<String> = words.iter().map(|word| quote(word.as_ref())).collect();
    quoted.join(" ")
}

fn quote(word: &str) -> String {
    let is_plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    if is_plain {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_quotes_words_only_when_needed() {
        let words = ["grep", "-r", "it's here", "a b", "", "$HOME", "src/main.rs"];
        assert_eq!(
            join(&words),
            r#"grep -r 'it'\''s here' 'a b' '' '$HOME' src/main.rs"#
        );
    }

    #[test]
    fn test_join_empty() {
        assert_eq!(join::<&str>(&[]), "");
    }
}
