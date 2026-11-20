//! Connection, charset, `sql_mode` and server methods. The connection itself belongs to the engine and lives for
//! the worker; these methods run their session statements on the store directly, uncounted and unfiltered, as
//! WordPress uses `mysqli_*` for them.

use std::cmp::Ordering;

use speedyspec_data::{DbError, SqlStore};
use speedyspec_data_query::parse_one;
use speedyspec_hooks::Key;
use speedyspec_wp_hooks::{Count, HookName};

use crate::api::html_escape;
use crate::caps::version_compare;
use crate::escape::real_escape;
use crate::php::to_bytes;
use crate::{CharsetRules, DbHost, ServerCaps, WpDb, WpDbError, WpValue};

fn literal(s: &str) -> String {
    format!("'{}'", String::from_utf8_lossy(&real_escape(s.as_bytes())))
}

impl WpDb {
    fn constant_text(&self, name: &str) -> Option<String> {
        self.host
            .constant(name)
            .and_then(|v| to_bytes(&v))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    fn session(&self, sql: &str) -> Result<speedyspec_data::Rows, WpDbError> {
        Ok(self.store.query(&parse_one(sql)?)?)
    }

    fn caps(&self) -> Result<ServerCaps, WpDbError> {
        if let Some(caps) = self.settings.borrow().caps.clone() {
            return Ok(caps);
        }
        let caps = ServerCaps::new(&self.store.introspect().server_version()?);
        self.settings.borrow_mut().caps = Some(caps.clone());
        Ok(caps)
    }

    /// `wpdb::db_connect`: the engine's connection is checked and the session set up.
    pub fn db_connect(&self, allow_bail: bool) -> Result<bool, WpDbError> {
        if self.host.constant("WP_SETUP_CONFIG").is_some() {
            return Ok(false);
        }
        match self.caps() {
            Ok(_) => {}
            Err(WpDbError::Db(DbError::Connection { .. })) => {
                self.modify(|s| s.dbh_present = false);
                if allow_bail {
                    self.bail(&self.connect_error_message(), "db_connect_fail");
                    self.host.dead_db();
                }
                return Ok(false);
            }
            Err(e) => return Err(e),
        }
        self.modify(|s| s.dbh_present = true);
        if !self.settings.borrow().has_connected {
            self.init_charset();
        }
        self.settings.borrow_mut().has_connected = true;
        self.set_charset(None, None)?;
        self.modify(|s| s.ready = true);
        self.set_sql_mode(&[])?;
        let name = self.config.name.clone();
        self.select(&name)?;
        Ok(true)
    }

    fn connect_error_message(&self) -> String {
        let t = |s: &str| self.host.translate(s);
        format!(
            "<h1>{}</h1>\n<p>{}</p>\n<ul>\n<li>{}</li>\n<li>{}</li>\n<li>{}</li>\n</ul>\n<p>{}</p>\n",
            t("Error establishing a database connection"),
            t("This either means that the username and password information in your %1$s file is incorrect or that contact with the database server at %2$s could not be established. This could mean your host&#8217;s database server is down.")
                .replacen("%1$s", "<code>wp-config.php</code>", 1)
                .replacen("%2$s", &format!("<code>{}</code>", html_escape(&self.config.host)), 1),
            t("Are you sure you have the correct username and password?"),
            t("Are you sure you have typed the correct hostname?"),
            t("Are you sure the database server is running?"),
            t("If you are unsure what these terms mean you should probably contact your host. If you still need help you can always visit the <a href=\"%s\">WordPress support forums</a>.")
                .replacen("%s", &t("https://wordpress.org/support/forums/"), 1),
        )
    }

    /// `wpdb::init_charset`.
    pub fn init_charset(&self) {
        let mut charset = String::new();
        let mut collate = String::new();
        let db_collate = self
            .constant_text("DB_COLLATE")
            .or_else(|| self.config.collate.clone());
        if self.host.is_multisite() {
            charset = "utf8".into();
            collate = db_collate
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| "utf8_general_ci".into());
        } else if let Some(c) = db_collate {
            collate = c;
        }
        if let Some(c) = self
            .constant_text("DB_CHARSET")
            .or_else(|| self.config.charset.clone())
        {
            charset = c;
        }
        let caps = self.caps().unwrap_or_else(|_| ServerCaps::new(""));
        let (charset, collate) = CharsetRules::determine_charset(&charset, &collate, &caps);
        let mut settings = self.settings.borrow_mut();
        settings.charset = charset;
        settings.collate = collate;
    }

    pub fn charset(&self) -> String {
        self.settings.borrow().charset.clone()
    }

    pub fn collate(&self) -> String {
        self.settings.borrow().collate.clone()
    }

