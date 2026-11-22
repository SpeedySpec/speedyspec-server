/// WordPress's developer notices, so a misuse is reported where WordPress reports it.
pub trait Diagnostics {
    /// `wp_trigger_error( $function_name, $message, E_USER_NOTICE )`.
    fn trigger_error(&self, function: &str, message: &[u8]);
    /// `_deprecated_function( $function_name, $version, $replacement )`.
    fn deprecated_function(&self, function: &str, version: &str, replacement: &str);
}
