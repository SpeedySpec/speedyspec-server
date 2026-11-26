//! Option writes through a unit of work: the checks and filters run when a write is staged, the write itself at
//! commit, and the cache updates and actions only after a successful commit.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_data::{ChangeKind, DbError, EngineHandle, Identified, PendingChange, UnitOfWork};
use speedyspec_data_uow::DefaultUnitOfWork;
use speedyspec_wp_hooks_contracts::HookValue;

use crate::api::OptionsApi;
use crate::write::{PlannedWrite, Staging, write_planned};
use crate::{OptionName, OptionRepository, OptionsError, SerializedValue};

/// What staging a write did. The cases other than `Registered` are where the direct function returns false
/// before writing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Staged {
    Registered,
    /// `update_option` with the value already stored.
    Unchanged,
    /// `add_option` of an option that exists.
    Exists,
    /// `delete_option` of an option that does not.
    Missing,
    /// A blank name, or `"0"`.
    Rejected,
}

/// One option's pending write in a unit of work, by name (the identity map's entry), and the row count its
/// commit gave.
struct StagedWrite {
    name: OptionName,
    planned: PlannedWrite,
    rows: Rc<Cell<u64>>,
    /// The write replaces a staged delete with an upsert: both stagings' finishes see a row written, even when the
    /// upsert left an identical row unchanged (0 rows).
    replaces_delete: bool,
}

impl Identified for StagedWrite {
    type Id = OptionName;

    fn identity(&self) -> OptionName {
        self.name.clone()
    }
}

/// The pending change: the option's write, applied at commit through the repository.
struct OptionChange {
    engine: String,
    repository: Rc<dyn OptionRepository>,
    slot: Rc<RefCell<StagedWrite>>,
}

impl PendingChange for OptionChange {
    fn engine(&self) -> &str {
        &self.engine
    }

    /// The kind of the write as it stands now: a later staging may have merged into it.
    fn kind(&self) -> ChangeKind {
        kind_of(&self.slot.borrow().planned)
    }

    fn apply(&self, _handle: &EngineHandle) -> Result<(), DbError> {
        let slot = self.slot.borrow();
        let rows =
            write_planned(&*self.repository, &slot.planned).map_err(|e| DbError::Engine {
                engine: self.engine.clone(),
                code: None,
                message: e.to_string(),
            })?;
        slot.rows.set(if slot.replaces_delete {
            rows.max(1)
        } else {
            rows
        });
        Ok(())
    }
}

fn kind_of(planned: &PlannedWrite) -> ChangeKind {
    match planned {
        PlannedWrite::Add { .. } => ChangeKind::New,
        PlannedWrite::Update { .. } => ChangeKind::Dirty,
        PlannedWrite::Delete { .. } => ChangeKind::Removed,
    }
}

/// upsert of the new row; a second update keeps the first one's explicit autoload when it sets none.
fn merge(first: &PlannedWrite, next: PlannedWrite) -> Result<(PlannedWrite, bool), OptionsError> {
    Ok(match (first, next) {
        (
            PlannedWrite::Add { option, .. },
            PlannedWrite::Update {
                value,
                stored,
                autoload,
                ..
            },
        ) => {
            let mut option = option.clone();
            option.value = SerializedValue::for_insert(&stored.read())?;
            if let Some(autoload) = autoload {
                option.autoload = autoload;
            }
            (
                PlannedWrite::Add {
                    option,
                    value,
                    stored,
                },
                false,
            )
        }
        (PlannedWrite::Delete { .. }, next @ PlannedWrite::Add { .. }) => (next, true),
        (
            PlannedWrite::Update {
                autoload: Some(first_autoload),
                ..
            },
            PlannedWrite::Update {
                name,
                old,
                value,
                stored,
                autoload: None,
            },
        ) => (
            PlannedWrite::Update {
                name,
                old,
                value,
                stored,
                autoload: Some(first_autoload.clone()),
            },
            false,
        ),
        (_, next) => (next, false),
    })
}

fn staging_of(uow: &DefaultUnitOfWork, name: &OptionName) -> Staging {
    match uow.identity_map().get::<StagedWrite>(name) {
        None => Staging::Unknown,
        Some(slot) => match &slot.borrow().planned {
            PlannedWrite::Add { value, .. } | PlannedWrite::Update { value, .. } => {
                Staging::Present(value.clone())
            }
            PlannedWrite::Delete { .. } => Staging::Absent,
        },
    }
}

