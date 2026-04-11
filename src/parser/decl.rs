use crate::parser::{expr::Expr, types::Type};

#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub name: String,
    pub typ: Type,
}

impl Arg {
    pub fn new(name: String, typ: Type) -> Self {
        Arg { name, typ }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prototype {
    pub name: String,
    pub args: Vec<Arg>,
    pub ret_type: Type,
}

impl Prototype {
    pub fn new(name: String, args: Vec<Arg>, ret_type: Type) -> Self {
        Prototype {
            name,
            args,
            ret_type,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DeclKind {
    Extern(Prototype),
    Function(Prototype, Expr),
}
