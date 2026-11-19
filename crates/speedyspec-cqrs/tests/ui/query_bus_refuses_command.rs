use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Command, QueryBus};

struct AddPost;

impl Command for AddPost {
    type Result = ();
    const KEY: &'static str = "AddPostHandler";
}

fn main() {
    let bus = QueryBus::new(Rc::new(Container::new()));
    let _ = bus.ask(AddPost);
}
