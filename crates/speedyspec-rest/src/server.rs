//! The REST server: WordPress's serve order (Template Method) over the operation dispatcher.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_http_message::{HttpResponse, HttpServerRequest};
use speedyspec_operation::Registries;
use speedyspec_operation_contracts::{
    Action, Caller, Category, OperationCatalog, OperationError, SchemaKind, Value,
};
use speedyspec_operation_dispatch::Dispatcher;
use speedyspec_rest_contracts::{
    RestHooks, RestMethod, RestOutcome, RestRequest, RestResponse, SharedRequest,
};
use speedyspec_transport::Exposure;
use speedyspec_transport_contracts::Authenticator;

use crate::middleware::{DispatchRequestMiddleware, Dispatching};
use crate::transport::REST;
use crate::{RouteMatch, RouteTable, coerce, decode, encode, errors, fields, index, links, query};

/// Serves REST requests in WordPress trunk's order:
///
/// 1. `rest_authentication_errors`, then the authenticator resolves the caller;
/// 2. `rest_pre_dispatch` (may answer);
/// 3. route matching (`rest_no_route`), and the index routes;
/// 4. `rest_request_before_callbacks` (before the permission check; writes reach the handler);
/// 5. the permission check; 6. `rest_dispatch_request` (may answer); 7. the handler;
/// 8. `rest_prepare_{type}` per item;
/// 9. `rest_request_after_callbacks` (sees errors);
/// 10. errors encoded as responses; 11. `_links` and `_embed`;
/// 12. `rest_post_dispatch`, then `_fields`; 13. `rest_pre_serve_request`.
pub struct RestServer {
    registries: Rc<Registries>,
    dispatcher: Dispatcher,
    table: RouteTable,
    problems: Vec<String>,
    hooks: Rc<dyn RestHooks>,
    authenticator: Rc<dyn Authenticator<HttpServerRequest>>,
    base_url: String,
}

fn item_type(kind: &SchemaKind) -> Option<(String, bool)> {
    match kind {
        SchemaKind::Ref { type_name } => Some((type_name.clone(), false)),
        SchemaKind::Array { items, .. } => match &items.kind {
            SchemaKind::Ref { type_name } => Some((type_name.clone(), true)),
            _ => None,
        },
        _ => None,
    }
}

fn not_found() -> OperationError {
    OperationError::new("operation_not_found", Category::NotFound, "no route")
}

