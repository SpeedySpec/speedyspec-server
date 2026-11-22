//! The transport registry.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_operation::Registries;
use speedyspec_operation_contracts::OperationCatalog;
use speedyspec_transport_contracts::{
    BindingSource, BindingStore, ExposedOperation, ExposureError, Transport,
};

use crate::Exposure;

/// Installed transports, and the bindings they read.
pub struct TransportRegistry {
    bindings: Rc<dyn BindingStore>,
    transports: RefCell<BTreeMap<String, Rc<dyn Transport>>>,
}

impl TransportRegistry {
    pub fn new(bindings: Rc<dyn BindingStore>) -> Self {
        TransportRegistry {
            bindings,
            transports: RefCell::new(BTreeMap::new()),
        }
    }

    pub fn install(&self, transport: Rc<dyn Transport>) -> Result<(), ExposureError> {
        let name = transport.name().as_str().to_owned();
        let mut transports = self.transports.borrow_mut();
        if transports.contains_key(&name) {
            return Err(ExposureError::DuplicateTransport { transport: name });
        }
        transports.insert(name, transport);
        Ok(())
    }

    /// Installed transport names, sorted.
    pub fn transports(&self) -> Vec<String> {
        self.transports.borrow().keys().cloned().collect()
    }

    /// Which operations each installed transport exposes. Unrestricted operations go on every transport; an explicit
    /// binding wins over the transport's default mapping; bindings for transports not installed are kept and ignored.
    pub fn expose(&self, registries: &Registries) -> Result<Exposure, Vec<ExposureError>> {
        if !registries.is_closed() {
            return Err(vec![ExposureError::RegistryOpen]);
        }
        let transports = self.transports.borrow();
        let mut errors = Vec::new();
        for (operation, transport, _) in self.bindings.bindings() {
            match registries.get_operation(&operation) {
                None => errors.push(ExposureError::UnknownOperation {
                    operation,
                    transport,
                }),
                Some(spec)
                    if transports.contains_key(&transport) && !spec.exposed_on(&transport) =>
                {
                    errors.push(ExposureError::OutsideRestriction {
                        operation,
                        transport,
                    })
                }
                Some(_) => {}
            }
        }
        let mut exposure = Exposure::default();
        for (name, transport) in transports.iter() {
            let exposed = exposure.by_transport.entry(name.clone()).or_default();
            for operation in registries.operation_names() {
                let Some(spec) = registries.get_operation(&operation) else {
                    continue;
                };
                if !spec.exposed_on(name) {
                    continue;
                }
                let (binding, source) = match self.bindings.binding(&operation, name) {
                    Some(binding) => {
                        if let Err(why) = transport.check_binding(&spec, &binding) {
                            errors.push(ExposureError::InvalidBinding {
                                operation: operation.clone(),
                                transport: name.clone(),
                                why,
                            });
                            continue;
                        }
                        (binding, BindingSource::Explicit)
                    }
                    None => match transport.default_binding(&spec) {
                        Some(binding) => (binding, BindingSource::Default),
                        None => continue,
                    },
                };
                exposed.insert(
                    operation,
                    Rc::new(ExposedOperation {
                        spec,
                        binding,
                        source,
                    }),
                );
            }
        }
        if errors.is_empty() {
            Ok(exposure)
        } else {
            Err(errors)
        }
    }
}
