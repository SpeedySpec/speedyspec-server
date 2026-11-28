//! [`OptionsApi`] as the options contracts' ports (Adapter): every method delegates to the inherent method of the
//! same WordPress function, so the trait and the API answer alike.

use speedyspec_wp_hooks_contracts::HookValue;

use crate::{ConditionalOptions, ConditionalWrite, Options, OptionsApi, OptionsError};

impl Options for OptionsApi {
    fn get_option(
        &self,
        name: &str,
        default: Option<HookValue>,
    ) -> Result<HookValue, OptionsError> {
        OptionsApi::get_option(self, name, default)
    }

    fn add_option(
        &self,
        name: &str,
        value: HookValue,
        deprecated: HookValue,
        autoload: HookValue,
    ) -> Result<bool, OptionsError> {
        OptionsApi::add_option(self, name, value, deprecated, autoload)
    }

    fn update_option(
        &self,
        name: &str,
        value: HookValue,
        autoload: HookValue,
    ) -> Result<bool, OptionsError> {
        OptionsApi::update_option(self, name, value, autoload)
    }

    fn delete_option(&self, name: &str) -> Result<bool, OptionsError> {
        OptionsApi::delete_option(self, name)
    }
}

impl ConditionalOptions for OptionsApi {
    fn update_option_if(
        &self,
        name: &str,
        expected: Option<&[u8]>,
        value: HookValue,
        autoload: HookValue,
    ) -> Result<ConditionalWrite, OptionsError> {
        OptionsApi::update_option_if_unchanged(self, name, expected, value, autoload)
    }

    fn stored_bytes(&self, name: &str) -> Result<Option<Vec<u8>>, OptionsError> {
        OptionsApi::stored_bytes(self, name)
    }

    fn forget_cached(&self, name: &str) -> Result<(), OptionsError> {
        OptionsApi::forget_cached(self, name)
    }

    fn serialized_form(&self, value: &HookValue) -> Result<Vec<u8>, OptionsError> {
        OptionsApi::serialized_form(self, value)
    }
}
