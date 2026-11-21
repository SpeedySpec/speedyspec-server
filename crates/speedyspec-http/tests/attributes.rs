use speedyspec_http::Attributes;

#[test]
fn attributes_are_typed() {
    let attributes = Attributes::new().with(5u32).with("x");
    assert_eq!(attributes.get::<u32>(), Some(&5));
    assert_eq!(attributes.get::<&str>(), Some(&"x"));
    assert_eq!(attributes.get::<i64>(), None);
}

#[test]
fn with_replaces_same_type_and_without_removes() {
    let attributes = Attributes::new().with(5u32).with(6u32);
    assert_eq!(attributes.get::<u32>(), Some(&6));
    let attributes = attributes.without::<u32>();
    assert_eq!(attributes.get::<u32>(), None);
}
