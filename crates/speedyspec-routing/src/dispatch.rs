/// Compiles an ordered list of regex patterns into something that finds the first one matching (Strategy).
///
/// Subjects are bytes and patterns have PCRE's semantics without `/u`, as WordPress matches its rules: shorthand
/// classes and case folding are ASCII, and `.` is one byte.
pub trait DispatchStrategy {
    fn build(&self, patterns: &[String]) -> Result<Box<dyn CompiledDispatch>, String>;
}

/// The compiled form of a pattern list.
pub trait CompiledDispatch {
    /// The first compiled pattern at index `>= from` that matches `subject`, with its captures (index 0 is the
    /// whole match). Patterns no engine compiled are never returned.
    fn find(&self, subject: &[u8], from: usize) -> Option<(usize, Vec<Option<String>>)>;
    /// The first pattern at index `>= from` that no Rust engine compiled.
    fn first_needs_php(&self, from: usize) -> Option<usize>;
}
