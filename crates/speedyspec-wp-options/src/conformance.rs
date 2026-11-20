//! The checks every option repository passes, so the memory and SQL repositories answer alike.

use crate::Option as OptionEntity;
use crate::{Autoload, OptionName, OptionRepository, SerializedValue};

fn ensure(check: &str, ok: bool, detail: impl FnOnce() -> String) {
    assert!(ok, "option repository check `{check}` failed: {}", detail());
}

fn name(n: &str) -> OptionName {
    OptionName::new(n).expect("a valid name")
}

fn option(n: &str, value: &str, autoload: Autoload) -> OptionEntity {
    OptionEntity {
        name: name(n),
        value: SerializedValue::new(value.as_bytes().to_vec()),
        autoload,
    }
}

fn names(rows: &[(String, SerializedValue)]) -> Vec<&str> {
    rows.iter().map(|(n, _)| n.as_str()).collect()
}

/// Runs every check against `repo`, which must start with an empty table. Panics naming the failing check.
pub fn option_repository_suite(repo: &dyn OptionRepository) {
    let value = |n: &str| repo.value(&name(n), false).expect("value");

    let added = repo.add(&option("a", "1", Autoload::On)).expect("add");
    ensure("add_new", added == 1, || format!("{added}"));
    let added = repo.add(&option("a", "1", Autoload::On)).expect("add");
    ensure("add_same", added == 0, || format!("{added}"));
    let added = repo.add(&option("a", "2", Autoload::On)).expect("add");
    ensure(
        "add_changed",
        added == 2 && value("a").as_ref().map(SerializedValue::as_bytes) == Some(b"2"),
        || format!("{added} {:?}", value("a")),
    );

    ensure(
        "value_found_and_missing",
        value("missing").is_none(),
        || format!("{:?}", value("missing")),
    );

    let autoload = repo.autoload_of(&name("a")).expect("autoload_of");
    ensure("autoload_of", autoload == Some(Autoload::On), || {
        format!("{autoload:?}")
    });
    let missing = repo.autoload_of(&name("missing")).expect("autoload_of");
    ensure("autoload_of", missing.is_none(), || format!("{missing:?}"));

    repo.add(&option("b", "b", Autoload::Off)).expect("add");
    repo.add(&option("c", "c", Autoload::Auto)).expect("add");
    repo.add(&option("d", "d", Autoload::Yes)).expect("add");
    let values: Vec<String> = ["yes", "on", "auto-on", "auto"]
        .iter()
        .map(|v| v.to_string())
        .collect();
    let loaded = repo.autoloaded(&values).expect("autoloaded");
    ensure(
        "autoloaded_filters_by_values",
        names(&loaded) == ["a", "c", "d"],
        || format!("{:?}", names(&loaded)),
    );

    let all = repo.all().expect("all");
    ensure("all", names(&all) == ["a", "b", "c", "d"], || {
        format!("{:?}", names(&all))
    });

    let some = repo
        .values_of(&["b".into(), "nope".into(), "a".into()])
        .expect("values_of");
    let mut found = names(&some);
    found.sort();
    ensure("values_of", found == ["a", "b"], || format!("{found:?}"));

    let change = repo
        .needing_autoload_change(&[(Autoload::On, vec!["a".into(), "b".into(), "nope".into()])])
        .expect("needing_autoload_change");
    ensure("needing_autoload_change", change == ["b"], || {
        format!("{change:?}")
    });

    let set = repo
        .set_autoload(&["b".into(), "c".into()], &Autoload::Off)
        .expect("set_autoload");
    ensure("set_autoload", set == 1, || format!("{set}"));
    let again = repo
        .set_autoload(&["b".into(), "c".into()], &Autoload::Off)
        .expect("set_autoload");
    ensure("set_autoload", again == 0, || format!("{again}"));

    let updated = repo
        .update(&name("a"), Some(&SerializedValue::new(b"3".to_vec())), None)
        .expect("update");
    let autoload = repo.autoload_of(&name("a")).expect("autoload_of");
    ensure(
        "update_value_only",
        updated == 1
            && value("a").as_ref().map(SerializedValue::as_bytes) == Some(b"3")
            && autoload == Some(Autoload::On),
        || format!("{updated} {autoload:?}"),
    );
    let same = repo
        .update(
            &name("a"),
            Some(&SerializedValue::new(b"3".to_vec())),
            Some(&Autoload::On),
        )
        .expect("update");
    ensure("update_same_is_zero", same == 0, || format!("{same}"));
    let missing = repo
        .update(
            &name("missing"),
            Some(&SerializedValue::new(b"x".to_vec())),
            None,
        )
        .expect("update");
    ensure("update_missing_is_zero", missing == 0, || {
        format!("{missing}")
    });

    let removed = repo.remove(&name("a")).expect("remove");
    let again = repo.remove(&name("a")).expect("remove");
    ensure(
        "remove",
        removed == 1 && again == 0 && value("a").is_none(),
        || format!("{removed} {again}"),
    );
}