    pub fn set_charset_value(&self, charset: String) {
        self.settings.borrow_mut().charset = charset;
    }

    pub fn set_collate(&self, collate: String) {
        self.settings.borrow_mut().collate = collate;
    }

    /// `wpdb::set_charset`: `SET NAMES` on the store, skipped when it is what the engine already set.
    pub fn set_charset(
        &self,
        charset: Option<&str>,
        collate: Option<&str>,
    ) -> Result<(), WpDbError> {
        let charset = charset.map_or_else(|| self.charset(), str::to_string);
        let collate = collate.map_or_else(|| self.collate(), str::to_string);
        if charset.is_empty() || !self.caps()?.has_cap("collation") {
            return Ok(());
        }
        // `mysqli_set_charset()` first: it changes the connection's charset even when the `COLLATE` that follows
        // names a collation of another charset and fails, as it does when a caller switches charsets.
        if self.caps()?.has_cap("set_charset") {
            self.store
                .execute(&parse_one(&format!("SET NAMES {}", literal(&charset)))?)?;
        }
        if !collate.is_empty() {
            let sql = format!(
                "SET NAMES {} COLLATE {}",
                literal(&charset),
                literal(&collate)
            );
            let _ = self.store.execute(&parse_one(&sql)?);
        }
        Ok(())
    }

    /// `wpdb::set_sql_mode`.
    pub fn set_sql_mode(&self, modes: &[String]) -> Result<(), WpDbError> {
        let mut modes: Vec<String> = modes.to_vec();
        if modes.is_empty() {
            let rows = match self.session("SELECT @@SESSION.sql_mode") {
                Ok(rows) => rows,
                Err(_) => return Ok(()),
            };
            let Some(current) = rows
                .rows
                .first()
                .and_then(|r| r.get_index(0))
                .and_then(|v| v.to_mysql_text())
            else {
                return Ok(());
            };
            if current.is_empty() || current == b"0" {
                return Ok(());
            }
            modes = String::from_utf8_lossy(&current)
                .split(',')
                .map(str::to_string)
                .collect();
        }
        let defaults = self.settings.borrow().incompatible_modes.clone();
        let hook = HookName::from("incompatible_sql_modes");
        let incompatible: Vec<String> = if self.hooks.skip_if_empty(&hook, Count::Filters)? {
            defaults
        } else {
            let list = WpValue::Array(
                defaults
                    .into_iter()
                    .enumerate()
                    .map(|(i, m)| (Key::Int(i as i64), WpValue::Str(m)))
                    .collect(),
            );
            match &*self.hooks.apply_filters(&hook, list, Vec::new())?.read() {
                WpValue::Array(items) => items
                    .iter()
                    .filter_map(|(_, v)| to_bytes(v))
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .collect(),
                WpValue::Null => Vec::new(),
                other => to_bytes(other)
                    .map(|b| vec![String::from_utf8_lossy(&b).into_owned()])
                    .unwrap_or_default(),
            }
        };
        modes.retain(|m| !incompatible.contains(m));
        self.store.execute(&parse_one(&format!(
            "SET SESSION sql_mode={}",
            literal(&modes.join(","))
        ))?)?;
        Ok(())
    }

    /// `wpdb::select`.
    pub fn select(&self, db: &str) -> Result<(), WpDbError> {
        let sql = format!("USE `{}`", db.replace('`', "``"));
        if self.store.execute(&parse_one(&sql)?).is_ok() {
            return Ok(());
        }
        self.modify(|s| s.ready = false);
        if self
            .hooks
            .did_action(&HookName::from("template_redirect"))?
            == 0
        {
            let t = |s: &str| self.host.translate(s);
            let user = html_escape(&self.config.user);
            let name = html_escape(db);
            let message = format!(
                "<h1>{}</h1>\n<p>{}</p>\n<ul>\n<li>{}</li>\n<li>{}</li>\n<li>{}</li>\n</ul>\n<p>{}</p>\n",
                t("Cannot select database"),
                t("The database server could be connected to (which means your username and password is okay) but the %s database could not be selected.")
                    .replacen("%s", &format!("<code>{name}</code>"), 1),
                t("Are you sure it exists?"),
                t("Does the user %1$s have permission to use the %2$s database?")
                    .replacen("%1$s", &format!("<code>{user}</code>"), 1)
                    .replacen("%2$s", &format!("<code>{name}</code>"), 1),
                t("On some systems the name of your database is prefixed with your username, so it would be like <code>username_%1$s</code>. Could that be the problem?")
                    .replacen("%1$s", &name, 1),
                t("If you do not know how to set up a database you should <strong>contact your host</strong>. If all else fails you may find help at the <a href=\"%s\">WordPress support forums</a>.")
                    .replacen("%s", &t("https://wordpress.org/support/forums/"), 1),
            );
            self.bail(&message, "db_select_fail");
        }
        Ok(())
    }

