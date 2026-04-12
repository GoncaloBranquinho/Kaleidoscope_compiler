use crate::parser::{BinaryOp, Literal, Type, UnaryOp};

pub type Expr = Box<ExprKind>;

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Identifier(String, Option<Type>),
    Var(Vec<((String, Option<Type>), Option<Expr>)>, Expr),
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
