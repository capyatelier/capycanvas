use unicode_normalization::UnicodeNormalization;

pub(crate) fn normalize(text: &str) -> String {
    text.nfkc().flat_map(char::to_lowercase).collect()
}

pub(crate) fn ascii_key_query(text: &str) -> bool {
    let mut characters = text.chars();
    characters.next().is_some_and(|character| character.is_ascii_graphic()) && characters.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_normalizes_compatibility_width_and_composed_hangul() {
        assert_eq!(normalize("Ｂｒｕｓｈ"), normalize("Brush"));
        assert_eq!(normalize("한글"), normalize("한글"));
        assert_eq!(normalize("畫布 日本語 🎨"), "畫布 日本語 🎨");
    }

    #[test]
    fn single_cjk_characters_remain_text_queries() {
        for query in ["画", "한", "あ", "🎨", "", "ab", " "] { assert!(!ascii_key_query(query)); }
        for query in ["a", "7", "+", "/"] { assert!(ascii_key_query(query)); }
    }
}
