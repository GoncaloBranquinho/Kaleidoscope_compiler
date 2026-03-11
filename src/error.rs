use crate::{
    lexer::tokens::{LexingError, Token},
    llvm::codegen::IRError,
};

use ::inkwell::support::LLVMString;
use lalrpop_util::ParseError;
use owo_colors::OwoColorize;
use std::fmt;

#[derive(Debug)]
pub enum CompilerError {
    Io(std::io::Error),
    Lexer(LexingError),
    Parser(ParseError<usize, Token, LexingError>),
    Llvm(String),
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Def => write!(f, "def"),
            Token::Extern => write!(f, "extern"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::Comma => write!(f, ","),
            Token::GreaterEq => write!(f, ">="),
            Token::LessEq => write!(f, "<="),
            Token::Less => write!(f, "<"),
            Token::Greater => write!(f, ">"),
            Token::Equal => write!(f, "="),
            Token::NotEqual => write!(f, "/="),
            Token::Add => write!(f, "+"),
            Token::Sub => write!(f, "-"),
            Token::Div => write!(f, "/"),
            Token::Mult => write!(f, "*"),
            Token::Identifier(s) => write!(f, "{s}"),
            Token::Number(n) => write!(f, "{n}"),
            Token::Whitespace | Token::Newline => Ok(()),
        }
    }
}

impl fmt::Display for LexingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid token `{}` found at {}:{}",
            self.token, self.row, self.col
        )
    }
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompilerError::Io(error) => write!(f, "{}: {}", "error".red().bold(), error),
            CompilerError::Lexer(error) => write!(f, "{}: {}", "error".red().bold(), error),
            CompilerError::Parser(error) => write!(f, "{}: {}", "error".red().bold(), error),
            CompilerError::Llvm(error) => write!(f, "{}: {}", "error".red().bold(), error),
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
        match error {
            ParseError::User { error: e } => CompilerError::Lexer(e),
            _ => CompilerError::Parser(error),
        }
    }
}

impl From<std::io::Error> for CompilerError {
    fn from(error: std::io::Error) -> Self {
        CompilerError::Io(error)
    }
}

impl From<IRError> for CompilerError {
    fn from(error: IRError) -> Self {
        CompilerError::Llvm(error.message_error)
    }
}

impl From<LLVMString> for CompilerError {
    fn from(error: LLVMString) -> Self {
        CompilerError::Llvm(error.to_string_lossy().to_string())
    }
}

impl std::error::Error for CompilerError {}
