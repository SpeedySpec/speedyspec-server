use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_data::{DbError, DialectKind, EmulatedOp, Statement};
use speedyspec_data_query::{PassChain, PassContext, Rewrite, RewritePass, Rewritten, parse_one};

struct Recording {
    name: &'static str,
    log: Rc<RefCell<Vec<String>>>,
    action: fn(&Statement) -> Rewrite,
}

impl RewritePass for Recording {
    fn name(&self) -> &'static str {
        self.name
    }
    fn rewrite(&self, stmt: &mut Statement, _: &PassContext) -> Result<Rewrite, DbError> {
        self.log
            .borrow_mut()
            .push(format!("{}: {}", self.name, stmt.tree()));
        Ok((self.action)(stmt))
    }
}

fn cx() -> PassContext<'static> {
    PassContext {
        target: DialectKind::Sqlite,
        introspector: None,
    }
}

#[test]
fn passes_run_in_order_and_split() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut chain = PassChain::new();
    chain.push(Box::new(Recording {
        name: "split",
        log: log.clone(),
        action: |_| {
            Rewrite::Split(vec![
                parse_one("SELECT 1").unwrap(),
                parse_one("SELECT 2").unwrap(),
            ])
        },
    }));
    chain.push(Box::new(Recording {
        name: "second",
        log: log.clone(),
        action: |_| Rewrite::Continue,
    }));
    match chain.run(parse_one("SELECT 0").unwrap(), &cx()).unwrap() {
        Rewritten::Statements(stmts) => assert_eq!(stmts.len(), 2),
        Rewritten::Emulated(op) => panic!("unexpected {op:?}"),
    }
    assert_eq!(
        *log.borrow(),
        vec!["split: SELECT 0", "second: SELECT 1", "second: SELECT 2"]
    );
}

#[test]
fn emulate_stops_chain() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut chain = PassChain::new();
    chain.push(Box::new(Recording {
        name: "emulate",
        log: log.clone(),
        action: |_| Rewrite::Emulate(EmulatedOp::FoundRows),
    }));
    chain.push(Box::new(Recording {
        name: "never",
        log: log.clone(),
        action: |_| Rewrite::Continue,
    }));
    assert_eq!(
        chain
            .run(parse_one("SELECT FOUND_ROWS()").unwrap(), &cx())
            .unwrap(),
        Rewritten::Emulated(EmulatedOp::FoundRows)
    );
    assert_eq!(log.borrow().len(), 1);
}
