use super::words;

#[test]
fn related_words_ignore_case_punctuation_and_single_letters() {
    let found = words("Lighthouse, 등대지기 a 해솔!");
    assert!(found.contains("lighthouse"));
    assert!(found.contains("등대지기"));
    assert!(!found.contains("a"));
}
