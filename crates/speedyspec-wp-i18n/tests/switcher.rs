//! The locale switcher's stack, against a recording double for its effects.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_wp_i18n::LocaleSwitcher;
use speedyspec_wp_i18n_contracts::{PhpValue, PortError, SwitchEffects};

fn s(v: &str) -> PhpValue {
    PhpValue::Str(v.into())
}

const NO_USER: PhpValue = PhpValue::Bool(false);

/// Records effects; `determine_locale` answers the switched locale, as the `determine_locale` filter makes it.
struct Effects {
    log: RefCell<Vec<String>>,
    switcher: RefCell<Option<Rc<LocaleSwitcher>>>,
    nest_once: Cell<bool>,
}

impl Effects {
    fn new() -> Effects {
        Effects {
            log: RefCell::new(Vec::new()),
            switcher: RefCell::new(None),
            nest_once: Cell::new(false),
        }
    }

    fn current(&self) -> PhpValue {
        let switcher = self.switcher.borrow();
        let switcher = switcher.as_ref().expect("the switcher is attached");
        switcher.filter_locale(s("en_US"))
    }
}

fn show(v: &PhpValue) -> String {
    match v {
        PhpValue::Str(s) => s.clone(),
        PhpValue::Int(n) => n.to_string(),
        PhpValue::Bool(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

impl SwitchEffects for Effects {
    fn determine_locale(&self) -> Result<PhpValue, PortError> {
        self.log.borrow_mut().push("determine_locale".into());
        Ok(self.current())
    }

    fn user_locale(&self, user: &PhpValue) -> Result<PhpValue, PortError> {
        self.log
            .borrow_mut()
            .push(format!("user_locale({})", show(user)));
        Ok(s("de_DE"))
    }

    fn change_locale(&self, locale: &PhpValue) -> Result<(), PortError> {
        self.log
            .borrow_mut()
            .push(format!("change_locale({})", show(locale)));
        if self.nest_once.replace(false) {
            let switcher = self.switcher.borrow().clone().unwrap();
            switcher.switch_to_locale(self, &s("de_DE"), &NO_USER)?;
        }
        Ok(())
    }

    fn switched(&self, locale: &PhpValue, user: &PhpValue) -> Result<(), PortError> {
        self.log
            .borrow_mut()
            .push(format!("switched({}, {})", show(locale), show(user)));
        Ok(())
    }

    fn restored(&self, locale: &PhpValue, previous: &PhpValue) -> Result<(), PortError> {
        self.log
            .borrow_mut()
            .push(format!("restored({}, {})", show(locale), show(previous)));
        Ok(())
    }
}

fn setup() -> (Rc<LocaleSwitcher>, Effects) {
    let switcher = Rc::new(LocaleSwitcher::new(
        s("en_US"),
        vec!["en_US".into(), "de_DE".into(), "es_ES".into()],
    ));
    let effects = Effects::new();
    *effects.switcher.borrow_mut() = Some(switcher.clone());
    (switcher, effects)
}

#[test]
fn switch_to_current_locale_returns_false_and_does_nothing() {
    let (sw, fx) = setup();
    assert!(!sw.switch_to_locale(&fx, &s("en_US"), &NO_USER).unwrap());
    assert_eq!(*fx.log.borrow(), vec!["determine_locale"]);
    assert!(!sw.is_switched());
}

#[test]
fn switch_to_unavailable_locale_returns_false() {
    let (sw, fx) = setup();
    assert!(!sw.switch_to_locale(&fx, &s("foo_BAR"), &NO_USER).unwrap());
    assert!(
        !sw.switch_to_locale(&fx, &PhpValue::Int(5), &NO_USER)
            .unwrap()
    );
    assert_eq!(
        *fx.log.borrow(),
        vec!["determine_locale", "determine_locale"]
    );
}

#[test]
fn switch_records_effects_in_order() {
    let (sw, fx) = setup();
    assert!(sw.switch_to_locale(&fx, &s("de_DE"), &NO_USER).unwrap());
    assert_eq!(
        *fx.log.borrow(),
        vec![
            "determine_locale",
            "change_locale(de_DE)",
            "switched(de_DE, false)"
        ]
    );
    assert_eq!(sw.switched_locale(), Some(s("de_DE")));
    assert_eq!(sw.switched_user_id(), Some(NO_USER));
}

#[test]
fn switch_to_user_locale_records_the_user() {
    let (sw, fx) = setup();
    assert!(sw.switch_to_user_locale(&fx, &PhpValue::Int(7)).unwrap());
    assert_eq!(sw.switched_user_id(), Some(PhpValue::Int(7)));
    assert_eq!(fx.log.borrow()[0], "user_locale(7)");
    assert_eq!(fx.log.borrow().last().unwrap(), "switched(de_DE, 7)");
}

#[test]
fn restore_after_two_switches_returns_first() {
    let (sw, fx) = setup();
    sw.switch_to_locale(&fx, &s("de_DE"), &NO_USER).unwrap();
    sw.switch_to_locale(&fx, &s("es_ES"), &NO_USER).unwrap();
    fx.log.borrow_mut().clear();
    assert_eq!(sw.restore_previous_locale(&fx).unwrap(), Some(s("de_DE")));
    assert_eq!(
        *fx.log.borrow(),
        vec!["change_locale(de_DE)", "restored(de_DE, es_ES)"]
    );
    assert_eq!(sw.restore_previous_locale(&fx).unwrap(), Some(s("en_US")));
    assert!(!sw.is_switched());
}

#[test]
fn restore_empty_stack_returns_none() {
    let (sw, fx) = setup();
    assert_eq!(sw.restore_previous_locale(&fx).unwrap(), None);
    assert_eq!(sw.restore_current_locale(&fx).unwrap(), None);
    assert!(fx.log.borrow().is_empty());
}

#[test]
fn restore_current_goes_to_original() {
    let (sw, fx) = setup();
    sw.switch_to_locale(&fx, &s("de_DE"), &NO_USER).unwrap();
    sw.switch_to_locale(&fx, &s("es_ES"), &NO_USER).unwrap();
    fx.log.borrow_mut().clear();
    assert_eq!(sw.restore_current_locale(&fx).unwrap(), Some(s("en_US")));
    assert_eq!(
        *fx.log.borrow(),
        vec!["change_locale(en_US)", "restored(en_US, en_US)"]
    );
    assert!(!sw.is_switched());
    assert_eq!(sw.switched_locale(), None);
}

#[test]
fn filter_locale_passes_through_unswitched() {
    let (sw, fx) = setup();
    assert_eq!(sw.filter_locale(s("fr_FR")), s("fr_FR"));
    sw.switch_to_locale(&fx, &s("de_DE"), &NO_USER).unwrap();
    assert_eq!(sw.filter_locale(s("fr_FR")), s("de_DE"));
}

#[test]
fn switch_from_inside_change_locale() {
    let (sw, fx) = setup();
    fx.nest_once.set(true);
    assert!(sw.switch_to_locale(&fx, &s("es_ES"), &NO_USER).unwrap());
    assert_eq!(sw.switched_locale(), Some(s("de_DE")));
    assert_eq!(
        *fx.log.borrow(),
        vec![
            "determine_locale",
            "change_locale(es_ES)",
            "determine_locale",
            "change_locale(de_DE)",
            "switched(de_DE, false)",
            "switched(es_ES, false)",
        ]
    );
    assert_eq!(sw.restore_previous_locale(&fx).unwrap(), Some(s("es_ES")));
}
