use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_http::{Handler, Middleware};
use speedyspec_http_pipeline::Pipeline;

type Log = Rc<RefCell<Vec<&'static str>>>;

struct Step {
    name: &'static str,
    log: Log,
    answer: bool,
}

impl Middleware<u32, String> for Step {
    fn process(&self, request: u32, next: &dyn Handler<u32, String>) -> String {
        self.log.borrow_mut().push(self.name);
        if self.answer {
            return format!("{} answered {request}", self.name);
        }
        next.handle(request)
    }
}

struct End(Log);

impl Handler<u32, String> for End {
    fn handle(&self, request: u32) -> String {
        self.0.borrow_mut().push("end");
        format!("end {request}")
    }
}

fn pipeline(log: &Log, answering: Option<&'static str>) -> Pipeline<u32, String> {
    let mut pipeline = Pipeline::new(Rc::new(End(log.clone())));
    for name in ["a", "b", "c"] {
        pipeline.pipe(Rc::new(Step {
            name,
            log: log.clone(),
            answer: answering == Some(name),
        }));
    }
    pipeline
}

#[test]
fn middleware_runs_in_pipe_order() {
    let log: Log = Rc::default();
    assert_eq!(pipeline(&log, None).handle(7), "end 7");
    assert_eq!(*log.borrow(), vec!["a", "b", "c", "end"]);
}

#[test]
fn middleware_can_short_circuit() {
    let log: Log = Rc::default();
    assert_eq!(pipeline(&log, Some("b")).handle(7), "b answered 7");
    assert_eq!(*log.borrow(), vec!["a", "b"]);
}

#[test]
fn empty_pipeline_runs_the_final_handler() {
    let log: Log = Rc::default();
    let pipeline: Pipeline<u32, String> = Pipeline::new(Rc::new(End(log.clone())));
    assert_eq!(pipeline.handle(1), "end 1");
}
