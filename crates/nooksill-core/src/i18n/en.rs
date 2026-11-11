//! English base table: the authoritative list of all UI text keys.
//!
//! Sorted for binary search. `i18n::tests` checks order, uniqueness and that
//! every `t("...")` / `fmt("...")` call site is registered here.

mod first;
mod second;

/// Two sorted segments concatenated at compile time; the public table stays one slice.
pub const TEXT: &[&str] = &{
    const N: usize = first::TEXT.len() + second::TEXT.len();
    let mut rows = [""; N];
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

pub fn lookup(key: &str) -> Option<&'static str> {
    TEXT.binary_search_by(|k| (*k).cmp(key))
        .ok()
        .map(|i| TEXT[i])
}
