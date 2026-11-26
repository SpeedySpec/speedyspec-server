//! Table names, prefixes and `set_blog_id` (Value Object).

use indexmap::IndexMap;

use speedyspec_wp_db_contracts::WpDbError;

/// What table naming depends on outside `wpdb`: `is_multisite()`, `defined('MULTISITE')`, `CUSTOM_USER_TABLE`,
/// `CUSTOM_USER_META_TABLE`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableEnv {
    pub multisite: bool,
    pub multisite_constant: bool,
    pub custom_user_table: Option<String>,
    pub custom_user_meta_table: Option<String>,
}

/// `wpdb::tables()`'s scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableScope {
    All,
    Blog,
    Global,
    MsGlobal,
    Old,
}

impl TableScope {
    pub fn parse(scope: &str) -> Option<TableScope> {
        match scope {
            "all" => Some(TableScope::All),
            "blog" => Some(TableScope::Blog),
            "global" => Some(TableScope::Global),
            "ms_global" => Some(TableScope::MsGlobal),
            "old" => Some(TableScope::Old),
            _ => None,
        }
    }
}

fn list(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

/// `wpdb`'s table lists, prefixes, blog and network ids, and the table-name properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableNames {
    tables: Vec<String>,
    old_tables: Vec<String>,
    global_tables: Vec<String>,
    ms_global_tables: Vec<String>,
    old_ms_global_tables: Vec<String>,
    prefix: String,
    base_prefix: Option<String>,
    blogid: i64,
    siteid: i64,
    names: IndexMap<String, String>,
}

impl Default for TableNames {
    fn default() -> Self {
        TableNames::new()
    }
}

impl TableNames {
    pub fn new() -> TableNames {
        TableNames {
            tables: list(&[
                "posts",
                "comments",
                "links",
                "options",
                "postmeta",
                "terms",
                "term_taxonomy",
                "term_relationships",
                "termmeta",
                "commentmeta",
            ]),
            old_tables: list(&["categories", "post2cat", "link2cat"]),
            global_tables: list(&["users", "usermeta"]),
            ms_global_tables: list(&[
                "blogs",
                "blogmeta",
                "signups",
                "site",
                "sitemeta",
                "registration_log",
            ]),
            old_ms_global_tables: list(&["sitecategories"]),
            prefix: String::new(),
            base_prefix: None,
            blogid: 0,
            siteid: 0,
            names: IndexMap::new(),
        }
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }
    pub fn set_prefix_value(&mut self, prefix: String) {
        self.prefix = prefix;
    }
    pub fn base_prefix(&self) -> Option<&str> {
        self.base_prefix.as_deref()
    }
    pub fn set_base_prefix(&mut self, base: Option<String>) {
        self.base_prefix = base;
    }
    pub fn blogid(&self) -> i64 {
        self.blogid
    }
    pub fn set_blogid(&mut self, id: i64) {
        self.blogid = id;
    }
    pub fn siteid(&self) -> i64 {
        self.siteid
    }
    pub fn set_siteid(&mut self, id: i64) {
        self.siteid = id;
    }

    pub fn tables_list(&self) -> &[String] {
        &self.tables
    }
    pub fn old_tables(&self) -> &[String] {
        &self.old_tables
    }
    pub fn global_tables(&self) -> &[String] {
        &self.global_tables
    }
    pub fn ms_global_tables(&self) -> &[String] {
        &self.ms_global_tables
    }
    pub fn old_ms_global_tables(&self) -> &[String] {
        &self.old_ms_global_tables
    }
    pub fn set_tables_list(&mut self, v: Vec<String>) {
        self.tables = v;
    }
    pub fn set_old_tables(&mut self, v: Vec<String>) {
        self.old_tables = v;
    }
    pub fn set_global_tables(&mut self, v: Vec<String>) {
        self.global_tables = v;
    }
    pub fn set_ms_global_tables(&mut self, v: Vec<String>) {
        self.ms_global_tables = v;
    }
    pub fn set_old_ms_global_tables(&mut self, v: Vec<String>) {
        self.old_ms_global_tables = v;
    }

    /// The property `$wpdb->{logical}`, when set.
    pub fn get(&self, logical: &str) -> Option<&str> {
        self.names.get(logical).map(String::as_str)
    }

    pub fn set(&mut self, logical: &str, name: String) {
        self.names.insert(logical.to_string(), name);
    }

    pub fn unset(&mut self, logical: &str) {
        self.names.shift_remove(logical);
    }

