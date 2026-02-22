#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub name: String,
    pub typ: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Double,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mult,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UnaryOp {
 Aa,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
  Var { name: String },
  DoubleLit { value: f64 },
  Binary { op: BinaryOp, left: Box<Expr>, right: Box<Expr>},
  Unary { op: UnaryOp, expr: Box<Expr>},
  Call { name: String, args: Vec<Box<Expr>>},
}

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
  FunctionDecl {
    name: String,
    args: Vec<Arg>,
    body: Box<Expr>,
  },
}
