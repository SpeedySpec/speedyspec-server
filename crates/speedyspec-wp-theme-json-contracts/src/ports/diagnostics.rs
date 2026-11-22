/// WordPress's developer notices, so a misuse is reported where WordPress reports it.
pub trait Diagnostics {
    /// `wp_trigger_error( $function_name, $message, E_USER_NOTICE )`.
    fn trigger_error(&self, function: &str, message: &[u8]);
    /// `_deprecated_function( $function_name, $version, $replacement )`.
    fn deprecated_function(&self, function: &str, version: &str, replacement: &str);
    /// `_doing_it_wrong( $function_name, $message, $version )`.
    fn doing_it_wrong(&self, function: &str, message: &[u8], version: &str);
    /// `_deprecated_argument( $function_name, $version, $message )`.
    fn deprecated_argument(&self, function: &str, version: &str, message: &[u8]);
}
