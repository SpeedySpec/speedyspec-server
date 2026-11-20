//! `prepare`, escaping, the insert/update/delete helpers, `process_fields`, and the readers over `last_result`.

use speedyspec_hooks::Key;

use crate::{
    CharsetRules, Field, FieldFormats, OutputShape, Prepare, QueryOutcome, Shaped, WpDb, WpDbError,
    WpRow, WpValue, delete_sql, insert_sql, update_sql, var,
};

/// mysqli's numeric code for a field type, from the name `ColumnInfo::sql_type` carries.
fn type_code(name: &str) -> i64 {
    let name = name.trim_start_matches("MYSQL_TYPE_");
    match name {
        "DECIMAL" => 0,
        "TINY" => 1,
        "SHORT" => 2,
        "LONG" => 3,
        "FLOAT" => 4,
        "DOUBLE" => 5,
        "NULL" => 6,
        "TIMESTAMP" => 7,
        "LONGLONG" => 8,
        "INT24" => 9,
        "DATE" => 10,
        "TIME" => 11,
        "DATETIME" => 12,
        "YEAR" => 13,
        "NEWDATE" => 14,
        "VARCHAR" => 15,
        "BIT" => 16,
        "TIMESTAMP2" => 17,
        "DATETIME2" => 18,
        "TIME2" => 19,
        "TYPED_ARRAY" => 20,
        "VECTOR" => 242,
        "JSON" => 245,
        "NEWDECIMAL" => 246,
        "ENUM" => 247,
        "SET" => 248,
        "TINY_BLOB" => 249,
        "MEDIUM_BLOB" => 250,
        "LONG_BLOB" => 251,
        "BLOB" => 252,
        "VAR_STRING" => 253,
        "STRING" => 254,
        "GEOMETRY" => 255,
        _ => 253,
    }
}

