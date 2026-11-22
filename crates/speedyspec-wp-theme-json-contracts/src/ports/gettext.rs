/// WordPress's translation of its own strings (`__( $text, $domain )`), owned by the i18n port.
pub trait Gettext {
    /// `__( $text, $domain )`.
    fn translate(&self, text: &str, domain: &str) -> Vec<u8>;
    /// `translate_with_gettext_context( $text, $context, $domain )`.
    fn translate_with_context(&self, text: &[u8], _context: &[u8], _domain: &[u8]) -> Vec<u8> {
        text.to_vec()
    }
}
