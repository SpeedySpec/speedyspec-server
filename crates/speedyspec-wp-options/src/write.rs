//! The write path: `add_option`, `update_option` and `delete_option`, each as `option.php` has it.
//!
//! Each function is split around its one write: a `plan_*` step runs the checks, filters and actions up to the
//! write and says what to write ([`PlannedWrite`]); [`OptionsApi::finish`] runs the cache updates and actions after
//! it. The unit-of-work path (Task 8) runs the plan, commits, then finishes.

use speedyspec_wp_hooks::HookValue;

use crate::Option as OptionEntity;
use crate::api::{OptionsApi, isset, set_key, unset_key};
use crate::{
    Autoload, AutoloadRequest, OptionName, OptionRepository, OptionsError, SerializedValue,
    autoload_values, php_empty,
};

/// What a write function decided to write, and what it needs afterwards.
#[derive(Debug, Clone)]
pub(crate) enum PlannedWrite {
    /// `add_option`'s `INSERT ... ON DUPLICATE KEY UPDATE`. `stored` is `maybe_serialize($value)`, what the caches
    /// hold.
    Add {
        option: OptionEntity,
        value: HookValue,
        stored: HookValue,
    },
    /// `update_option`'s `wpdb::update`; `autoload` is set when the column changes.
    Update {
        name: OptionName,
        old: HookValue,
        value: HookValue,
        stored: HookValue,
        autoload: Option<Autoload>,
    },
    /// `delete_option`'s `wpdb::delete`; `autoload` is the row's.
    Delete {
        name: OptionName,
        autoload: Autoload,
    },
}

fn text(name: &OptionName) -> HookValue {
    HookValue::Str(name.as_str().to_string())
}

fn renamed(old: &OptionName, new: &str) -> String {
    format!("The \"{old}\" option key has been renamed to \"{new}\".")
}

impl OptionsApi {
    /// `add_option( $option, $value, $deprecated, $autoload )`.
    pub fn add_option(
        &self,
        name: &str,
        value: HookValue,
        deprecated: HookValue,
        autoload: HookValue,
    ) -> Result<bool, OptionsError> {
        match self.plan_add(name, value, deprecated, autoload, true)? {
            Some(planned) => self.write_and_finish(&planned),
            None => Ok(false),
        }
    }

    /// `update_option( $option, $value, $autoload )`.
    pub fn update_option(
        &self,
        name: &str,
        value: HookValue,
        autoload: HookValue,
    ) -> Result<bool, OptionsError> {
        match self.plan_update(name, value, autoload)? {
            Some(planned) => self.write_and_finish(&planned),
            None => Ok(false),
        }
    }

    /// `delete_option( $option )`.
    pub fn delete_option(&self, name: &str) -> Result<bool, OptionsError> {
        match self.plan_delete(name, true)? {
            Some(planned) => self.write_and_finish(&planned),
            None => Ok(false),
        }
    }

    fn write_and_finish(&self, planned: &PlannedWrite) -> Result<bool, OptionsError> {
        let rows = self.write(planned)?;
        self.finish(planned, rows)
    }

    /// The one statement a planned write sends; its affected-row count.
    pub(crate) fn write(&self, planned: &PlannedWrite) -> Result<u64, OptionsError> {
        write_planned(&*self.d.repository, planned)
    }

    /// `add_option` up to its `INSERT`; `None` where it returns false.
    pub(crate) fn plan_add(
        &self,
        name: &str,
        value: HookValue,
        deprecated: HookValue,
        autoload: HookValue,
        fire_before: bool,
    ) -> Result<Option<PlannedWrite>, OptionsError> {
        if !php_empty(&deprecated.read()) {
            self.d
                .deprecations
                .deprecated_argument("add_option", "2.3.0", "")?;
        }
        let Some(option) = OptionName::new(name) else {
            return Ok(None);
        };
        if let Some(new) = option.deprecated_replacement()
            && !self.d.installing.installing()
        {
            self.d.deprecations.deprecated_argument(
                "add_option",
                "5.5.0",
                &renamed(&option, new),
            )?;
            return self.plan_add(new, value, deprecated, autoload, fire_before);
        }
        self.d.protected.protect(option.as_str())?;
        let value = self.d.codec.clone_value(value);
        let value = self.d.sanitizing.sanitize_option(&option, value)?;
        let group = Self::group();
        let notoptions = self
            .cache()
            .get(&HookValue::Str("notoptions".into()), &group, false)?;
        let known_missing = notoptions.is_some_and(|n| isset(&n.read(), option.as_str()).is_some());
        if !known_missing {
            let default = self.default_option(&option)?;
            if default.read() != self.get_option(option.as_str(), None)?.read() {
                return Ok(None);
            }
        }
        let stored = self.d.codec.maybe_serialize(&value)?;
        let autoload = self.d.autoload.determine(
            option.as_str(),
            &value,
            &stored,
            &AutoloadRequest::from_value(&autoload.read()),
        )?;
        let entity = OptionEntity {
            name: option,
            value: SerializedValue::for_insert(&stored.read())?,
            autoload,
        };
        let planned = PlannedWrite::Add {
            option: entity,
            value,
            stored,
        };
        if fire_before {
            self.before(&planned)?;
        }
        Ok(Some(planned))
    }

