pub type Type = Box<TypeKind>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TypeKind {
    F64,
    I64,
}