fn int_value(n: u64) -> WpValue {
    WpValue::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

impl WpDb {
    /// `wpdb::prepare`; each notice goes to the host's `_doing_it_wrong`.
    pub fn prepare(&self, query: Option<&[u8]>, args: &[WpValue]) -> Option<Vec<u8>> {
        let allow = self.settings.borrow().allow_unsafe_unquoted_parameters;
        let outcome = Prepare {
            escape: &self.escape,
            allow_unsafe_unquoted_parameters: allow,
        }
        .prepare(query, args);
        for notice in &outcome.notices {
            self.host.doing_it_wrong(notice);
        }
        outcome.sql
    }

    /// `wpdb::_escape`.
    pub fn escape(&self, data: &WpValue) -> WpValue {
        crate::escape(data, &self.escape)
    }

    pub fn esc_like(&self, text: &[u8]) -> Vec<u8> {
        crate::esc_like(text)
    }

    pub fn quote_identifier(&self, identifier: &str) -> String {
        crate::quote_identifier(identifier)
    }

    pub fn field_types(&self) -> indexmap::IndexMap<String, String> {
        self.settings.borrow().field_types.clone()
    }

    pub fn set_field_types(&self, types: indexmap::IndexMap<String, String>) {
        self.settings.borrow_mut().field_types = types;
    }

    /// `wpdb::process_fields`.
    pub fn process_fields(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
    ) -> Result<Vec<Field>, WpDbError> {
        let types = self.field_types();
        let mut fields = FieldFormats {
            field_types: &types,
        }
        .process_field_formats(data, format);
        for field in fields.iter_mut() {
            if field.format == "%d" || field.format == "%f" {
                continue;
            }
            field.charset = self.get_col_charset(table, &field.name)?;
        }
        for field in fields.iter_mut() {
            if field.format == "%d" || field.format == "%f" {
                continue;
            }
            field.length = self.get_col_length(table, &field.name)?;
        }
        let mut converted = fields.clone();
        self.strip_context(|cx| {
            CharsetRules::strip_invalid_text(&mut converted, cx, &mut |sql| self.convert_row(sql))
        })?;
        let problems: Vec<&str> = fields
            .iter()
            .zip(&converted)
            .filter(|(a, b)| a.value != b.value)
            .map(|(a, _)| a.name.as_str())
            .collect();
        if problems.is_empty() {
            return Ok(fields);
        }
        let message = if problems.len() == 1 {
            self.host
                .translate("WordPress database error: Processing the value for the following field failed: %s. The supplied value may be too long or contains invalid data.")
                .replacen("%s", problems[0], 1)
        } else {
            self.host
                .translate("WordPress database error: Processing the values for the following fields failed: %s. The supplied values may be too long or contain invalid data.")
                .replacen("%s", &problems.join(", "), 1)
        };
        let m = message.clone();
        self.modify(|s| s.last_error = m);
        Err(WpDbError::ProcessFields(message))
    }

    fn run_helper(&self, sql: String, values: Vec<WpValue>) -> Result<u64, WpDbError> {
        let list = WpValue::Array(
            values
                .into_iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), v))
                .collect(),
        );
        let prepared = self
            .prepare(Some(sql.as_bytes()), &[list])
            .unwrap_or_default();
        self.modify(|s| s.check_current_query = false);
        match self.query_bytes(&prepared)? {
            QueryOutcome::Affected { rows, .. } => Ok(rows),
            QueryOutcome::Rows(rows) => Ok(rows.len() as u64),
            QueryOutcome::Done => Ok(1),
        }
    }

    fn insert_replace(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
        verb: &str,
    ) -> Result<u64, WpDbError> {
        self.modify(|s| s.insert_id = 0);
        let fields = self.process_fields(table, data, format)?;
        let (sql, values) = insert_sql(table, &fields, verb);
        self.run_helper(sql, values)
    }

    /// `wpdb::_insert_replace_helper`: `type` must be `INSERT` or `REPLACE` in any case.
    pub fn insert_replace_helper(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
        verb: &str,
    ) -> Result<u64, WpDbError> {
        self.modify(|s| s.insert_id = 0);
        if !["INSERT", "REPLACE"].contains(&verb.to_uppercase().as_str()) {
            return Err(WpDbError::ProcessFields(String::new()));
        }
        self.insert_replace(table, data, format, verb)
    }

    /// `wpdb::insert`.
    pub fn insert(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        self.insert_replace(table, data, format, "INSERT")
    }

    /// `wpdb::replace`.
    pub fn replace(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        self.insert_replace(table, data, format, "REPLACE")
    }

    /// `wpdb::update`.
    pub fn update(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        conditions: &[(String, WpValue)],
        format: Option<&[String]>,
        where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        let set = self.process_fields(table, data, format)?;
        let conditions = self.process_fields(table, conditions, where_format)?;
        let (sql, values) = update_sql(table, &set, &conditions);
        self.run_helper(sql, values)
    }

    /// `wpdb::delete`.
    pub fn delete(
        &self,
        table: &str,
        conditions: &[(String, WpValue)],
        where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        let conditions = self.process_fields(table, conditions, where_format)?;
        let (sql, values) = delete_sql(table, &conditions);
        self.run_helper(sql, values)
    }

    // ----- readers

    fn rows(&self) -> std::rc::Rc<Vec<WpRow>> {
        self.read(|s| s.last_result.clone())
    }

    /// The SQL a reader runs first, as `get_var` and its siblings decide it (`check_safe_collation` may clear
    /// `check_current_query`); `None` when there is none. The caller runs it through `query` (or the `Query` handler).
    pub fn reader_query(&self, query: Option<&[u8]>) -> Option<Vec<u8>> {
        let q = query.filter(|q| !q.is_empty() && *q != b"0")?;
        if self.read(|s| s.check_current_query) && self.check_safe_collation(q) {
            self.modify(|s| s.check_current_query = false);
        }
        Some(q.to_vec())
    }

    /// Runs the reader's SQL first, as `get_var` and its siblings do.
    fn run_reader_sql(&self, query: Option<&[u8]>) -> Result<(), WpDbError> {
        if let Some(q) = self.reader_query(query) {
            // A failed query leaves `last_result` empty; the readers then answer from it, as WordPress does.
            let _ = self.query_bytes(&q);
        }
        Ok(())
    }

    /// Sets `func_call` as a reader does before its query.
    pub fn set_func_call(&self, call: String) {
        self.modify(|s| s.func_call = Some(call));
    }

    pub fn get_var(&self, x: usize, y: usize) -> Option<Vec<u8>> {
        var(&self.rows(), x, y)
    }

    /// `wpdb::get_var` with its optional SQL.
    pub fn get_var_sql(
        &self,
        query: Option<&[u8]>,
        x: usize,
        y: usize,
    ) -> Result<Option<Vec<u8>>, WpDbError> {
        let call = format!(
            "$db->get_var(\"{}\", {x}, {y})",
            String::from_utf8_lossy(query.unwrap_or_default())
        );
        self.modify(|s| s.func_call = Some(call));
        self.run_reader_sql(query)?;
        Ok(self.get_var(x, y))
    }

    /// The row at `y` in a shape; a bad shape prints WordPress's error.
    pub fn get_row(&self, output: Option<OutputShape>, y: usize) -> Option<WpRow> {
        let rows = self.rows();
        let row = rows.get(y)?;
        match output {
            Some(_) => Some(row.clone()),
            None => {
                self.print_error(Some(
                    " $db->get_row(string query, output type, int offset) -- Output type must be one of: OBJECT, ARRAY_A, ARRAY_N",
                ));
                None
            }
        }
    }

    /// `wpdb::get_row` with its SQL (`None` without SQL, as WordPress returns null).
    pub fn get_row_sql(
        &self,
        query: Option<&[u8]>,
        output: Option<OutputShape>,
        output_name: &str,
        y: usize,
    ) -> Result<Option<WpRow>, WpDbError> {
        let call = format!(
            "$db->get_row(\"{}\",{output_name},{y})",
            String::from_utf8_lossy(query.unwrap_or_default())
        );
        self.modify(|s| s.func_call = Some(call));
        if query.is_none_or(|q| q.is_empty() || q == b"0") {
            return Ok(None);
        }
        self.run_reader_sql(query)?;
        Ok(self.get_row(output, y))
    }

    pub fn get_col(&self, x: usize) -> Vec<Option<Vec<u8>>> {
        let rows = self.rows();
        (0..rows.len()).map(|y| var(&rows, x, y)).collect()
    }

    pub fn get_col_sql(
        &self,
        query: Option<&[u8]>,
        x: usize,
    ) -> Result<Vec<Option<Vec<u8>>>, WpDbError> {
        self.run_reader_sql(query)?;
        Ok(self.get_col(x))
    }

    pub fn get_results(&self, output: Option<OutputShape>) -> Option<Shaped> {
        output.map(|shape| Shaped::of(&self.rows(), shape))
    }

    /// `wpdb::get_results` with its SQL (`None` without SQL or with an unknown shape).
    pub fn get_results_sql(
        &self,
        query: Option<&[u8]>,
        output: Option<OutputShape>,
    ) -> Result<Option<Shaped>, WpDbError> {
        let name = match output {
            Some(OutputShape::Object) | None => "OBJECT",
            Some(OutputShape::ObjectK) => "OBJECT_K",
            Some(OutputShape::ArrayA) => "ARRAY_A",
            Some(OutputShape::ArrayN) => "ARRAY_N",
        };
        let call = format!(
            "$db->get_results(\"{}\", {name})",
            String::from_utf8_lossy(query.unwrap_or_default())
        );
        self.modify(|s| s.func_call = Some(call));
        if query.is_none_or(|q| q.is_empty() || q == b"0") {
            return Ok(None);
        }
        self.run_reader_sql(query)?;
        Ok(self.get_results(output))
    }

    /// `wpdb::get_col_info`: one property of every column (`col_offset` -1) or of one.
    pub fn get_col_info(&self, info_type: &str, col_offset: i64) -> WpValue {
        let Some(columns) = self.read(|s| s.col_info.clone()).filter(|c| !c.is_empty()) else {
            return WpValue::Null;
        };
        let db = self.config.name.clone();
        let one = |c: &speedyspec_data::ColumnInfo| -> WpValue {
            match info_type {
                "name" => WpValue::Str(c.name.clone()),
                "orgname" => WpValue::Str(c.org_name.clone()),
                "table" => WpValue::Str(c.table.clone()),
                "orgtable" => WpValue::Str(c.org_table.clone()),
                "max_length" => int_value(c.max_length),
                "length" => int_value(c.length),
                "charsetnr" => WpValue::Int(i64::from(c.charset)),
                "flags" => WpValue::Int(i64::from(c.flags)),
                "type" => WpValue::Int(type_code(&c.sql_type)),
                "decimals" => WpValue::Int(i64::from(c.decimals)),
                "def" => WpValue::Str(String::new()),
                "db" => WpValue::Str(db.clone()),
                "catalog" => WpValue::Str("def".into()),
                _ => WpValue::Null,
            }
        };
        if col_offset == -1 {
            WpValue::Array(
                columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (Key::Int(i as i64), one(c)))
                    .collect(),
            )
        } else {
            usize::try_from(col_offset)
                .ok()
                .and_then(|i| columns.get(i))
                .map_or(WpValue::Null, one)
        }
    }
}

