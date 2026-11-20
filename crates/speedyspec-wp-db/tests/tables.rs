use speedyspec_wp_db::{TableEnv, TableNames, TableScope, WpDbError};

fn single() -> TableEnv {
    TableEnv {
        multisite: false,
        multisite_constant: false,
        custom_user_table: None,
        custom_user_meta_table: None,
    }
}

fn multi() -> TableEnv {
    TableEnv {
        multisite: true,
        multisite_constant: true,
        custom_user_table: None,
        custom_user_meta_table: None,
    }
}

#[test]
fn single_site_names() {
    let mut t = TableNames::new();
    assert_eq!(t.set_prefix("wp_", true, &single()).unwrap(), "wp_");
    assert_eq!(t.name("posts"), "wp_posts");
    assert_eq!(t.name("users"), "wp_users");
    assert_eq!(t.name("categories"), "wp_categories");
    assert_eq!(t.name("my_table"), "wp_my_table");
    assert_eq!(t.get("posts"), Some("wp_posts"));
    assert_eq!(t.base_prefix(), Some("wp_"));
    // A second call returns the previous base prefix.
    assert_eq!(t.set_prefix("xx_", true, &single()).unwrap(), "wp_");
}

#[test]
fn multisite_blog_two() {
    let env = multi();
    let mut t = TableNames::new();
    assert_eq!(t.set_prefix("wp_", true, &env).unwrap(), "");
    assert_eq!(t.prefix(), "");
    assert_eq!(t.name("users"), "wp_users");
    assert_eq!(t.set_blog_id(2, 1, &env), 0);
    assert_eq!(t.name("posts"), "wp_2_posts");
    assert_eq!(t.name("users"), "wp_users");
    assert_eq!(t.name("blogs"), "wp_blogs");
    assert_eq!(t.get_blog_prefix(Some(1), &env), "wp_");
    assert_eq!(t.get_blog_prefix(Some(0), &env), "wp_");
    assert_eq!(t.get_blog_prefix(None, &env), "wp_2_");
    assert_eq!(t.siteid(), 1);
    assert_eq!(t.blogid(), 2);
}

#[test]
fn tables_scope_order() {
    let env = multi();
    let mut t = TableNames::new();
    t.set_prefix("wp_", true, &env).unwrap();
    let keys = |v: Vec<(String, String)>| v.into_iter().map(|(k, _)| k).collect::<Vec<_>>();
    assert_eq!(
        keys(t.tables(TableScope::Blog, true, 0, &env)),
        [
            "posts",
            "comments",
            "links",
            "options",
            "postmeta",
            "terms",
            "term_taxonomy",
            "term_relationships",
            "termmeta",
            "commentmeta"
        ]
    );
    let all = keys(t.tables(TableScope::All, true, 0, &env));
    assert_eq!(&all[..2], ["users", "usermeta"]);
    assert_eq!(
        &all[12..],
        [
            "blogs",
            "blogmeta",
            "signups",
            "site",
            "sitemeta",
            "registration_log"
        ]
    );
    assert_eq!(TableScope::parse("nope"), None);
    assert_eq!(TableScope::parse("ms_global"), Some(TableScope::MsGlobal));
    let unprefixed = t.tables(TableScope::Old, false, 0, &env);
    assert_eq!(
        unprefixed[0],
        ("categories".to_string(), "categories".to_string())
    );
    let blog3 = t.tables(TableScope::Blog, true, 3, &env);
    assert_eq!(blog3[0].1, "wp_3_posts");
}

#[test]
fn custom_user_table() {
    let env = TableEnv {
        custom_user_table: Some("shared_users".into()),
        ..single()
    };
    let mut t = TableNames::new();
    t.set_prefix("wp_", true, &env).unwrap();
    let global = t.tables(TableScope::Global, true, 0, &env);
    assert_eq!(global[0], ("users".to_string(), "shared_users".to_string()));
    assert_eq!(t.name("users"), "shared_users");
}

#[test]
fn invalid_prefix() {
    let mut t = TableNames::new();
    t.set_prefix("wp_", true, &single()).unwrap();
    assert!(matches!(
        t.set_prefix("wp-", true, &single()),
        Err(WpDbError::InvalidPrefix)
    ));
    assert_eq!(t.name("posts"), "wp_posts");
    assert_eq!(t.base_prefix(), Some("wp_"));
}
