use crate::parser::{BinaryOp, Literal, TypeKind, UnaryOp};

pub type Expr = Box<ExprKind>;

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Identifier(String),
    Var(Vec<((String, TypeKind), Option<Expr>)>, Expr),
    Literal(Literal),
    Binary(BinaryOp, Expr, Expr),
    Unary(UnaryOp, Expr),
    IfThenElse(Expr, Expr, Expr),
    Call(String, Vec<Expr>),
    ForLoop(String, Expr, Expr, Option<Expr>, Expr),
}

impl From<Literal> for ExprKind {
    fn from(l: Literal) -> Self {
        ExprKind::Literal(l)
    }
}
