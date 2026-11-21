use indexmap::IndexMap;
use speedyspec_wp_kses_contracts::{AllowedAttrs, AllowedHtml, AttrRules};
use speedyspec_wp_kses_lexical::array_lc;

#[test]
fn lowercases_tags_and_attributes() {
    let mut attrs = IndexMap::new();
    attrs.insert(b"HREF".to_vec().into_boxed_slice(), AttrRules::Allowed);
    let mut tags = IndexMap::new();
    tags.insert(b"A".to_vec().into_boxed_slice(), AllowedAttrs::Attrs(attrs));
    tags.insert(b"Br".to_vec().into_boxed_slice(), AllowedAttrs::Empty);
    let out = array_lc(&AllowedHtml { tags });
    let Some(AllowedAttrs::Attrs(a)) = out.get(b"a") else {
        panic!("a")
    };
    assert_eq!(a.get(&b"href"[..]), Some(&AttrRules::Allowed));
    assert_eq!(out.get(b"br"), Some(&AllowedAttrs::Attrs(IndexMap::new())));
}
