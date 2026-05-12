use crate::parser::{BinaryOp, Literal, Type, UnaryOp};

pub type Expr = Box<ExprKind>;

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Identifier(String, Option<Type>),
    Var(Vec<VarInfo>, Expr),
    Literal(Literal),
    Cons(Vec<Expr>, ConsKind),
    Binary(BinaryOp, Expr, Expr),
    Seq(Vec<Expr>),
    Unary(UnaryOp, Expr),
    IfThenElse(Expr, Expr, Expr),
    Call(String, Vec<Expr>, Option<Type>),
    ForLoop(String, Expr, Expr, Option<Expr>, Expr),
    Projection(Expr, Expr, Option<Type>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConsKind {
    List,
    Tuple,
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
