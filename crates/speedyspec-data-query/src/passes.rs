//! Rewrite passes (Visitor over the tree, Chain of Responsibility over the passes).

use speedyspec_data::{DbError, DialectKind, EmulatedOp, Introspector, Statement};

/// What a pass knows: the target dialect and, when the engine is open, its schema.
pub struct PassContext<'a> {
    pub target: DialectKind,
    pub introspector: Option<&'a dyn Introspector>,
}

/// A pass's verdict on one statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Rewrite {
    /// Keep going with the (possibly changed) statement.
    Continue,
    /// Stop: the engine answers this itself.
    Emulate(EmulatedOp),
    /// Replace the statement with these; each runs through the passes after this one.
    Split(Vec<Statement>),
}

/// One rewrite of a statement's tree toward a target dialect.
pub trait RewritePass {
    fn name(&self) -> &'static str;
    fn rewrite(&self, stmt: &mut Statement, cx: &PassContext) -> Result<Rewrite, DbError>;
}

/// The chain's result.
#[derive(Debug, Clone, PartialEq)]
pub enum Rewritten {
    Statements(Vec<Statement>),
    Emulated(EmulatedOp),
}

/// Passes in order.
#[derive(Default)]
pub struct PassChain {
    passes: Vec<Box<dyn RewritePass>>,
}

impl PassChain {
    pub fn new() -> PassChain {
        PassChain::default()
    }

    pub fn push(&mut self, pass: Box<dyn RewritePass>) {
        self.passes.push(pass);
    }

    pub fn run(&self, stmt: Statement, cx: &PassContext) -> Result<Rewritten, DbError> {
        self.run_from(0, stmt, cx)
    }

    fn run_from(
        &self,
        first: usize,
        mut stmt: Statement,
        cx: &PassContext,
    ) -> Result<Rewritten, DbError> {
        for (i, pass) in self.passes.iter().enumerate().skip(first) {
            match pass.rewrite(&mut stmt, cx)? {
                Rewrite::Continue => {}
                Rewrite::Emulate(op) => return Ok(Rewritten::Emulated(op)),
                Rewrite::Split(parts) => {
                    let mut out = Vec::new();
                    for part in parts {
                        match self.run_from(i + 1, part, cx)? {
                            Rewritten::Statements(stmts) => out.extend(stmts),
                            Rewritten::Emulated(op) => {
                                return Err(DbError::Unsupported {
                                    construct: format!(
                                        "emulating part of a split statement ({op:?})"
                                    ),
                                    dialect: cx.target,
                                });
                            }
                        }
                    }
                    return Ok(Rewritten::Statements(out));
                }
            }
        }
        Ok(Rewritten::Statements(vec![stmt]))
    }
}
