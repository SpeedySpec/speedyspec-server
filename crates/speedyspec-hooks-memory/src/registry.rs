use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_hooks::{AddOptions, Callback, CallbackId, HookError, HookErrorKind, Hooks, Value};

struct Entry {
    id: CallbackId,
    priority: i64,
    accepted_args: usize,
    key: Option<String>,
    owner: Option<String>,
    callback: Rc<Callback>,
}

#[derive(Default)]
struct Hook {
    /// Kept sorted by (priority, insertion order) at insert time so firing is a
    /// straight walk. Inserting after every entry of equal or lower priority
    /// preserves insertion order within a priority.
    entries: Vec<Entry>,
    ran: u64,
}

#[derive(Default)]
struct Inner {
    /// Hook name -> index into `hooks`; names are interned on first use.
    names: HashMap<String, usize>,
    hooks: Vec<Hook>,
    next_id: u64,
}

impl Inner {
    fn intern(&mut self, name: &str) -> usize {
        if let Some(&i) = self.names.get(name) {
            return i;
        }
        let i = self.hooks.len();
        self.hooks.push(Hook::default());
        self.names.insert(name.to_owned(), i);
        i
    }

    fn lookup(&self, name: &str) -> Option<&Hook> {
        self.names.get(name).map(|&i| &self.hooks[i])
    }
}

/// What firing needs from an entry, copied out so the registry is not borrowed
/// while callbacks run (they may re-enter it).
struct Pending {
    accepted_args: usize,
    owner: Option<String>,
    callback: Rc<Callback>,
}

/// In-memory hook backend. Never returns `NotPortable` or `Backend`.
#[derive(Default)]
pub struct MemoryHooks {
    inner: RefCell<Inner>,
}

impl MemoryHooks {
    pub fn new() -> MemoryHooks {
        MemoryHooks::default()
    }

    /// Snapshot of the entries to fire. Changes made by a callback therefore
    /// apply from the next firing.
    fn snapshot(&self, hook: &str) -> Vec<Pending> {
        let inner = self.inner.borrow();
        inner.lookup(hook).map_or_else(Vec::new, |h| {
            h.entries
                .iter()
                .map(|e| Pending {
                    accepted_args: e.accepted_args,
                    owner: e.owner.clone(),
                    callback: Rc::clone(&e.callback),
                })
                .collect()
        })
    }
}

impl MemoryHooks {
    /// Takes the first entry of `hook` matching `matches` out of the registry. The entry is
    /// returned rather than dropped in place: dropping it can release the callback's last
    /// reference, which may run code that uses the registry again (a PHP destructor), so it
    /// must happen after the borrow has ended. The caller drops it.
    fn take_entry(&self, hook: &str, matches: impl Fn(&Entry) -> bool) -> Option<Entry> {
        let mut inner = self.inner.borrow_mut();
        let &i = inner.names.get(hook)?;
        let entries = &mut inner.hooks[i].entries;
        let at = entries.iter().position(matches)?;
        Some(entries.remove(at))
    }
}

fn callback_error(hook: &str, owner: &Option<String>, message: String) -> HookError {
    HookError {
        kind: HookErrorKind::Callback,
        hook: hook.to_owned(),
        owner: owner.clone(),
        message,
    }
}

fn visible(args: &[Value], accepted: usize) -> &[Value] {
    &args[..accepted.min(args.len())]
}

impl Hooks for MemoryHooks {
    fn add(
        &self,
        hook: &str,
        callback: Rc<Callback>,
        options: AddOptions,
    ) -> Result<CallbackId, HookError> {
        let mut inner = self.inner.borrow_mut();
        let id = CallbackId(inner.next_id);
        inner.next_id += 1;
        let index = inner.intern(hook);
        let entries = &mut inner.hooks[index].entries;
        let at = entries.partition_point(|e| e.priority <= options.priority);
        entries.insert(
            at,
            Entry {
                id,
                priority: options.priority,
                accepted_args: options.accepted_args,
                key: options.key,
                owner: options.owner,
                callback,
            },
        );
        Ok(id)
    }

    fn remove(&self, hook: &str, id: CallbackId) -> Result<bool, HookError> {
        let removed = self.take_entry(hook, |e| e.id == id);
        Ok(removed.is_some())
    }

    fn remove_keyed(&self, hook: &str, key: &str, priority: i64) -> Result<bool, HookError> {
        let removed = self.take_entry(hook, |e| {
            e.priority == priority && e.key.as_deref() == Some(key)
        });
        Ok(removed.is_some())
    }

    fn has(&self, hook: &str) -> Result<bool, HookError> {
        Ok(self
            .inner
            .borrow()
            .lookup(hook)
            .is_some_and(|h| !h.entries.is_empty()))
    }

    fn has_keyed(&self, hook: &str, key: &str) -> Result<bool, HookError> {
        Ok(self
            .inner
            .borrow()
            .lookup(hook)
            .is_some_and(|h| h.entries.iter().any(|e| e.key.as_deref() == Some(key))))
    }

    fn run(&self, hook: &str, args: &[Value]) -> Result<(), HookError> {
        {
            let mut inner = self.inner.borrow_mut();
            let i = inner.intern(hook);
            inner.hooks[i].ran += 1;
        }
        for p in self.snapshot(hook) {
            (p.callback)(visible(args, p.accepted_args))
                .map_err(|e| callback_error(hook, &p.owner, e.0))?;
        }
        Ok(())
    }

    fn filter(&self, hook: &str, value: Value, args: &[Value]) -> Result<Value, HookError> {
        let mut value = value;
        for p in self.snapshot(hook) {
            let mut call_args = Vec::with_capacity(1 + args.len());
            call_args.push(value);
            call_args.extend_from_slice(args);
            value = (p.callback)(visible(&call_args, p.accepted_args))
                .map_err(|e| callback_error(hook, &p.owner, e.0))?;
        }
        Ok(value)
    }

    fn ran(&self, hook: &str) -> Result<u64, HookError> {
        Ok(self.inner.borrow().lookup(hook).map_or(0, |h| h.ran))
    }
}
