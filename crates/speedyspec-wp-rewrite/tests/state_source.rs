mod support;

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_cqrs::CommandHandler;
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_rewrite::{AddRewriteTag, RewriteError, RewriteState, RewriteStateSource};
use support::fixture;

/// A source whose state cannot be read (a PHP property that does not convert), counting writes.
struct Unreadable {
    stores: Cell<usize>,
}

impl RewriteStateSource for Unreadable {
    fn load(&self) -> Result<RewriteState, RewriteError> {
        Err(RewriteError("the properties do not convert".into()))
    }

    fn store(&self, _state: &RewriteState) {
        self.stores.set(self.stores.get() + 1);
    }

    fn hook_value(&self) -> HookValue {
        HookValue::Null
    }
}

#[test]
fn failed_load_never_writes_defaults_back() {
    let f = fixture("/%postname%/");
    let source = Rc::new(Unreadable {
        stores: Cell::new(0),
    });
    let result = CommandHandler::<AddRewriteTag>::handle(
        &f.service,
        AddRewriteTag {
            source: source.clone(),
            tag: "%x%".into(),
            regex: "(.+)".into(),
            query: "x=".into(),
        },
    );
    assert!(result.is_err());
    assert_eq!(source.stores.get(), 0);
}
