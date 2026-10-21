/// Shortens `s` to at most `max` characters, adding "…" when it cuts.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    format!("{}…", &s[..max])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_unchanged() {
        assert_eq!(truncate("hi", 5), "hi");
    }

    #[test]
    fn long_text_is_cut() {
        assert_eq!(truncate("hello world", 5), "hello…");
    }

    #[test]
    fn accented_text_is_cut_by_characters() {
        assert_eq!(truncate("héllo wörld", 2), "hé…");
    }
}
