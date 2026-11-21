//! [`KsesApi`] behind the contracts' inbound ports.

use std::rc::Rc;

use speedyspec_wp_kses_contracts::{
    AllowedHtml, AllowedHtmlSource, AllowedSpec, Context, Kses, KsesError, Protocols, SafeCss,
    UriAttributes,
};

use crate::KsesApi;

impl Kses for KsesApi {
    fn kses(
        &self,
        content: &[u8],
        allowed: &AllowedSpec,
        protocols: Option<&Protocols>,
    ) -> Result<Vec<u8>, KsesError> {
        KsesApi::kses(self, content, allowed, protocols)
    }
    fn one_attr(&self, attr: &[u8], element: &[u8]) -> Result<Vec<u8>, KsesError> {
        KsesApi::one_attr(self, attr, element)
    }
    fn post(&self, data: &[u8]) -> Result<Vec<u8>, KsesError> {
        KsesApi::post(self, data)
    }
    fn data(&self, data: &[u8]) -> Result<Vec<u8>, KsesError> {
        KsesApi::data(self, data)
    }
}

impl AllowedHtmlSource for KsesApi {
    fn allowed_html(&self, ctx: &Context) -> Result<Rc<AllowedHtml>, KsesError> {
        KsesApi::allowed_html(self, ctx)
    }
    fn uri_attributes(&self) -> Result<Rc<UriAttributes>, KsesError> {
        KsesApi::uri_attributes(self)
    }
}

impl SafeCss for KsesApi {
    fn safecss_filter_attr(&self, css: &[u8]) -> Result<Vec<u8>, KsesError> {
        KsesApi::safecss_filter_attr(self, css)
    }
}
