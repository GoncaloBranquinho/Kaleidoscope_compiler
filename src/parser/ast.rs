#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub name: String,
    pub typ: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prototype {
    pub name: String,
    pub args: Vec<Arg>,
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
    Lt,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Var {
        name: String,
    },
    DoubleLit {
        value: f64,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    IfThenElse {
        cond: Box<Expr>,
        fst: Box<Expr>,
        snd: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Box<Expr>>,
    },
    ForLoop {
        var_name: String,
        start: Box<Expr>,
        end: Box<Expr>,
        step: Box<Expr>,
        body: Box<Expr>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Decl {
    Function { proto: Prototype, body: Box<Expr> },
    Extern(Prototype),
}