    /// `apply_filters( "default_option_{$option}", false, $option, false )`, the existence test both writers use.
    fn default_option(&self, option: &OptionName) -> Result<HookValue, OptionsError> {
        self.filter(
            &format!("default_option_{option}"),
            HookValue::Bool(false),
            || vec![text(option), HookValue::Bool(false)],
        )
    }

    /// `update_option` up to its `UPDATE` (or the `add_option` it becomes); `None` where it returns false.
    pub(crate) fn plan_update(
        &self,
        name: &str,
        value: HookValue,
        autoload: HookValue,
    ) -> Result<Option<PlannedWrite>, OptionsError> {
        self.plan_update_over(name, value, autoload, None, true)
    }

    /// `plan_update`, with the old value given when a unit of work already holds a write of this option.
    pub(crate) fn plan_update_over(
        &self,
        name: &str,
        value: HookValue,
        autoload: HookValue,
        known_old: Option<HookValue>,
        fire_before: bool,
    ) -> Result<Option<PlannedWrite>, OptionsError> {
        let Some(option) = OptionName::new(name) else {
            return Ok(None);
        };
        if let Some(new) = option.deprecated_replacement()
            && !self.d.installing.installing()
        {
            self.d.deprecations.deprecated_argument(
                "update_option",
                "5.5.0",
                &renamed(&option, new),
            )?;
            return self.plan_update_over(new, value, autoload, known_old, fire_before);
        }
        self.d.protected.protect(option.as_str())?;
        let value = self.d.codec.clone_value(value);
        let value = self.d.sanitizing.sanitize_option(&option, value)?;
        let old = match known_old {
            Some(old) => old,
            None => self.get_option(option.as_str(), None)?,
        };
        let value = self.filter(&format!("pre_update_option_{option}"), value, || {
            vec![old.clone(), text(&option)]
        })?;
        let value = self.filter("pre_update_option", value, || {
            vec![text(&option), old.clone()]
        })?;
        if value.read() == old.read()
            || self.d.codec.maybe_serialize(&value)?.read()
                == self.d.codec.maybe_serialize(&old)?.read()
        {
            return Ok(None);
        }
        if self.default_option(&option)?.read() == old.read() {
            return self.plan_add(
                option.as_str(),
                value,
                HookValue::Str(String::new()),
                autoload,
                fire_before,
            );
        }
        let stored = self.d.codec.maybe_serialize(&value)?;
        if fire_before {
            self.action("update_option", || {
                vec![text(&option), old.clone(), value.clone()]
            })?;
        }
        let column = if !matches!(autoload.read().as_ref(), HookValue::Null) {
            Some(self.d.autoload.determine(
                option.as_str(),
                &value,
                &stored,
                &AutoloadRequest::from_value(&autoload.read()),
            )?)
        } else {
            match self.d.repository.autoload_of(&option)? {
                Some(raw @ (Autoload::AutoOn | Autoload::AutoOff | Autoload::Auto)) => {
                    let decided = self.d.autoload.determine(
                        option.as_str(),
                        &value,
                        &stored,
                        &AutoloadRequest::Default,
                    )?;
                    (decided != raw).then_some(decided)
                }
                _ => None,
            }
        };
        Ok(Some(PlannedWrite::Update {
            name: option,
            old,
            value,
            stored,
            autoload: column,
        }))
    }

    /// `delete_option` up to its `DELETE`; `None` where it returns false (no such row).
    pub(crate) fn plan_delete(
        &self,
        name: &str,
        fire_before: bool,
    ) -> Result<Option<PlannedWrite>, OptionsError> {
        let Some(option) = OptionName::new(name) else {
            return Ok(None);
        };
        self.d.protected.protect(option.as_str())?;
        let Some(autoload) = self.d.repository.autoload_of(&option)? else {
            return Ok(None);
        };
        let planned = PlannedWrite::Delete {
            name: option,
            autoload,
        };
        if fire_before {
            self.before(&planned)?;
        }
        Ok(Some(planned))
    }

    /// The action each write fires just before its statement: `add_option`, `update_option` or `delete_option`.
    pub(crate) fn before(&self, planned: &PlannedWrite) -> Result<(), OptionsError> {
        match planned {
            PlannedWrite::Add { option, value, .. } => {
                self.action("add_option", || vec![text(&option.name), value.clone()])
            }
            PlannedWrite::Update {
                name, old, value, ..
            } => self.action("update_option", || {
                vec![text(name), old.clone(), value.clone()]
            }),
            PlannedWrite::Delete { name, .. } => self.action("delete_option", || vec![text(name)]),
        }
    }

    fn autoloads(&self, autoload: &Autoload) -> Result<bool, OptionsError> {
        Ok(autoload_values(&self.d.autoload.values_to_autoload()?)
            .iter()
            .any(|v| v == autoload.as_str()))
    }

