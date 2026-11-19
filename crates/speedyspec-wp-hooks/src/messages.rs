//! Every Hook API function as a use case: a [`Command`] or a [`Query`], handled by [`HookApi`].
//!
//! [`register_hook_handlers`] binds [`HookApi`] as the handler of each one, under its `KEY`; PHP overrides one by
//! binding the same key.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler,
};

use crate::{
    ArrayKey, Callback, CallbackId, HasFilterResult, HookApi, HookError, HookName, HookValue,
};

/// `add_filter()`: always true.
#[derive(Clone, Debug)]
pub struct AddFilter {
    pub hook: HookName,
    pub callback: Callback,
    pub priority: HookValue,
    pub accepted_args: i64,
}

impl Command for AddFilter {
    type Result = Result<bool, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\AddFilterHandler";
}

impl CommandHandler<AddFilter> for HookApi {
    fn handle(&self, m: AddFilter) -> Result<bool, HookError> {
        self.add_filter(&m.hook, m.callback, &m.priority, m.accepted_args)
    }
}

/// `add_action()`.
#[derive(Clone, Debug)]
pub struct AddAction {
    pub hook: HookName,
    pub callback: Callback,
    pub priority: HookValue,
    pub accepted_args: i64,
}

impl Command for AddAction {
    type Result = Result<bool, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\AddActionHandler";
}

impl CommandHandler<AddAction> for HookApi {
    fn handle(&self, m: AddAction) -> Result<bool, HookError> {
        self.add_filter(&m.hook, m.callback, &m.priority, m.accepted_args)
    }
}

/// `remove_filter()`.
#[derive(Clone, Debug)]
pub struct RemoveFilter {
    pub hook: HookName,
    pub callback: CallbackId,
    pub priority: HookValue,
}

impl Command for RemoveFilter {
    type Result = Result<bool, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\RemoveFilterHandler";
}

impl CommandHandler<RemoveFilter> for HookApi {
    fn handle(&self, m: RemoveFilter) -> Result<bool, HookError> {
        self.remove_filter(&m.hook, m.callback, &m.priority)
    }
}

/// `remove_action()`.
#[derive(Clone, Debug)]
pub struct RemoveAction {
    pub hook: HookName,
    pub callback: CallbackId,
    pub priority: HookValue,
}

impl Command for RemoveAction {
    type Result = Result<bool, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\RemoveActionHandler";
}

impl CommandHandler<RemoveAction> for HookApi {
    fn handle(&self, m: RemoveAction) -> Result<bool, HookError> {
        self.remove_filter(&m.hook, m.callback, &m.priority)
    }
}

/// `remove_all_filters()`: `priority` is `None` for every priority.
#[derive(Clone, Debug)]
pub struct RemoveAllFilters {
    pub hook: HookName,
    pub priority: Option<HookValue>,
}

impl Command for RemoveAllFilters {
    type Result = Result<bool, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\RemoveAllFiltersHandler";
}

impl CommandHandler<RemoveAllFilters> for HookApi {
    fn handle(&self, m: RemoveAllFilters) -> Result<bool, HookError> {
        self.remove_all_filters(&m.hook, m.priority.as_ref())
    }
}

/// `remove_all_actions()`.
#[derive(Clone, Debug)]
pub struct RemoveAllActions {
    pub hook: HookName,
    pub priority: Option<HookValue>,
}

impl Command for RemoveAllActions {
    type Result = Result<bool, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\RemoveAllActionsHandler";
}

impl CommandHandler<RemoveAllActions> for HookApi {
    fn handle(&self, m: RemoveAllActions) -> Result<bool, HookError> {
        self.remove_all_filters(&m.hook, m.priority.as_ref())
    }
}

/// `apply_filters()`.
#[derive(Clone, Debug)]
pub struct ApplyFilters {
    pub hook: HookName,
    pub value: HookValue,
    pub args: Vec<HookValue>,
}

impl Command for ApplyFilters {
    type Result = Result<HookValue, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\ApplyFiltersHandler";
}

impl CommandHandler<ApplyFilters> for HookApi {
    fn handle(&self, m: ApplyFilters) -> Result<HookValue, HookError> {
        self.apply_filters(&m.hook, m.value, m.args)
    }
}

