#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Literal {
    F64(f64),
    I64(i64),
    Unit,
}
