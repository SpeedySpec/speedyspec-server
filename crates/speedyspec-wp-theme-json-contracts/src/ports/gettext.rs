/// WordPress's translation of its own strings (`__( $text, $domain )`), owned by the i18n port.
pub trait Gettext {
    /// `__( $text, $domain )`.
    fn translate(&self, text: &str, domain: &str) -> Vec<u8>;
}