    /// `wpdb::close`: the request's handle is closed; the worker's connection stays open for the next request.
    pub fn close(&self) -> bool {
        if !self.read(|s| s.dbh_present) {
            return false;
        }
        self.modify(|s| {
            s.ready = false;
            s.dbh_present = false;
        });
        self.settings.borrow_mut().has_connected = false;
        true
    }

    /// Sets `reconnect_retries` and the pause between tries (WordPress pauses one second).
    pub fn set_reconnect(&self, retries: u32, delay: std::time::Duration) {
        let mut s = self.settings.borrow_mut();
        s.reconnect_retries = retries;
        s.reconnect_delay = delay;
    }

    pub fn reconnect_retries(&self) -> u32 {
        self.settings.borrow().reconnect_retries
    }

    /// `wpdb::check_connection`.
    pub fn check_connection(&self, allow_bail: bool) -> Result<bool, WpDbError> {
        let alive = |db: &WpDb| match db.session("SELECT 1") {
            Ok(_) => Ok(true),
            Err(WpDbError::Db(DbError::Connection { .. })) => Ok(false),
            Err(e) => Err(e),
        };
        if self.read(|s| s.dbh_present) && alive(self)? {
            return Ok(true);
        }
        let (retries, delay) = {
            let s = self.settings.borrow();
            (s.reconnect_retries, s.reconnect_delay)
        };
        for _ in 0..retries {
            if alive(self)? {
                self.modify(|s| s.dbh_present = true);
                if self.db_connect(false)? {
                    return Ok(true);
                }
            }
            std::thread::sleep(delay);
        }
        if self
            .hooks
            .did_action(&HookName::from("template_redirect"))?
            > 0
            || !allow_bail
        {
            return Ok(false);
        }
        let t = |s: &str| self.host.translate(s);
        let message = format!(
            "<h1>{}</h1>\n<p>{}</p>\n<ul>\n<li>{}</li>\n<li>{}</li>\n</ul>\n<p>{}</p>\n",
            t("Error reconnecting to the database"),
            t("This means that the contact with the database server at %s was lost. This could mean your host&#8217;s database server is down.")
                .replacen("%s", &format!("<code>{}</code>", html_escape(&self.config.host)), 1),
            t("Are you sure the database server is running?"),
            t("Are you sure the database server is not under particularly heavy load?"),
            t("If you are unsure what these terms mean you should probably contact your host. If you still need help you can always visit the <a href=\"%s\">WordPress support forums</a>.")
                .replacen("%s", &t("https://wordpress.org/support/forums/"), 1),
        );
        self.bail(&message, "db_connect_fail");
        self.host.dead_db();
        Ok(false)
    }

    pub fn db_server_info(&self) -> Result<String, WpDbError> {
        Ok(self.caps()?.server_info)
    }

    pub fn db_version(&self) -> Result<String, WpDbError> {
        Ok(self.caps()?.version)
    }

    pub fn has_cap(&self, cap: &str) -> bool {
        self.caps().is_ok_and(|c| c.has_cap(cap))
    }

    /// `wpdb::supports_collation` (deprecated since 3.5.0).
    pub fn supports_collation(&self) -> bool {
        self.host.deprecated_function(
            "supports_collation",
            "3.5.0",
            "wpdb::has_cap( 'collation' )",
        );
        self.has_cap("collation")
    }

    /// `wpdb::get_charset_collate`.
    pub fn get_charset_collate(&self) -> String {
        let (charset, collate) = (self.charset(), self.collate());
        let mut out = String::new();
        if !charset.is_empty() && charset != "0" {
            out = format!("DEFAULT CHARACTER SET {charset}");
        }
        if !collate.is_empty() && collate != "0" {
            out.push_str(&format!(" COLLATE {collate}"));
        }
        out
    }

    /// `wpdb::check_database_version` with `$required_mysql_version` and WordPress's version given.
    pub fn check_database_version(
        &self,
        required: &str,
        wp_version: &str,
    ) -> Result<(), WpDbError> {
        if version_compare(&self.db_version()?, required) == Ordering::Less {
            let message = self
                .host
                .translate("<strong>Error:</strong> WordPress %1$s requires MySQL %2$s or higher")
                .replacen("%1$s", wp_version, 1)
                .replacen("%2$s", required, 1);
            return Err(WpDbError::Bailed {
                code: "database_version".into(),
                message,
            });
        }
        Ok(())
    }

    /// `wpdb::parse_db_host`.
    pub fn parse_db_host(&self, host: &str) -> Option<DbHost> {
        DbHost::parse(host)
    }
}
