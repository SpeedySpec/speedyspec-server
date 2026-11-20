//! MySQL's introspector: schema answers from `information_schema` and `SELECT VERSION()`.

use speedyspec_data::{ColumnDef, DbError, DbValue, IndexDef, Introspector, Rows, SqlStore};
use speedyspec_data_query::parse_one;

use crate::MySqlStore;

fn text(rows: &Rows, row: usize, col: usize) -> Option<String> {
    rows.rows
        .get(row)?
        .get_index(col)?
        .to_mysql_text()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn bound(sql: &str, params: Vec<DbValue>) -> Result<speedyspec_data::Statement, DbError> {
    let (tree, _) = parse_one(sql)?.into_parts();
    Ok(speedyspec_data::Statement::new(tree, params))
}

impl Introspector for MySqlStore {
    fn server_version(&self) -> Result<String, DbError> {
        let rows = self.query(&parse_one("SELECT VERSION()")?)?;
        Ok(text(&rows, 0, 0).unwrap_or_default())
    }

    fn tables(&self, like: Option<&str>) -> Result<Vec<String>, DbError> {
        let rows = match like {
            Some(pattern) => self.query(&bound(
                "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME LIKE ? ORDER BY TABLE_NAME",
                vec![pattern.into()],
            )?)?,
            None => self.query(&parse_one(
                "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() ORDER BY TABLE_NAME",
            )?)?,
        };
        Ok((0..rows.rows.len())
            .filter_map(|i| text(&rows, i, 0))
            .collect())
    }

    fn columns(&self, table: &str) -> Result<Vec<ColumnDef>, DbError> {
        let rows = self.query(&bound(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, EXTRA, COLLATION_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            vec![table.into()],
        )?)?;
        Ok((0..rows.rows.len())
            .map(|i| ColumnDef {
                name: text(&rows, i, 0).unwrap_or_default(),
                sql_type: text(&rows, i, 1).unwrap_or_default(),
                nullable: text(&rows, i, 2).as_deref() == Some("YES"),
                key: text(&rows, i, 3).unwrap_or_default(),
                default: text(&rows, i, 4),
                extra: text(&rows, i, 5).unwrap_or_default(),
                collation: text(&rows, i, 6),
            })
            .collect())
    }

    fn indexes(&self, table: &str) -> Result<Vec<IndexDef>, DbError> {
        let rows = self.query(&bound(
            "SELECT INDEX_NAME, NON_UNIQUE, COLUMN_NAME, SUB_PART FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? ORDER BY INDEX_NAME = 'PRIMARY' DESC, INDEX_NAME, SEQ_IN_INDEX",
            vec![table.into()],
        )?)?;
        let mut out: Vec<IndexDef> = Vec::new();
        for i in 0..rows.rows.len() {
            let name = text(&rows, i, 0).unwrap_or_default();
            let column = (
                text(&rows, i, 2).unwrap_or_default(),
                text(&rows, i, 3).and_then(|s| s.parse().ok()),
            );
            match out.last_mut() {
                Some(index) if index.name == name => index.columns.push(column),
                _ => out.push(IndexDef {
                    name,
                    unique: text(&rows, i, 1).as_deref() == Some("0"),
                    columns: vec![column],
                }),
            }
        }
        Ok(out)
    }

    fn table_collation(&self, table: &str) -> Result<Option<String>, DbError> {
        let rows = self.query(&bound(
            "SELECT TABLE_COLLATION FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?",
            vec![table.into()],
        )?)?;
        Ok(text(&rows, 0, 0))
    }
}
