//! A computed exposure.

use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_transport_contracts::{ExposedOperation, ExposureError, ExposureLookup};

/// Which operations each transport exposes, with which binding. Computed by `TransportRegistry::expose`. Bindings that
/// could not be used are skipped and listed in [`Exposure::problems`], so one plugin's bad binding never takes the
/// other operations down (as WordPress answers a bad `register_rest_route` with `_doing_it_wrong` and keeps serving).
#[derive(Default)]
pub struct Exposure {
    pub(crate) by_transport: BTreeMap<String, BTreeMap<String, Rc<ExposedOperation>>>,
    pub(crate) problems: Vec<ExposureError>,
}

impl Exposure {
    /// The bindings that were skipped, and why.
    pub fn problems(&self) -> &[ExposureError] {
        &self.problems
    }
}

impl ExposureLookup for Exposure {
    fn find(&self, transport: &str, operation: &str) -> Option<Rc<ExposedOperation>> {
        self.by_transport.get(transport)?.get(operation).cloned()
    }

    fn exposed(&self, transport: &str) -> Vec<Rc<ExposedOperation>> {
        self.by_transport
            .get(transport)
            .map(|ops| ops.values().cloned().collect())
            .unwrap_or_default()
    }
}

impl std::fmt::Debug for Exposure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(
                self.by_transport
                    .iter()
                    .map(|(transport, ops)| (transport, ops.keys().collect::<Vec<_>>())),
            )
            .finish()?;
        if !self.problems.is_empty() {
            write!(f, " with problems {:?}", self.problems)?;
        }
        Ok(())
    }
}
