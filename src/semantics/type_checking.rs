use crate::parser::Type;

pub trait TypeCheck {
    type Output;

    fn type_check(&mut self) -> Result<Self::Output, SemanticErrorKind>;
}

#[derive(Debug, Clone)]
pub enum SemanticErrorKind {
    TypeMismatch { expected: Type, found: Type },
}
