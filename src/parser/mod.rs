pub mod core;
pub mod decl;
pub mod expr;
pub mod literals;
pub mod op;
pub mod program;
pub mod types;

pub use decl::{DeclKind, Prototype};
pub use expr::{Expr, ExprKind};
pub use literals::Literal;
//pub use parser;
pub use core::Parser;
pub use program::Program;
pub use types::{Type, TypeKind};
