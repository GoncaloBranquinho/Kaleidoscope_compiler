pub type Type = Box<TypeKind>;

#[derive(Clone, Debug, PartialEq)]
pub enum TypeKind {
    F64,
    I64,
    Unit,
    Tuple(Vec<Type>),
    List(Option<Type>),
}
