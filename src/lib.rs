pub mod lexer;
pub mod parser;
pub mod error;

pub use error::CompilerError;
use lalrpop_util::lalrpop_mod;

lalrpop_mod!(pub grammar, "/parser/grammar.rs");
