use crate::parser::{BinaryOp, Literal, Type, UnaryOp};

pub type Expr = Box<ExprKind>;

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Identifier(String, Option<Type>),
    Var(Vec<VarInfo>, Expr),
    Literal(Literal),
    Tuple(Vec<Expr>),
    Binary(BinaryOp, Expr, Expr),
    Seq(Vec<Expr>),
    Unary(UnaryOp, Expr),
    IfThenElse(Expr, Expr, Expr),
    Call(String, Vec<Expr>),
    ForLoop(String, Expr, Expr, Option<Expr>, Expr),
    Projection(Expr, Expr),
    Pair(Option<Expr>, Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub struct VarInfo {
    pub name: String,
    pub t: Option<Type>,
    pub val: Option<Expr>,
}

impl From<Literal> for ExprKind {
    fn from(l: Literal) -> Self {
        ExprKind::Literal(l)
    }
}