    /// The cache updates and actions after the write, given its affected-row count.
    pub(crate) fn finish(&self, planned: &PlannedWrite, rows: u64) -> Result<bool, OptionsError> {
        let group = Self::group();
        let alloptions_key = HookValue::Str("alloptions".into());
        let notoptions_key = HookValue::Str("notoptions".into());
        let installing = self.d.installing.installing();
        let forget_notoption = |name: &OptionName| -> Result<(), OptionsError> {
            if let Some(n) = self.cache().get(&notoptions_key, &group, false)? {
                let mut n = n.read().into_owned();
                if matches!(n, HookValue::Array(_)) && isset(&n, name.as_str()).is_some() {
                    unset_key(&mut n, name.as_str());
                    self.cache().set(&notoptions_key, n, &group, 0)?;
                }
            }
            Ok(())
        };
        let n = |name: &OptionName| text(name);
        match planned {
            PlannedWrite::Add {
                option,
                value,
                stored,
            } => {
                if rows == 0 {
                    return Ok(false);
                }
                let name = &option.name;
                if !installing {
                    if self.autoloads(&option.autoload)? {
                        let mut all = self.load_alloptions(true)?.read().into_owned();
                        set_key(&mut all, name.as_str(), stored.clone());
                        self.cache().set(&alloptions_key, all, &group, 0)?;
                    } else {
                        self.cache().set(&n(name), stored.clone(), &group, 0)?;
                    }
                }
                forget_notoption(name)?;
                self.action(&format!("add_option_{name}"), || {
                    vec![n(name), value.clone()]
                })?;
                self.action("added_option", || vec![n(name), value.clone()])?;
                Ok(true)
            }
            PlannedWrite::Update {
                name,
                old,
                value,
                stored,
                autoload,
            } => {
                if rows == 0 {
                    return Ok(false);
                }
                forget_notoption(name)?;
                if !installing {
                    match autoload {
                        None => {
                            let mut all = self.load_alloptions(true)?.read().into_owned();
                            if isset(&all, name.as_str()).is_some() {
                                set_key(&mut all, name.as_str(), stored.clone());
                                self.cache().set(&alloptions_key, all, &group, 0)?;
                            } else {
                                self.cache().set(&n(name), stored.clone(), &group, 0)?;
                            }
                        }
                        Some(a) if self.autoloads(a)? => {
                            self.cache().delete(&n(name), &group)?;
                            let mut all = self.load_alloptions(true)?.read().into_owned();
                            set_key(&mut all, name.as_str(), stored.clone());
                            self.cache().set(&alloptions_key, all, &group, 0)?;
                        }
                        Some(_) => {
                            let mut all = self.load_alloptions(true)?.read().into_owned();
                            if isset(&all, name.as_str()).is_some() {
                                unset_key(&mut all, name.as_str());
                                self.cache().set(&alloptions_key, all, &group, 0)?;
                            }
                            self.cache().set(&n(name), stored.clone(), &group, 0)?;
                        }
                    }
                }
                self.action(&format!("update_option_{name}"), || {
                    vec![old.clone(), value.clone(), n(name)]
                })?;
                self.action("updated_option", || {
                    vec![n(name), old.clone(), value.clone()]
                })?;
                Ok(true)
            }
            PlannedWrite::Delete { name, autoload } => {
                if !installing {
                    if self.autoloads(autoload)? {
                        let mut all = self.load_alloptions(true)?.read().into_owned();
                        if matches!(all, HookValue::Array(_))
                            && isset(&all, name.as_str()).is_some()
                        {
                            unset_key(&mut all, name.as_str());
                            self.cache().set(&alloptions_key, all, &group, 0)?;
                        }
                    } else {
                        self.cache().delete(&n(name), &group)?;
                    }
                    let mut notoptions = self
                        .cache()
                        .get(&notoptions_key, &group, false)?
                        .map(|v| v.read().into_owned())
                        .unwrap_or(HookValue::Bool(false));
                    if !matches!(notoptions, HookValue::Array(_)) {
                        notoptions = HookValue::Array(Vec::new());
                    }
                    set_key(&mut notoptions, name.as_str(), HookValue::Bool(true));
                    self.cache().set(&notoptions_key, notoptions, &group, 0)?;
                }
                if rows == 0 {
                    return Ok(false);
                }
                self.action(&format!("delete_option_{name}"), || vec![n(name)])?;
                self.action("deleted_option", || vec![n(name)])?;
                Ok(true)
            }
        }
    }
}

/// The one statement `planned` sends through `repo`; its affected-row count.
pub(crate) fn write_planned(
    repo: &dyn OptionRepository,
    planned: &PlannedWrite,
) -> Result<u64, OptionsError> {
    match planned {
        PlannedWrite::Add { option, .. } => repo.add(option),
        PlannedWrite::Update {
            name,
            stored,
            autoload,
            ..
        } => repo.update(
            name,
            SerializedValue::for_update(stored)?.as_ref(),
            autoload.as_ref(),
        ),
        PlannedWrite::Delete { name, .. } => repo.remove(name),
    }
}
