//! `SqlOptionRepository` (Repository): one statement per `$wpdb` call `option.php` makes, built with the query
//! builder (never text), on the table `wpdb` names now (so `set_blog_id` is followed).

use std::rc::Rc;

use speedyspec_data::{DbValue, Statement};
use speedyspec_data_query::Query;
use speedyspec_wp_db::{QueryOutcome, WpDb, WpDbError, WpRow, WpValue};
use speedyspec_wp_options::Option as OptionEntity;
use speedyspec_wp_options::{
    Autoload, OptionName, OptionRepository, OptionsError, SerializedValue,
};

use crate::mapper::value;

pub struct SqlOptionRepository {
    wpdb: Rc<WpDb>,
    engine: String,
}

/// A failure `wpdb` hides is the empty answer; a hook's failure (a PHP exception from a `query` callback) is not.
enum Outcome {
    Rows(Rc<Vec<WpRow>>),
    Affected(u64),
    Hidden,
}

fn field(row: &WpRow, name: &str) -> Option<Vec<u8>> {
    row.fields
        .iter()
        .find(|(n, _)| n == name)
        .and_then(|(_, v)| v.clone())
}

fn text(b: Vec<u8>) -> String {
    String::from_utf8_lossy(&b).into_owned()
}

fn names(names: &[String]) -> Vec<DbValue> {
    names.iter().map(|n| DbValue::Text(n.clone())).collect()
}

impl SqlOptionRepository {
    pub fn new(wpdb: Rc<WpDb>, engine: String) -> SqlOptionRepository {
        SqlOptionRepository { wpdb, engine }
    }

    fn table(&self) -> String {
        self.wpdb.table_name("options")
    }

    fn send(
        &self,
        built: Result<Statement, speedyspec_data::DbError>,
        quiet: bool,
    ) -> Result<Outcome, OptionsError> {
        let stmt = built.map_err(|e| OptionsError::Db(e.to_string()))?;
        let previous = quiet.then(|| self.wpdb.suppress_errors(true));
        let result = self.wpdb.query_statement(stmt);
        if let Some(previous) = previous {
            self.wpdb.suppress_errors(previous);
        }
        match result {
            Ok(QueryOutcome::Rows(rows)) => Ok(Outcome::Rows(rows)),
            Ok(QueryOutcome::Affected { rows, .. }) => Ok(Outcome::Affected(rows)),
            Ok(QueryOutcome::Done) => Ok(Outcome::Affected(0)),
            Err(WpDbError::Hook(e)) => Err(OptionsError::Hook(e)),
            Err(_) => Ok(Outcome::Hidden),
        }
    }

    fn rows(
        &self,
        built: Result<Statement, speedyspec_data::DbError>,
        quiet: bool,
    ) -> Result<Rc<Vec<WpRow>>, OptionsError> {
        Ok(match self.send(built, quiet)? {
            Outcome::Rows(rows) => rows,
            _ => Rc::new(Vec::new()),
        })
    }

    fn affected(
        &self,
        built: Result<Statement, speedyspec_data::DbError>,
    ) -> Result<u64, OptionsError> {
        Ok(match self.send(built, false)? {
            Outcome::Affected(n) => n,
            _ => 0,
        })
    }

    fn pairs(rows: &[WpRow]) -> Vec<(String, SerializedValue)> {
        rows.iter()
            .filter_map(|r| {
                Some((
                    text(field(r, "option_name")?),
                    SerializedValue::new(field(r, "option_value").unwrap_or_default()),
                ))
            })
            .collect()
    }

    /// `wpdb::process_fields` over the fields an `update` or `delete` sends; false where `wpdb` would return false.
    fn fields_pass(&self, table: &str, data: &[(String, WpValue)]) -> Result<bool, OptionsError> {
        match self.wpdb.process_fields(table, data, None) {
            Ok(_) => Ok(true),
            Err(WpDbError::Hook(e)) => Err(OptionsError::Hook(e)),
            Err(_) => Ok(false),
        }
    }
}

fn wp(value: &SerializedValue) -> WpValue {
    value.to_value()
}

impl OptionRepository for SqlOptionRepository {
    fn engine(&self) -> String {
        self.engine.clone()
    }

    fn value(
        &self,
        name: &OptionName,
        quiet: bool,
    ) -> Result<Option<SerializedValue>, OptionsError> {
        let rows = self.rows(
            Query::table(&self.table())
                .select(&["option_value"])
                .where_eq("option_name", name.as_str())
                .limit(1)
                .build(),
            quiet,
        )?;
        Ok(rows
            .first()
            .map(|r| SerializedValue::new(field(r, "option_value").unwrap_or_default())))
    }

    fn autoload_of(&self, name: &OptionName) -> Result<Option<Autoload>, OptionsError> {
        let rows = self.rows(
            Query::table(&self.table())
                .select(&["autoload"])
                .where_eq("option_name", name.as_str())
                .limit(1)
                .build(),
            false,
        )?;
        Ok(rows
            .first()
            .map(|r| Autoload::parse(&field(r, "autoload").map(text).unwrap_or_default())))
    }