impl WpDb {
    /// The current `last_result`, read.
    pub fn with_last_result<T>(&self, f: impl FnOnce(&[WpRow]) -> T) -> T {
        let rows = self.rows();
        f(&rows)
    }

    /// Changes whenever `last_result` is replaced or flushed.
    pub fn result_generation(&self) -> u64 {
        self.read(|s| s.result_generation)
    }

    /// Whether the request holds a connection (`! empty( $this->dbh )`).
    pub fn read_dbh_present(&self) -> bool {
        self.read(|s| s.dbh_present)
    }

    /// `show_errors` or `suppress_errors`.
    pub fn read_bool(&self, name: &str) -> bool {
        self.read(|s| match name {
            "show_errors" => s.show_errors,
            "suppress_errors" => s.suppress_errors,
            _ => false,
        })
    }

    /// `wpdb::strip_invalid_text` over fields PHP built.
    pub fn strip_fields(&self, fields: &mut [Field]) -> Result<(), WpDbError> {
        self.strip_context(|cx| {
            CharsetRules::strip_invalid_text(fields, cx, &mut |sql| self.convert_row(sql))
        })
    }

    /// `wpdb::strip_invalid_text_from_query`.
    pub fn strip_query_text(&self, query: &[u8]) -> Result<Vec<u8>, WpDbError> {
        self.strip_invalid_text_from_query(query)
    }
}
