//! Simplified Chinese translations. Missing keys fall back to English.
//!
//! Sorted by the English key so `lookup` can binary search.

mod first;
mod second;

/// Two sorted segments concatenated at compile time; the public table stays one slice.
pub const TEXT: &[(&str, &str)] = &{
    const N: usize = first::TEXT.len() + second::TEXT.len();
    let mut rows = [("", ""); N];
    let mut i = 0;
    while i < first::TEXT.len() {
        rows[i] = first::TEXT[i];
        i += 1;
    }
    let mut j = 0;
    while j < second::TEXT.len() {
        rows[i + j] = second::TEXT[j];
        j += 1;
    }
    rows
};
