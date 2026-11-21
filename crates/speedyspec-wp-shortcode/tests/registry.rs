//! `MemoryRegistry` keeps `$shortcode_tags`' PHP array order and a generation that changes on every write.

use std::rc::Rc;

use speedyspec_wp_shortcode::MemoryRegistry;
use speedyspec_wp_shortcode_contracts::{
    ParsedAtts, PhpValue, ShortcodeError, ShortcodeHandler, ShortcodeRegistry, ShortcodeTagNames, TagName,
};

struct Echo;
impl ShortcodeHandler for Echo {
    fn call(&self, _: &ParsedAtts, _: Option<&[u8]>, _: &[u8]) -> Result<PhpValue, ShortcodeError> {
        Ok(PhpValue::Str("x".into()))
    }
}

fn tag(n: &str) -> TagName {
    TagName::new(n.as_bytes()).unwrap()
}

fn names(r: &MemoryRegistry) -> Vec<Vec<u8>> {
    r.tag_names().iter().map(|t| t.as_bytes().to_vec()).collect()
}

#[test]
fn readd_keeps_position() {
    let r = MemoryRegistry::new();
    r.add(tag("a"), Rc::new(Echo));
    r.add(tag("b"), Rc::new(Echo));
    r.add(tag("a"), Rc::new(Echo));
    assert_eq!(names(&r), vec![b"a".to_vec(), b"b".to_vec()]);
}

#[test]
fn remove_then_add_moves_to_end() {
    let r = MemoryRegistry::new();
    r.add(tag("a"), Rc::new(Echo));
    r.add(tag("b"), Rc::new(Echo));
    r.remove(b"a");
    r.add(tag("a"), Rc::new(Echo));
    assert_eq!(names(&r), vec![b"b".to_vec(), b"a".to_vec()]);
}

#[test]
fn generation_changes_on_every_mutation() {
    let r = MemoryRegistry::new();
    let g0 = r.generation();
    r.add(tag("a"), Rc::new(Echo));
    let g1 = r.generation();
    assert_ne!(g0, g1);
    assert!(r.exists(b"a"));
    assert!(r.handler(b"a").is_some());
    let _ = r.tag_names();
    assert_eq!(r.generation(), g1);
    r.remove(b"a");
    let g2 = r.generation();
    assert_ne!(g1, g2);
    r.remove_all();
    assert_ne!(r.generation(), g2);
    assert!(!r.exists(b"a"));
}
