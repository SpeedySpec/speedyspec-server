mod support;

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Command, CommandBus, Query, QueryBus};
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_options::{
    AddOption, AutoloadValuesToAutoload, DeleteOption, DetermineOptionAutoloadValue,
    FilterDefaultAutoloadValueViaOptionSize, GetOption, GetOptions, LoadAlloptions,
    PrimeOptionCaches, PrimeOptionCachesByGroup, ProtectSpecialOption, SetOptionAutoload,
    SetOptionAutoloadValues, SetOptionsAutoload, UpdateOption, register_options_handlers,
};
use support::*;

#[test]
fn handlers_are_bound() {
    let f = fixture();
    let container = Rc::new(Container::new());
    register_options_handlers(&container, Rc::new(f.api));
    // Every key resolves.
    macro_rules! bound {
        (commands: $($c:ty),*; queries: $($q:ty),*) => {
            $( assert!(container.has(<$c as Command>::KEY), stringify!($c)); )*
            $( assert!(container.has(<$q as Query>::KEY), stringify!($q)); )*
        };
    }
    bound!(
        commands: AddOption, UpdateOption, DeleteOption, PrimeOptionCaches, PrimeOptionCachesByGroup,
            SetOptionAutoloadValues, SetOptionsAutoload, SetOptionAutoload, ProtectSpecialOption;
        queries: GetOption, GetOptions, LoadAlloptions, DetermineOptionAutoloadValue,
            FilterDefaultAutoloadValueViaOptionSize, AutoloadValuesToAutoload
    );
    let commands = CommandBus::new(container.clone());
    let queries = QueryBus::new(container.clone());
    let updated = commands
        .dispatch(UpdateOption {
            name: "k".into(),
            value: s("v"),
            autoload: HookValue::Null,
        })
        .unwrap()
        .unwrap();
    assert!(updated);
    let got = queries
        .ask(GetOption {
            name: "k".into(),
            default: None,
        })
        .unwrap()
        .unwrap();
    assert_eq!(got, s("v"));
    let decided = queries
        .ask(DetermineOptionAutoloadValue {
            name: "k".into(),
            value: s("v"),
            serialized: s("v"),
            autoload: HookValue::Bool(false),
        })
        .unwrap()
        .unwrap();
    assert_eq!(decided, s("off"));
    assert_eq!(
        <GetOption as Query>::KEY,
        "SpeedySpec\\WpOptions\\GetOptionHandler"
    );
}
