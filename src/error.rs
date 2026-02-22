use lalrpop_util::ParseError;
use crate::lexer::tokens::{Token, LexingError};
use std::fmt;


#[derive(Debug)]
pub enum CompilerError {
    Io(std::io::Error),
    Lexer(LexingError),
    Parser(ParseError<usize, Token, LexingError>),
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompilerError::Io(error) => write!(f, "IO Error: {error:?}"),
            CompilerError::Lexer(error)  => write!(f, "Lexer Error: {error:?}"), 
            CompilerError::Parser(error) => write!(f, "Parse Error: {error:?}"),
        }
    }
}

impl From<LexingError> for CompilerError {
    fn from(error: LexingError) -> Self {
        CompilerError::Lexer(error)
    }
}

impl From<ParseError<usize, Token, LexingError>> for CompilerError {
    fn from(error: ParseError<usize, Token, LexingError>) -> Self {
        CompilerError::Parser(error)
    }
}

impl From<std::io::Error> for CompilerError {
    fn from(error: std::io::Error) -> Self {
        CompilerError::Io(error)
    }
}

impl std::error::Error for CompilerError {}