impl OptionsApi {
    /// `update_option`, staged in `uow`.
    pub fn stage_update(
        &self,
        uow: &DefaultUnitOfWork,
        name: &str,
        value: HookValue,
        autoload: HookValue,
    ) -> Result<Staged, OptionsError> {
        let Some(option) = OptionName::new(name) else {
            return Ok(Staged::Rejected);
        };
        let staging = staging_of(uow, &option);
        match self.plan_update_over(name, value, autoload, &staging, false)? {
            Some(planned) => self.stage(uow, planned),
            None => Ok(Staged::Unchanged),
        }
    }

    /// `add_option`, staged in `uow`.
    pub fn stage_add(
        &self,
        uow: &DefaultUnitOfWork,
        name: &str,
        value: HookValue,
        autoload: HookValue,
    ) -> Result<Staged, OptionsError> {
        let Some(option) = OptionName::new(name) else {
            return Ok(Staged::Rejected);
        };
        let staging = staging_of(uow, &option);
        if matches!(staging, Staging::Present(_)) {
            return Ok(Staged::Exists);
        }
        match self.plan_add_in(
            name,
            value,
            HookValue::Str(String::new()),
            autoload,
            false,
            &staging,
        )? {
            Some(planned) => self.stage(uow, planned),
            None => Ok(Staged::Exists),
        }
    }

    /// `delete_option`, staged in `uow`.
    pub fn stage_delete(
        &self,
        uow: &DefaultUnitOfWork,
        name: &str,
    ) -> Result<Staged, OptionsError> {
        let Some(option) = OptionName::new(name) else {
            return Ok(Staged::Rejected);
        };
        let staged = uow
            .identity_map()
            .get::<StagedWrite>(&option)
            .map(|slot| slot.borrow().planned.clone());
        let planned = match staged {
            Some(PlannedWrite::Delete { .. }) => return Ok(Staged::Missing),
            // Staged here: the row the delete removes is the staged one, whatever the database holds now.
            Some(PlannedWrite::Add { option: entity, .. }) => Some(PlannedWrite::Delete {
                name: option,
                autoload: entity.autoload,
            }),
            Some(PlannedWrite::Update {
                autoload: Some(autoload),
                ..
            }) => Some(PlannedWrite::Delete {
                name: option,
                autoload,
            }),
            Some(PlannedWrite::Update { autoload: None, .. }) | None => {
                self.plan_delete(name, false)?
            }
        };
        match planned {
            Some(planned) => self.stage(uow, planned),
            None => Ok(Staged::Missing),
        }
    }

    fn stage(
        &self,
        uow: &DefaultUnitOfWork,
        planned: PlannedWrite,
    ) -> Result<Staged, OptionsError> {
        let name = match &planned {
            PlannedWrite::Add { option, .. } => option.name.clone(),
            PlannedWrite::Update { name, .. } | PlannedWrite::Delete { name, .. } => name.clone(),
        };
        let slot = match uow.identity_map().get::<StagedWrite>(&name) {
            Some(slot) => {
                let (merged, replaces_delete) = merge(&slot.borrow().planned, planned.clone())?;
                let mut staged = slot.borrow_mut();
                staged.planned = merged;
                staged.replaces_delete |= replaces_delete;
                drop(staged);
                slot
            }
            None => {
                let slot = uow.identity_map().insert(StagedWrite {
                    name,
                    planned: planned.clone(),
                    rows: Rc::new(Cell::new(0)),
                    replaces_delete: false,
                });
                uow.register(Box::new(OptionChange {
                    engine: self.d.repository.engine(),
                    repository: self.d.repository.clone(),
                    slot: slot.clone(),
                }));
                slot
            }
        };
        // Each staged write finishes as its direct function would, after the commit, with the row count it gave.
        let rows = slot.borrow().rows.clone();
        let api = OptionsApi::new(self.d.clone());
        uow.after_commit(Box::new(move || {
            // The before-action waits for the commit too: no action fires for a write that did not happen.
            if let Err(error) = api.before(&planned).and_then(|()| api.finish(&planned, rows.get()).map(|_| ())) {
                tracing::warn!(%error, "an option's cache update or action failed after its commit");
            }
        }));
        Ok(Staged::Registered)
    }
}