    fn values_of(&self, names: &[String]) -> Result<Vec<(String, SerializedValue)>, OptionsError> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let rows = self.rows(
            Query::table(&self.table())
                .select(&["option_name", "option_value"])
                .where_in("option_name", self::names(names))
                .build(),
            false,
        )?;
        Ok(Self::pairs(&rows))
    }

    fn autoloaded(
        &self,
        values: &[String],
    ) -> Result<Vec<(String, SerializedValue)>, OptionsError> {
        let rows = self.rows(
            Query::table(&self.table())
                .select(&["option_name", "option_value"])
                .where_in("autoload", names(values))
                .build(),
            true,
        )?;
        Ok(Self::pairs(&rows))
    }

    fn all(&self) -> Result<Vec<(String, SerializedValue)>, OptionsError> {
        let rows = self.rows(
            Query::table(&self.table())
                .select(&["option_name", "option_value"])
                .build(),
            true,
        )?;
        Ok(Self::pairs(&rows))
    }

    /// One `SELECT` of every name's autoload; the comparison with each group's value is made here, as MySQL's
    /// case-insensitive collation makes it, so no value is written into SQL text.
    fn needing_autoload_change(
        &self,
        groups: &[(Autoload, Vec<String>)],
    ) -> Result<Vec<String>, OptionsError> {
        let all: Vec<String> = groups.iter().flat_map(|(_, n)| n.iter().cloned()).collect();
        if all.is_empty() {
            return Ok(Vec::new());
        }
        let rows = self.rows(
            Query::table(&self.table())
                .select(&["option_name", "autoload"])
                .where_in("option_name", names(&all))
                .build(),
            false,
        )?;
        let mut changed = Vec::new();
        for row in rows.iter() {
            let (Some(stored_name), stored) = (
                field(row, "option_name").map(text),
                field(row, "autoload").map(text),
            ) else {
                continue;
            };
            let stored = stored.unwrap_or_default();
            let differs = groups.iter().any(|(autoload, names)| {
                names
                    .iter()
                    .any(|n| n.to_lowercase() == stored_name.to_lowercase())
                    && !stored.eq_ignore_ascii_case(autoload.as_str())
            });
            if differs && !changed.contains(&stored_name) {
                changed.push(stored_name);
            }
        }
        Ok(changed)
    }

    fn set_autoload(&self, names: &[String], autoload: &Autoload) -> Result<u64, OptionsError> {
        if names.is_empty() {
            return Ok(0);
        }
        self.affected(
            Query::update(&self.table())
                .set("autoload", autoload.as_str())
                .where_in("option_name", self::names(names))
                .build(),
        )
    }

    /// `wpdb::query` refuses an `INSERT` holding text the table's charset cannot hold (nothing is sent); the same
    /// field check stands in for it here.
    fn add(&self, option: &OptionEntity) -> Result<u64, OptionsError> {
        let table = self.table();
        let data = vec![
            (
                "option_name".to_string(),
                WpValue::Str(option.name.as_str().to_string()),
            ),
            ("option_value".to_string(), wp(&option.value)),
            (
                "autoload".to_string(),
                WpValue::Str(option.autoload.as_str().to_string()),
            ),
        ];
        // `wpdb::query` checks only a query that is not plain ASCII (`check_ascii`).
        let ascii = option.name.as_str().is_ascii()
            && option.value.as_bytes().is_ascii()
            && option.autoload.as_str().is_ascii();
        if !ascii && !self.fields_pass(&table, &data)? {
            return Ok(0);
        }
        let columns = ["option_name", "option_value", "autoload"];
        self.affected(
            Query::insert_into(&table, &columns)
                .values(crate::OptionMapper::to_values(option))
                .on_duplicate_update(&columns)
                .build(),
        )
    }

    fn update(
        &self,
        name: &OptionName,
        value: Option<&SerializedValue>,
        autoload: Option<&Autoload>,
    ) -> Result<u64, OptionsError> {
        let table = self.table();
        let mut data = vec![("option_value".to_string(), value.map_or(WpValue::Null, wp))];
        if let Some(a) = autoload {
            data.push(("autoload".to_string(), WpValue::Str(a.as_str().to_string())));
        }
        let condition = vec![(
            "option_name".to_string(),
            WpValue::Str(name.as_str().to_string()),
        )];
        if !self.fields_pass(&table, &data)? || !self.fields_pass(&table, &condition)? {
            return Ok(0);
        }
        let mut query = Query::update(&table);
        query.set("option_value", value.map_or(DbValue::Null, self::value_of));
        if let Some(a) = autoload {
            query.set("autoload", a.as_str());
        }
        query.where_eq("option_name", name.as_str());
        self.affected(query.build())
    }

    fn remove(&self, name: &OptionName) -> Result<u64, OptionsError> {
        let table = self.table();
        let condition = vec![(
            "option_name".to_string(),
            WpValue::Str(name.as_str().to_string()),
        )];
        if !self.fields_pass(&table, &condition)? {
            return Ok(0);
        }
        self.affected(
            Query::delete_from(&table)
                .where_eq("option_name", name.as_str())
                .build(),
        )
    }
}

fn value_of(v: &SerializedValue) -> DbValue {
    value(v)
}
