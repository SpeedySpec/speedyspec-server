//! Native areas: what each part of the speedyspec_wordpress extension adds to PHP (functions, classes) and does at
//! the start and end of every request. They are given to [`crate::Php::boot`] and fixed for the process.

use std::cell::RefCell;
use std::rc::Rc;

use ext_php_rs::builders::FunctionBuilder;

/// One area of the extension.
pub trait NativeArea {
    /// Global functions this area adds; registered once, at module startup.
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        Vec::new()
    }

    /// Registers this area's classes (with `ext_php_rs::builders::ClassBuilder`); runs once, at module startup.
    fn register_classes(&self) -> Result<(), String> {
        Ok(())
    }

    /// Runs after each request has started and the extension's own PHP setup has run, before any other PHP code.
    fn request_start(&self) {}

    /// Runs as each request ends, also after a fatal error or `exit()` (from the module's request shutdown).
    fn request_end(&self) {}
}

thread_local! {
    static AREAS: RefCell<Vec<Rc<dyn NativeArea>>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn install(areas: Vec<Rc<dyn NativeArea>>) {
    AREAS.with(|a| *a.borrow_mut() = areas);
}

pub(crate) fn uninstall() {
    let areas = AREAS.with(|a| std::mem::take(&mut *a.borrow_mut()));
    drop(areas);
}

fn each(f: impl Fn(&dyn NativeArea)) {
    let areas = AREAS.with(|a| a.borrow().clone());
    for area in &areas {
        f(area.as_ref());
    }
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    let areas = AREAS.with(|a| a.borrow().clone());
    areas.iter().flat_map(|area| area.functions()).collect()
}

pub(crate) fn register_classes() -> Result<(), String> {
    let areas = AREAS.with(|a| a.borrow().clone());
    for area in &areas {
        area.register_classes()?;
    }
    Ok(())
}

pub(crate) fn request_start() {
    each(|area| area.request_start());
}

/// The module's RSHUTDOWN.
pub(crate) extern "C" fn request_shutdown(_ty: i32, _module_number: i32) -> i32 {
    let ended = std::panic::catch_unwind(|| each(|area| area.request_end()));
    if ended.is_ok() { 0 } else { 1 }
}
