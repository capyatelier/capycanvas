use unicode_normalization::UnicodeNormalization;

pub(crate) fn normalize(text: &str) -> String {
    let mut previous = '\0';
    let folded: String = text.nfkc().map(|character| match character {
        'İ' | 'ı' => 'I',
        'ẞ' => 'ß',
        character => character,
    }).flat_map(char::to_uppercase).flat_map(char::to_lowercase).filter(|&character| {
        if character == '\u{307}' && previous == 'i' { return false; }
        previous = character;
        true
    }).collect();
    folded.nfkc().collect()
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
    fn search_matches_turkish_i_variants_and_english_aliases() {
        for (upper, lower) in [("IŞIK", "ışık"), ("İÇE AKTAR", "içe aktar"),
            ("I\u{307}ÇE AKTAR", "içe aktar"), ("i\u{307}çe aktar", "içe aktar"), ("IMAGE SIZE", "image size")] {
            assert_eq!(normalize(upper), normalize(lower));
        }
    }

    #[test]
    fn search_matches_german_sharp_s_and_greek_final_sigma_without_losing_accents() {
        for upper in ["GRÖSSE", "GRÖẞE"] { assert_eq!(normalize(upper), normalize("Größe")); }
        assert_ne!(normalize("Größe"), normalize("GROSSE"));
        for text in ["ΟΣ", "ος", "οσ"] { assert_eq!(normalize(text), "οσ"); }
    }

    #[test]
    fn search_preserves_vietnamese_accents_across_normalization() {
        assert_eq!(normalize("TIẾNG VIỆT"), normalize("Tie\u{302}\u{301}ng Vie\u{323}\u{302}t"));
        assert_eq!(normalize("ĐỘ ĐỤC"), normalize("độ đục"));
        assert_ne!(normalize("độ đục"), normalize("do duc"));
        assert_ne!(normalize("vẽ"), normalize("vẻ"));
    }

    #[test]
    fn single_cjk_characters_remain_text_queries() {
        for query in ["画", "한", "あ", "🎨", "", "ab", " ", "ß", "ẞ", "İ", "ı", "đ", "σ", "ς"] { assert!(!ascii_key_query(query)); }
        for query in ["a", "7", "+", "/"] { assert!(ascii_key_query(query)); }
    }
}
