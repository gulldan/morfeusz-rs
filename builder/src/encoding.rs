pub trait SortEncoder {
    type Key: Ord;

    fn word_sort_key(&self, word: &str) -> Self::Key;
}

pub trait WordBytesEncoder {
    fn encode_word_bytes(&self, word: &str) -> Vec<u8>;
}

pub struct IdentityEncoder;

impl SortEncoder for IdentityEncoder {
    type Key = String;

    fn word_sort_key(&self, word: &str) -> Self::Key {
        word.to_owned()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Utf8WordEncoder;

impl WordBytesEncoder for Utf8WordEncoder {
    fn encode_word_bytes(&self, word: &str) -> Vec<u8> {
        word.as_bytes().to_vec()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Utf8AnalyzerEncoder;

impl SortEncoder for Utf8AnalyzerEncoder {
    type Key = String;

    fn word_sort_key(&self, word: &str) -> Self::Key {
        word.to_lowercase()
    }
}

impl WordBytesEncoder for Utf8AnalyzerEncoder {
    fn encode_word_bytes(&self, word: &str) -> Vec<u8> {
        word.as_bytes().to_vec()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Utf8GeneratorEncoder;

impl SortEncoder for Utf8GeneratorEncoder {
    type Key = String;

    fn word_sort_key(&self, word: &str) -> Self::Key {
        word.to_owned()
    }
}

impl WordBytesEncoder for Utf8GeneratorEncoder {
    fn encode_word_bytes(&self, word: &str) -> Vec<u8> {
        word.as_bytes().to_vec()
    }
}
