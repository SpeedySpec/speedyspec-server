//! wpdb's values and errors, as the contracts hold them.

use std::rc::Rc;

use speedyspec_data::{DbValue, Row};
use speedyspec_wp_db_contracts::{QueryOutcome, WpDbError, WpRow, WpValue, take_hook_error};
use speedyspec_wp_hooks_contracts::HookError;

#[test]
fn query_outcome_to_wp() {
    let rows = Rc::new(vec![WpRow::default(), WpRow::default()]);
    assert_eq!(QueryOutcome::Rows(rows).to_wp(), WpValue::Int(2));
    assert_eq!(
        QueryOutcome::Affected {
            rows: 3,
            insert_id: 9
        }
        .to_wp(),
        WpValue::Int(3)
    );
    assert_eq!(QueryOutcome::Done.to_wp(), WpValue::Bool(true));
}

#[test]
fn wp_row_keeps_the_last_duplicate_at_the_first_position() {
    let columns: Rc<[String]> = ["a", "b", "a"].iter().map(|c| c.to_string()).collect();
    let row = Row::new(
        columns,
        vec![
            DbValue::Text("first".into()),
            DbValue::Text("second".into()),
            DbValue::Text("third".into()),
        ],
    );
    let wp = WpRow::from_row(&row);
    assert_eq!(
        wp.fields,
        vec![
            ("a".to_string(), Some(b"third".to_vec())),
            ("b".to_string(), Some(b"second".to_vec())),
        ]
    );
    assert_eq!(wp.get("a"), Some(&Some(b"third".to_vec())));
}

#[test]
fn hook_error_is_taken_once() {
    let error = WpDbError::from(HookError::message("boom"));
    assert!(matches!(error, WpDbError::Hook(_)));
    assert_eq!(
        take_hook_error().map(|e| e.message),
        Some("boom".to_string())
    );
    assert!(take_hook_error().is_none());
}
