/// Attributes shared by the routes registered inside [`crate::Router::group`]: a path prefix, middleware and a name
/// prefix. Nested groups concatenate prefixes and append middleware.
#[derive(Debug, Clone)]
pub struct Group<M> {
    pub(crate) prefix: String,
    pub(crate) middleware: Vec<M>,
    pub(crate) name: String,
}

impl<M> Group<M> {
    pub fn new() -> Group<M> {
        Group {
            prefix: String::new(),
            middleware: Vec::new(),
            name: String::new(),
        }
    }

    pub fn prefix(mut self, prefix: &str) -> Group<M> {
        self.prefix = prefix.trim_end_matches('/').to_string();
        self
    }

    pub fn middleware(mut self, middleware: M) -> Group<M> {
        self.middleware.push(middleware);
        self
    }

    pub fn name(mut self, name: &str) -> Group<M> {
        self.name = name.to_string();
        self
    }
}

impl<M> Default for Group<M> {
    fn default() -> Group<M> {
        Group::new()
    }
}