/// `apply_filters_ref_array()`.
#[derive(Clone, Debug)]
pub struct ApplyFiltersRefArray {
    pub hook: HookName,
    pub args: Vec<HookValue>,
}

impl Command for ApplyFiltersRefArray {
    type Result = Result<HookValue, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\ApplyFiltersRefArrayHandler";
}

impl CommandHandler<ApplyFiltersRefArray> for HookApi {
    fn handle(&self, m: ApplyFiltersRefArray) -> Result<HookValue, HookError> {
        self.apply_filters_ref_array(&m.hook, m.args)
    }
}

/// `apply_filters_deprecated()`.
#[derive(Clone, Debug)]
pub struct ApplyFiltersDeprecated {
    pub hook: HookName,
    pub args: Vec<HookValue>,
    pub version: HookValue,
    pub replacement: HookValue,
    pub message: HookValue,
}

impl Command for ApplyFiltersDeprecated {
    type Result = Result<HookValue, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\ApplyFiltersDeprecatedHandler";
}

impl CommandHandler<ApplyFiltersDeprecated> for HookApi {
    fn handle(&self, m: ApplyFiltersDeprecated) -> Result<HookValue, HookError> {
        self.apply_filters_deprecated(&m.hook, m.args, &m.version, &m.replacement, &m.message)
    }
}

/// `do_action()`.
#[derive(Clone, Debug)]
pub struct DoAction {
    pub hook: HookName,
    pub args: Vec<HookValue>,
}

impl Command for DoAction {
    type Result = Result<(), HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\DoActionHandler";
}

impl CommandHandler<DoAction> for HookApi {
    fn handle(&self, m: DoAction) -> Result<(), HookError> {
        self.do_action(&m.hook, m.args)
    }
}

/// `do_action_ref_array()`.
#[derive(Clone, Debug)]
pub struct DoActionRefArray {
    pub hook: HookName,
    pub args: Vec<HookValue>,
}

impl Command for DoActionRefArray {
    type Result = Result<(), HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\DoActionRefArrayHandler";
}

impl CommandHandler<DoActionRefArray> for HookApi {
    fn handle(&self, m: DoActionRefArray) -> Result<(), HookError> {
        self.do_action_ref_array(&m.hook, m.args)
    }
}

/// `do_action_deprecated()`.
#[derive(Clone, Debug)]
pub struct DoActionDeprecated {
    pub hook: HookName,
    pub args: Vec<HookValue>,
    pub version: HookValue,
    pub replacement: HookValue,
    pub message: HookValue,
}

impl Command for DoActionDeprecated {
    type Result = Result<(), HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\DoActionDeprecatedHandler";
}

impl CommandHandler<DoActionDeprecated> for HookApi {
    fn handle(&self, m: DoActionDeprecated) -> Result<(), HookError> {
        self.do_action_deprecated(&m.hook, m.args, &m.version, &m.replacement, &m.message)
    }
}

/// `has_filter()`: `callback` is `None` for `false`; `priority` only when it is an integer.
#[derive(Clone, Debug)]
pub struct HasFilter {
    pub hook: HookName,
    pub callback: Option<CallbackId>,
    pub priority: Option<i64>,
}

impl Query for HasFilter {
    type Result = Result<HasFilterResult, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\HasFilterHandler";
}

impl QueryHandler<HasFilter> for HookApi {
    fn handle(&self, m: HasFilter) -> Result<HasFilterResult, HookError> {
        self.has_filter(&m.hook, m.callback.as_ref(), m.priority)
    }
}

/// `has_action()`.
#[derive(Clone, Debug)]
pub struct HasAction {
    pub hook: HookName,
    pub callback: Option<CallbackId>,
    pub priority: Option<i64>,
}

impl Query for HasAction {
    type Result = Result<HasFilterResult, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\HasActionHandler";
}

impl QueryHandler<HasAction> for HookApi {
    fn handle(&self, m: HasAction) -> Result<HasFilterResult, HookError> {
        self.has_filter(&m.hook, m.callback.as_ref(), m.priority)
    }
}

