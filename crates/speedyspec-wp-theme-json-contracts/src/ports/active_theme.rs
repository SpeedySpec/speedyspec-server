use crate::Value;

/// The active theme and the site facts the resolver and the global styles functions read. Each method mirrors one
/// WordPress call; the defaults describe a classic theme without `theme.json` on a production site.
pub trait ActiveTheme {
    /// `current_theme_supports( $feature )`.
    fn supports(&self, feature: &str) -> bool;
    /// `get_stylesheet()`.
    fn stylesheet(&self) -> String {
        String::new()
    }
    /// `get_stylesheet_directory()`.
    fn stylesheet_directory(&self) -> String {
        String::new()
    }
    /// `get_template_directory()`.
    fn template_directory(&self) -> String {
        String::new()
    }
    /// `wp_get_theme()->get_file_path( 'theme.json' )`: the child's file when it has one, else the parent's.
    fn theme_json_path(&self) -> String {
        String::new()
    }
    /// `wp_get_theme()->parent()->get_file_path( 'theme.json' )`, or `None` without a parent.
    fn parent_theme_json_path(&self) -> Option<String> {
        None
    }
    /// `wp_get_theme()->get( 'TextDomain' )`.
    fn text_domain(&self) -> Value {
        Value::Bool(false)
    }
    /// `wp_get_theme()->parent()->get( 'TextDomain' )`.
    fn parent_text_domain(&self) -> Value {
        Value::Bool(false)
    }
    /// `wp_theme_has_theme_json()`.
    fn has_theme_json(&self) -> bool {
        false
    }
    /// `wp_is_block_theme()`.
    fn is_block_theme(&self) -> bool {
        false
    }
    /// `wp_is_development_mode( 'theme' )`.
    fn is_development_mode(&self) -> bool {
        false
    }
    /// `get_classic_theme_supports_block_editor_settings()`.
    fn classic_editor_settings(&self) -> Value {
        Value::Array(Vec::new())
    }
    /// `sanitize_url( get_theme_file_uri( $file ) )`.
    fn theme_file_uri(&self, file: &str) -> String {
        file.to_owned()
    }
    /// The directory holding core's `theme.json` (`ABSPATH . WPINC`), when the resolver was not given one.
    fn core_directory(&self) -> String {
        String::new()
    }
    /// `wp_check_filetype( $file )['type']`.
    fn file_type(&self, _file: &str) -> Value {
        Value::Bool(false)
    }
}
