/// The text functions theme.json calls, owned by the text port (each fires its WordPress filters).
pub trait TextFunctions {
    /// `sanitize_title( $title )`.
    fn sanitize_title(&self, title: &[u8]) -> Vec<u8>;
    /// `sanitize_html_class( $classname )`.
    fn sanitize_html_class(&self, classname: &[u8]) -> Vec<u8>;
    /// `esc_html( $text )`.
    fn esc_html(&self, text: &[u8]) -> Vec<u8>;
    /// `esc_attr( $text )`.
    fn esc_attr(&self, text: &[u8]) -> Vec<u8>;
}
