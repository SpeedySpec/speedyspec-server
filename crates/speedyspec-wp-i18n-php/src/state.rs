//! What the native functions share: the API over the request's controller, the other controllers and file objects
//! PHP made this request, and which `WP_Translations` objects stand for which domain.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_i18n::{FileHandle, I18nApi, TranslationController, TranslationFile};
use speedyspec_wp_i18n_contracts::TranslationFileCache;
use speedyspec_wp_i18n_files::StdFileProbe;
use speedyspec_wp_i18n_memory::MemoryTranslationFileCache;

use crate::reader::PhpFallbackReader;

pub(crate) struct Shared {
    pub api: Rc<I18nApi>,
    reader: Rc<PhpFallbackReader>,
    probe: Rc<StdFileProbe>,
    cache: Rc<dyn TranslationFileCache<TranslationFile>>,
    /// Controllers made with `new WP_Translation_Controller()`, by id (the request's own is 0, in `api`).
    controllers: RefCell<Vec<Rc<RefCell<TranslationController>>>>,
    /// File objects made with `WP_Translation_File::create()`, by id.
    pub files: RefCell<Vec<Rc<FileHandle>>>,
    /// `spl_object_id` of each `WP_Translations` object, with its domain and controller id.
    pub translations: RefCell<HashMap<u32, (String, i64)>>,
    /// Calls the hot path served, and calls it left to WordPress's function.
    pub fast: Cell<u64>,
    pub slow: Cell<u64>,
    /// Bumped every request, so per-request lookups cached by the hot path start over.
    pub generation: Cell<u64>,
}

thread_local! {
    static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

pub(crate) fn install(shared: Rc<Shared>) {
    SHARED.with(|s| *s.borrow_mut() = Some(shared));
}

pub(crate) fn shared() -> Option<Rc<Shared>> {
    SHARED.with(|s| s.borrow().clone())
}

impl Shared {
    pub fn new(hooks: Rc<HookApi>) -> Shared {
        let reader = Rc::new(PhpFallbackReader::default());
        let probe = Rc::new(StdFileProbe);
        let cache: Rc<dyn TranslationFileCache<TranslationFile>> =
            Rc::new(MemoryTranslationFileCache::default());
        let controller = TranslationController::new(reader.clone(), probe.clone(), cache.clone());
        let api = Rc::new(I18nApi::new(hooks, Rc::new(RefCell::new(controller))));
        Shared {
            api,
            reader,
            probe,
            cache,
            controllers: RefCell::new(Vec::new()),
            files: RefCell::new(Vec::new()),
            translations: RefCell::new(HashMap::new()),
            generation: Cell::new(0),
            fast: Cell::new(0),
            slow: Cell::new(0),
        }
    }

    pub fn start_request(&self) {
        self.generation.set(self.generation.get() + 1);
    }

    pub fn end_request(&self) {
        self.api.reset();
        self.controllers.borrow_mut().clear();
        self.files.borrow_mut().clear();
        self.translations.borrow_mut().clear();
        self.generation.set(self.generation.get() + 1);
    }

    /// A new controller over the same reader, probe and file cache; its id.
    pub fn new_controller(&self) -> i64 {
        let c =
            TranslationController::new(self.reader.clone(), self.probe.clone(), self.cache.clone());
        let mut list = self.controllers.borrow_mut();
        list.push(Rc::new(RefCell::new(c)));
        list.len() as i64
    }

    /// The controller with `id`.
    pub fn controller(&self, id: i64) -> Option<Rc<RefCell<TranslationController>>> {
        if id == 0 {
            return Some(self.api.controller().clone());
        }
        let i = usize::try_from(id).ok()?.checked_sub(1)?;
        self.controllers.borrow().get(i).cloned()
    }
}