/// `did_action()`.
#[derive(Clone, Debug)]
pub struct DidAction {
    pub hook: HookName,
}

impl Query for DidAction {
    type Result = Result<i64, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\DidActionHandler";
}

impl QueryHandler<DidAction> for HookApi {
    fn handle(&self, m: DidAction) -> Result<i64, HookError> {
        self.did_action(&m.hook)
    }
}

/// `did_filter()`.
#[derive(Clone, Debug)]
pub struct DidFilter {
    pub hook: HookName,
}

impl Query for DidFilter {
    type Result = Result<i64, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\DidFilterHandler";
}

impl QueryHandler<DidFilter> for HookApi {
    fn handle(&self, m: DidFilter) -> Result<i64, HookError> {
        self.did_filter(&m.hook)
    }
}

/// `doing_filter()`.
#[derive(Clone, Debug)]
pub struct DoingFilter {
    pub hook: Option<HookValue>,
}

impl Query for DoingFilter {
    type Result = bool;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\DoingFilterHandler";
}

impl QueryHandler<DoingFilter> for HookApi {
    fn handle(&self, m: DoingFilter) -> bool {
        self.doing_filter(m.hook.as_ref())
    }
}

/// `doing_action()`.
#[derive(Clone, Debug)]
pub struct DoingAction {
    pub hook: Option<HookValue>,
}

impl Query for DoingAction {
    type Result = bool;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\DoingActionHandler";
}

impl QueryHandler<DoingAction> for HookApi {
    fn handle(&self, m: DoingAction) -> bool {
        self.doing_filter(m.hook.as_ref())
    }
}

/// `current_filter()`: `None` is `false`.
#[derive(Clone, Debug)]
pub struct CurrentFilter;

impl Query for CurrentFilter {
    type Result = Option<HookValue>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\CurrentFilterHandler";
}

impl QueryHandler<CurrentFilter> for HookApi {
    fn handle(&self, _m: CurrentFilter) -> Option<HookValue> {
        self.current_filter()
    }
}

/// `current_action()`.
#[derive(Clone, Debug)]
pub struct CurrentAction;

impl Query for CurrentAction {
    type Result = Option<HookValue>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\CurrentActionHandler";
}

impl QueryHandler<CurrentAction> for HookApi {
    fn handle(&self, _m: CurrentAction) -> Option<HookValue> {
        self.current_filter()
    }
}

/// `WP_Hook::current_priority()` of a named hook.
#[derive(Clone, Debug)]
pub struct CurrentPriority {
    pub hook: HookName,
}

impl Query for CurrentPriority {
    type Result = Result<Option<ArrayKey>, HookError>;
    const KEY: &'static str = "SpeedySpec\\WpHooks\\CurrentPriorityHandler";
}

impl QueryHandler<CurrentPriority> for HookApi {
    fn handle(&self, m: CurrentPriority) -> Result<Option<ArrayKey>, HookError> {
        self.current_priority(&m.hook)
    }
}

/// Binds [`HookApi`] as the handler of every Hook API use case.
pub fn register_hook_handlers(container: &Container, api: Rc<HookApi>) {
    let a = api.clone();
    bind_command_handler::<AddFilter>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_command_handler::<AddAction>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_command_handler::<RemoveFilter>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_command_handler::<RemoveAction>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_command_handler::<RemoveAllFilters>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_command_handler::<RemoveAllActions>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_command_handler::<ApplyFilters>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_command_handler::<ApplyFiltersRefArray>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_command_handler::<ApplyFiltersDeprecated>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_command_handler::<DoAction>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_command_handler::<DoActionRefArray>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_command_handler::<DoActionDeprecated>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_query_handler::<HasFilter>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_query_handler::<HasAction>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_query_handler::<DidAction>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_query_handler::<DidFilter>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_query_handler::<DoingFilter>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_query_handler::<DoingAction>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_query_handler::<CurrentFilter>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_query_handler::<CurrentAction>(container, move |_, _| Ok(a.clone()));
    let a = api.clone();
    bind_query_handler::<CurrentPriority>(container, move |_, _| Ok(a.clone()));
}
