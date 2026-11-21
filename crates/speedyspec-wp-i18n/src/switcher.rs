//! `WP_Locale_Switcher`'s stack and decisions (Memento: each switch pushes the locale to return past).
//!
//! The stack sits in a `RefCell` that is never borrowed while an effect runs, because effects run PHP (the
//! `change_locale` action) that may switch again on the same switcher.

use std::cell::RefCell;

use speedyspec_wp_i18n_contracts::{PhpValue, PortError, SwitchEffects, php_truthy};

/// One request's locale switcher.
#[derive(Debug)]
pub struct LocaleSwitcher {
    stack: RefCell<Vec<(PhpValue, PhpValue)>>,
    original: PhpValue,
    available: Vec<String>,
}

impl LocaleSwitcher {
    /// A switcher whose original locale is `original` (`determine_locale()` when it was made) and which may switch
    /// to `available` (`en_US` and `get_available_languages()`).
    pub fn new(original: PhpValue, available: Vec<String>) -> LocaleSwitcher {
        LocaleSwitcher {
            stack: RefCell::new(Vec::new()),
            original,
            available,
        }
    }

    /// `switch_to_locale( $locale, $user_id )`: false when `locale` is the current or not an available one.
    pub fn switch_to_locale(
        &self,
        effects: &dyn SwitchEffects,
        locale: &PhpValue,
        user: &PhpValue,
    ) -> Result<bool, PortError> {
        let current = effects.determine_locale()?;
        if current == *locale {
            return Ok(false);
        }
        let available = match locale {
            PhpValue::Str(s) => self.available.iter().any(|a| a == s),
            _ => false,
        };
        if !available {
            return Ok(false);
        }
        self.stack.borrow_mut().push((locale.clone(), user.clone()));
        effects.change_locale(locale)?;
        effects.switched(locale, user)?;
        Ok(true)
    }

    /// `switch_to_user_locale( $user_id )`.
    pub fn switch_to_user_locale(
        &self,
        effects: &dyn SwitchEffects,
        user: &PhpValue,
    ) -> Result<bool, PortError> {
        let locale = effects.user_locale(user)?;
        self.switch_to_locale(effects, &locale, user)
    }

    /// `restore_previous_locale()`: the locale now in effect, or `None` (WordPress's false) when nothing was
    /// switched.
    pub fn restore_previous_locale(
        &self,
        effects: &dyn SwitchEffects,
    ) -> Result<Option<PhpValue>, PortError> {
        let (previous, locale) = {
            let mut stack = self.stack.borrow_mut();
            let Some(previous) = stack.pop() else {
                return Ok(None);
            };
            let locale = match stack.last() {
                Some((locale, _)) if php_truthy(locale) => locale.clone(),
                _ => self.original.clone(),
            };
            (previous, locale)
        };
        effects.change_locale(&locale)?;
        effects.restored(&locale, &previous.0)?;
        Ok(Some(locale))
    }

    /// `restore_current_locale()`: back to the original locale, or `None` when nothing was switched.
    pub fn restore_current_locale(
        &self,
        effects: &dyn SwitchEffects,
    ) -> Result<Option<PhpValue>, PortError> {
        {
            let mut stack = self.stack.borrow_mut();
            if stack.is_empty() {
                return Ok(None);
            }
            *stack = vec![(self.original.clone(), PhpValue::Bool(false))];
        }
        self.restore_previous_locale(effects)
    }

    /// `is_switched()`.
    pub fn is_switched(&self) -> bool {
        !self.stack.borrow().is_empty()
    }

    /// `get_switched_locale()`: `None` is WordPress's false.
    pub fn switched_locale(&self) -> Option<PhpValue> {
        self.stack.borrow().last().map(|(locale, _)| locale.clone())
    }

    /// `get_switched_user_id()`: `None` is WordPress's false.
    pub fn switched_user_id(&self) -> Option<PhpValue> {
        self.stack.borrow().last().map(|(_, user)| user.clone())
    }

    /// `filter_locale( $locale )`, the `locale` and `determine_locale` filter: the switched locale when there is a
    /// truthy one.
    pub fn filter_locale(&self, locale: PhpValue) -> PhpValue {
        match self.switched_locale() {
            Some(switched) if php_truthy(&switched) => switched,
            _ => locale,
        }
    }
}
