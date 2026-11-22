//! The serve pipeline every transport runs.

use std::marker::PhantomData;
use std::rc::Rc;

use speedyspec_operation_contracts::{Category, OperationError};
use speedyspec_operation_dispatch::Dispatcher;
use speedyspec_transport_contracts::{Authenticator, Codec, ExposureLookup};

use crate::Exposure;

/// Serves one transport's requests (Template Method): authenticate, decode, check the transport exposes the
/// operation, dispatch, encode. Any failure is encoded with the codec's error form.
pub struct Pipeline<R, S> {
    transport: String,
    authenticator: Rc<dyn Authenticator<R>>,
    codec: Rc<dyn Codec<R, S>>,
    dispatcher: Rc<Dispatcher>,
    exposure: Rc<Exposure>,
    _types: PhantomData<fn(&R) -> S>,
}

impl<R, S> Pipeline<R, S> {
    pub fn new(
        transport: &str,
        authenticator: Rc<dyn Authenticator<R>>,
        codec: Rc<dyn Codec<R, S>>,
        dispatcher: Rc<Dispatcher>,
        exposure: Rc<Exposure>,
    ) -> Self {
        Pipeline {
            transport: transport.to_owned(),
            authenticator,
            codec,
            dispatcher,
            exposure,
            _types: PhantomData,
        }
    }

    pub fn serve(&self, request: &R) -> S {
        match self.run(request) {
            Ok(response) => response,
            Err(error) => self.codec.encode_error(request, &error),
        }
    }

    fn run(&self, request: &R) -> Result<S, OperationError> {
        let mut caller = self.authenticator.authenticate(request)?;
        // The pipeline, not the authenticator, decides which transport a call came through.
        caller.transport = self.transport.clone();
        let call = self.codec.decode(request, self.exposure.as_ref())?;
        if let Some(fields) = &call.requested_fields {
            caller = caller.with_requested_fields(fields.clone());
        }
        if self
            .exposure
            .find(&self.transport, &call.operation)
            .is_none()
        {
            return Err(OperationError::new(
                "operation_not_found",
                Category::NotFound,
                format!("no operation {}", call.operation),
            ));
        }
        let output = self
            .dispatcher
            .call(&call.operation, call.input.clone(), &caller)?;
        Ok(self.codec.encode(request, &call, output))
    }
}