    /// Every table-name property that is set, in order.
    pub fn names(&self) -> &IndexMap<String, String> {
        &self.names
    }

    /// The table's name: the property when set, else the current prefix and the logical name.
    pub fn name(&self, logical: &str) -> String {
        self.get(logical)
            .map_or_else(|| format!("{}{logical}", self.prefix), str::to_string)
    }

    /// `wpdb::set_prefix`; returns the old prefix.
    pub fn set_prefix(
        &mut self,
        prefix: &str,
        set_table_names: bool,
        env: &TableEnv,
    ) -> Result<String, WpDbError> {
        if prefix
            .bytes()
            .any(|b| !(b.is_ascii_alphanumeric() || b == b'_'))
        {
            return Err(WpDbError::InvalidPrefix);
        }
        let mut old = if env.multisite {
            String::new()
        } else {
            prefix.to_string()
        };
        if let Some(base) = &self.base_prefix {
            old = base.clone();
        }
        self.base_prefix = Some(prefix.to_string());
        if set_table_names {
            for (table, name) in self.tables(TableScope::Global, true, 0, env) {
                self.set(&table, name);
            }
            if env.multisite && self.blogid == 0 {
                return Ok(old);
            }
            self.prefix = self.get_blog_prefix(None, env);
            self.set_scoped_names(env);
        }
        Ok(old)
    }

    fn set_scoped_names(&mut self, env: &TableEnv) {
        for scope in [TableScope::Blog, TableScope::Old] {
            for (table, name) in self.tables(scope, true, 0, env) {
                self.set(&table, name);
            }
        }
    }

    /// `wpdb::set_blog_id`; returns the old blog id.
    pub fn set_blog_id(&mut self, blog_id: i64, network_id: i64, env: &TableEnv) -> i64 {
        if network_id != 0 {
            self.siteid = network_id;
        }
        let old = self.blogid;
        self.blogid = blog_id;
        self.prefix = self.get_blog_prefix(None, env);
        self.set_scoped_names(env);
        old
    }

    /// `wpdb::get_blog_prefix`.
    pub fn get_blog_prefix(&self, blog_id: Option<i64>, env: &TableEnv) -> String {
        let base = self.base_prefix.clone().unwrap_or_default();
        if !env.multisite {
            return base;
        }
        let blog_id = blog_id.unwrap_or(self.blogid);
        if env.multisite_constant && (blog_id == 0 || blog_id == 1) {
            base
        } else {
            format!("{base}{blog_id}_")
        }
    }

    /// `wpdb::tables`: `(logical, name)` pairs; without `prefix` each name is the logical name.
    pub fn tables(
        &self,
        scope: TableScope,
        prefix: bool,
        blog_id: i64,
        env: &TableEnv,
    ) -> Vec<(String, String)> {
        let mut tables: Vec<String> = match scope {
            TableScope::All => {
                let mut t = [self.global_tables.clone(), self.tables.clone()].concat();
                if env.multisite {
                    t.extend(self.ms_global_tables.iter().cloned());
                }
                t
            }
            TableScope::Blog => self.tables.clone(),
            TableScope::Global => {
                let mut t = self.global_tables.clone();
                if env.multisite {
                    t.extend(self.ms_global_tables.iter().cloned());
                }
                t
            }
            TableScope::MsGlobal => self.ms_global_tables.clone(),
            TableScope::Old => {
                let mut t = self.old_tables.clone();
                if env.multisite {
                    t.extend(self.old_ms_global_tables.iter().cloned());
                }
                t
            }
        };
        if !prefix {
            return tables.drain(..).map(|t| (t.clone(), t)).collect();
        }
        let blog_id = if blog_id == 0 { self.blogid } else { blog_id };
        let blog_prefix = self.get_blog_prefix(Some(blog_id), env);
        let base = self.base_prefix.clone().unwrap_or_default();
        let mut out: IndexMap<String, String> = IndexMap::new();
        for table in tables {
            let global =
                self.global_tables.contains(&table) || self.ms_global_tables.contains(&table);
            let name = if global {
                format!("{base}{table}")
            } else {
                format!("{blog_prefix}{table}")
            };
            // `$tables[$table] = ..` on an existing key keeps its position.
            out.insert(table, name);
        }
        if let (Some(users), Some(custom)) = (out.get_mut("users"), &env.custom_user_table) {
            *users = custom.clone();
        }
        if let (Some(meta), Some(custom)) = (out.get_mut("usermeta"), &env.custom_user_meta_table) {
            *meta = custom.clone();
        }
        out.into_iter().collect()
    }
}
