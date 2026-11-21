//! The shape of the recorded API cases.

/// Which function a case calls.
pub enum Op {
    Do,
    DoIgnoreHtml,
    Strip,
    Has(Vec<u8>),
    Tags,
    TagsEmptyRegistry,
}

/// What WordPress returned.
#[derive(Debug, PartialEq)]
pub enum Expected {
    Bytes(Vec<u8>),
    Bool(bool),
    Tags(Vec<Vec<u8>>),
}

/// One call, WordPress's result, and the notices WordPress raised during it.
pub struct ApiCase {
    pub op: Op,
    pub input: &'static [u8],
    pub expected: Expected,
    pub diagnostics: Vec<Vec<u8>>,
}