impl RestServer {
    /// A server for what `exposure` exposes on `rest`, dispatching through its own dispatcher over `registries` and the
    /// handlers in `container`. `base_url` is the REST root links are written under (`https://example.com/wp-json`).
    pub fn new(
        container: Rc<Container>,
        registries: Rc<Registries>,
        exposure: Rc<Exposure>,
        hooks: Rc<dyn RestHooks>,
        authenticator: Rc<dyn Authenticator<HttpServerRequest>>,
        base_url: &str,
    ) -> RestServer {
        let (table, problems) = RouteTable::build(exposure.as_ref(), hooks.as_ref());
        let dispatcher = Dispatcher::new(container, registries.clone()).with_middleware(Rc::new(
            DispatchRequestMiddleware {
                hooks: hooks.clone(),
            },
        ));
        RestServer {
            registries,
            dispatcher,
            table,
            problems,
            hooks,
            authenticator,
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    /// Routes that could not be added, and why.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    pub fn table(&self) -> &RouteTable {
        &self.table
    }

    /// The response to `request`, or `None` when it is not a REST request.
    pub fn serve(&self, request: &HttpServerRequest) -> Option<HttpResponse> {
        let rest = decode::rest_request(request)?;
        let head = rest.head;
        let shared: SharedRequest = Rc::new(RefCell::new(rest));
        let response = match self
            .hooks
            .authentication_errors()
            .and_then(|()| self.authenticator.authenticate(request))
        {
            Err(error) => errors::error_response(&error),
            Ok(mut caller) => {
                caller.transport = REST.to_owned();
                let response = self.dispatch(shared.clone(), &caller);
                let mut response = self.hooks.post_dispatch(response, &shared);
                if let Some(Value::String(wanted)) = shared.borrow().query_params.get("_fields")
                    && response.status < 400
                {
                    let wanted: Vec<String> = wanted
                        .split(',')
                        .map(|f| f.trim().to_owned())
                        .filter(|f| !f.is_empty())
                        .collect();
                    response.data = fields::filter(&response.data, &wanted);
                }
                response
            }
        };
        let served = self.hooks.pre_serve(&response, &shared);
        Some(encode::http_response(&response, head || served))
    }

    /// Steps 2–11 for one request: what `rest_do_request` answers. `_embed` is read from the request's query.
    pub fn dispatch(&self, request: SharedRequest, caller: &Caller) -> RestResponse {
        if let Some(outcome) = self.hooks.pre_dispatch(&request) {
            return outcome.unwrap_or_else(|e| errors::error_response(&e));
        }
        let (method, path) = {
            let r = request.borrow();
            (r.method, r.path.clone())
        };
        let (operation, route, url_params) = match self.table.find(method, &path) {
            RouteMatch::Found {
                operation,
                route,
                url_params,
            } => (operation, route, url_params),
            RouteMatch::NoRoute => return self.index_or_no_route(method, &path),
        };
        {
            let mut r = request.borrow_mut();
            r.route = Some(route);
            r.operation = Some(operation.clone());
            r.url_params = url_params;
        }
        let embed = request.borrow().query_params.contains_key("_embed");
        let outcome = match self.hooks.before_callbacks(&request) {
            Err(error) => Err(error),
            Ok(()) => self.call(&operation, &request, caller, embed),
        };
        let mut response = self
            .hooks
            .after_callbacks(outcome, &request)
            .unwrap_or_else(|e| errors::error_response(&e));
        if embed {
            links::embed(&mut response, &|href| self.embedded(href, caller));
        } else {
            links::render(&mut response);
        }
        response
    }

    fn index_or_no_route(&self, method: RestMethod, path: &str) -> RestResponse {
        if method == RestMethod::Get {
            let trimmed = path.trim_matches('/');
            if trimmed.is_empty() {
                return RestResponse::ok(index::root(
                    &self.table,
                    self.registries.as_ref(),
                    self.registries.as_ref(),
                ));
            }
            if index::namespaces(&self.table)
                .iter()
                .any(|ns| ns.eq_ignore_ascii_case(trimmed))
            {
                return RestResponse::ok(index::namespace(
                    &self.table,
                    trimmed,
                    self.registries.as_ref(),
                    self.registries.as_ref(),
                ));
            }
        }
        errors::error_response(&not_found())
    }

    /// Steps 5–8: the call with the request's (possibly rewritten) params, then each item prepared and linked.
    fn call(
        &self,
        operation: &str,
        request: &SharedRequest,
        caller: &Caller,
        embed: bool,
    ) -> RestOutcome {
        let spec = self
            .registries
            .get_operation(operation)
            .ok_or_else(not_found)?;
        let params = Value::Map(request.borrow().params());
        let input = coerce::coerce(&spec.input.schema, &params, self.registries.as_ref());
        let guard = Dispatching::enter(request.clone());
        let result = self.dispatcher.call(operation, input, caller);
        let answered = guard.answered();
        drop(guard);
        let data = result?;
        if let Some(response) = answered {
            return Ok(response);
        }
        let status = match spec.resource.as_ref().map(|r| &r.action) {
            Some(Action::Create) => 201,
            _ => 200,
        };
        let Some((type_name, list)) = item_type(&spec.output.schema.kind) else {
            return Ok(RestResponse {
                status,
                ..RestResponse::ok(data)
            });
        };
        if !list {
            let mut response = self.hooks.prepare_item(
                &type_name,
                RestResponse {
                    status,
                    ..RestResponse::ok(data)
                },
                request,
            );
            response.links = self.links(&type_name, &response.data);
            return Ok(response);
        }
        let Value::List(items) = data else {
            return Ok(RestResponse {
                status,
                ..RestResponse::ok(data)
            });
        };
        let items = items
            .into_iter()
            .map(|item| {
                let mut one = self
                    .hooks
                    .prepare_item(&type_name, RestResponse::ok(item), request);
                one.links = self.links(&type_name, &one.data);
                if embed {
                    links::embed(&mut one, &|href| self.embedded(href, caller));
                } else {
                    links::render(&mut one);
                }
                one.data
            })
            .collect();
        Ok(RestResponse {
            status,
            ..RestResponse::ok(Value::List(items))
        })
    }

    fn links(&self, type_name: &str, item: &Value) -> Vec<speedyspec_rest_contracts::RestLink> {
        links::for_item(
            type_name,
            item,
            self.registries.as_ref(),
            self.registries.as_ref(),
            &self.table,
            &self.base_url,
        )
    }

    /// An embedded link's response: a GET of its path through [`RestServer::dispatch`], then `rest_post_dispatch`, as
    /// WordPress embeds. Embedded responses are not embedded again.
    fn embedded(&self, href: &str, caller: &Caller) -> RestResponse {
        let local = href.strip_prefix(&self.base_url).unwrap_or(href);
        let (path, raw_query) = local.split_once('?').unwrap_or((local, ""));
        let mut sub = RestRequest::new(RestMethod::Get, path);
        sub.query_params = query::parse(raw_query);
        sub.query_params.shift_remove("_embed");
        let sub: SharedRequest = Rc::new(RefCell::new(sub));
        let response = self.dispatch(sub.clone(), caller);
        self.hooks.post_dispatch(response, &sub)
    }
}
