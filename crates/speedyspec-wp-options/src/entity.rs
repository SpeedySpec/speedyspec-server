//! The option entity: a plain object that knows nothing of how it is stored.

use speedyspec_data::Identified;

use crate::{Autoload, OptionName, SerializedValue};

/// One row of `wp_options`, by name. `option_id` is the table's concern, not the entity's.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Option {
    pub name: OptionName,
    pub value: SerializedValue,
    pub autoload: Autoload,
}

impl Identified for Option {
    type Id = OptionName;

    fn identity(&self) -> OptionName {
        self.name.clone()
    }
}
