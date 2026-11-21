//! `WP_Rewrite::mod_rewrite_rules()` and `iis7_url_rewrite_rules()`. SpeedySpec writes neither file; plugins that
//! read the text still get WordPress's.

use speedyspec_wp_hooks::{HookName, HookValue};

use crate::generate::url_path;
use crate::{RewriteError, RewriteService, RewriteState, RewriteStateSource};

fn trailingslashit(value: &str) -> String {
    format!("{}/", value.trim_end_matches(['/', '\\']))
}

/// `esc_attr()` for a URL: `& < > " '` encoded, existing entities left alone.
fn esc_attr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    for (i, c) in text.char_indices() {
        match c {
            '&' => {
                let rest = &bytes[i + 1..];
                let entity = rest.iter().position(|&b| b == b';').is_some_and(|end| {
                    end > 0
                        && end <= 10
                        && rest[..end]
                            .iter()
                            .all(|b| b.is_ascii_alphanumeric() || *b == b'#')
                });
                out.push_str(if entity { "&" } else { "&amp;" });
            }
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            c => out.push(c),
        }
    }
    out
}

fn text(value: HookValue) -> String {
    match value.read().into_owned() {
        HookValue::Str(s) => s,
        _ => String::new(),
    }
}

impl RewriteService {
    /// `WP_Rewrite::mod_rewrite_rules()`.
    pub fn mod_rewrite_rules(
        &self,
        state: &mut RewriteState,
        source: &dyn RewriteStateSource,
    ) -> Result<String, RewriteError> {
        if !self.using_permalinks(state) {
            return Ok(String::new());
        }
        // WordPress leaves `$site_root` as the parsed array when the site URL has no path, and PHP prints an array
        // as "Array"; kept so the text matches.
        let site_root = url_path(&self.deps.urls.site_url())
            .map_or("Array".to_string(), |p| trailingslashit(&p));
        let home_root =
            url_path(&self.deps.urls.home_url()).map_or("/".to_string(), |p| trailingslashit(&p));

        let mut rules = String::from("<IfModule mod_rewrite.c>\nRewriteEngine On\n");
        rules.push_str("RewriteRule .* - [E=HTTP_AUTHORIZATION:%{HTTP:Authorization}]\n");
        rules.push_str(&format!("RewriteBase {home_root}\n"));
        rules.push_str("RewriteRule ^index\\.php$ - [L]\n");
        for rule in &state.non_wp_rules {
            let pattern = rule.pattern.replace(".+?", ".+");
            rules.push_str(&format!(
                "RewriteRule ^{pattern} {home_root}{} [QSA,L]\n",
                rule.query
            ));
        }
        if state.use_verbose_rules {
            state.matches = String::new();
            let rewrite = self.rewrite_rules(state, source)?;
            rules.push_str(&format!(
                "RewriteCond %{{REQUEST_FILENAME}} -f [OR]\nRewriteCond %{{REQUEST_FILENAME}} -d\nRewriteRule ^.*$ - [S={}]\n",
                rewrite.len()
            ));
            for rule in &rewrite {
                let pattern = rule.pattern.replace(".+?", ".+");
                let root = if rule.query.contains(&state.index) {
                    &home_root
                } else {
                    &site_root
                };
                rules.push_str(&format!(
                    "RewriteRule ^{pattern} {root}{} [QSA,L]\n",
                    rule.query
                ));
            }
        } else {
            rules.push_str(&format!(
                "RewriteCond %{{REQUEST_FILENAME}} !-f\nRewriteCond %{{REQUEST_FILENAME}} !-d\nRewriteRule . {home_root}{} [L]\n",
                state.index
            ));
        }
        rules.push_str("</IfModule>\n");

        let rules = text(self.deps.hooks.apply_filters(
            &HookName::from("mod_rewrite_rules"),
            HookValue::Str(rules),
            vec![],
        )?);
        let rules = self.deps.hooks.apply_filters_deprecated(
            &HookName::from("rewrite_rules"),
            vec![HookValue::Str(rules)],
            &HookValue::Str("1.5.0".into()),
            &HookValue::Str("mod_rewrite_rules".into()),
            &HookValue::Str(String::new()),
        )?;
        Ok(text(rules))
    }

    /// `WP_Rewrite::iis7_url_rewrite_rules()`.
    pub fn iis7_url_rewrite_rules(
        &self,
        state: &RewriteState,
        add_parent_tags: bool,
    ) -> Result<String, RewriteError> {
        if !self.using_permalinks(state) {
            return Ok(String::new());
        }
        let mut rules = String::new();
        if add_parent_tags {
            rules.push_str("<configuration>\n\t<system.webServer>\n\t\t<rewrite>\n\t\t\t<rules>");
        }
        rules.push_str(&format!(
            "\n\t\t\t<rule name=\"WordPress: {}\" patternSyntax=\"Wildcard\">\n\t\t\t\t<match url=\"*\" />\n\t\t\t\t\t<conditions>\n\t\t\t\t\t\t<add input=\"{{REQUEST_FILENAME}}\" matchType=\"IsFile\" negate=\"true\" />\n\t\t\t\t\t\t<add input=\"{{REQUEST_FILENAME}}\" matchType=\"IsDirectory\" negate=\"true\" />\n\t\t\t\t\t</conditions>\n\t\t\t\t<action type=\"Rewrite\" url=\"index.php\" />\n\t\t\t</rule>",
            esc_attr(&self.deps.urls.home_url())
        ));
        if add_parent_tags {
            rules.push_str(
                "\n\t\t\t</rules>\n\t\t</rewrite>\n\t</system.webServer>\n</configuration>",
            );
        }
        Ok(text(self.deps.hooks.apply_filters(
            &HookName::from("iis7_url_rewrite_rules"),
            HookValue::Str(rules),
            vec![],
        )?))
    }
}
