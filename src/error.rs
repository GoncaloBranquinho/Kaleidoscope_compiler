use ::inkwell::support::LLVMString;
use inkwell::builder::BuilderError;
use llvm_sys::error::{LLVMErrorRef, LLVMGetErrorMessage};
use owo_colors::OwoColorize;
use std::{ffi::CStr, fmt};

use crate::{
    lexer::core::LexerError, parser::core::ParserErrorKind,
    semantics::type_checking::SemanticErrorKind,
};

#[derive(Debug)]
pub enum CompilerError {
    Io(std::io::Error),
    Lexer(LexerError),
    Parser(ParserErrorKind),
    Llvm(String),
    Semantic(SemanticErrorKind),
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompilerError::Io(error) => write!(f, "{}: {}", "error".red().bold(), error),
            CompilerError::Lexer(error) => write!(f, "{}: {}", "error".red().bold(), error),
            CompilerError::Parser(error) => write!(f, "{}: {}", "error".red().bold(), error),
            CompilerError::Llvm(error) => write!(f, "{}: {}", "error".red().bold(), error),
            CompilerError::Semantic(error) => {
                write!(f, "{}: {}", "error".red().bold(), error)
            }
        }
    }
}

impl fmt::Display for LexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexerError::UnknownChar(c) => write!(f, "Unknown character: {}", c),
        }
    }
}

impl fmt::Display for ParserErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParserErrorKind::LexerError(e) => write!(f, "{}", e),
            ParserErrorKind::UnexpectedToken(tok) => write!(f, "Unexpected token: {:?}", tok),
            ParserErrorKind::UnexpectedEof => write!(f, "Unexpected end of file"),
            ParserErrorKind::InvalidSignatuere => write!(f, "Invalid signature"),
        }
    }
}

impl fmt::Display for SemanticErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SemanticErrorKind::TypeMismatch { expected, found } => write!(
                f,
                "Type mismatch: expected: '{:?}', found: '{:?}'",
                expected, found
            ),
        }
    }
}

impl From<ParserErrorKind> for CompilerError {
    fn from(error: ParserErrorKind) -> Self {
        match error {
            ParserErrorKind::LexerError(e) => CompilerError::Lexer(e),
            _ => CompilerError::Parser(error),
        }
    }
}

impl From<std::io::Error> for CompilerError {
    fn from(error: std::io::Error) -> Self {
        CompilerError::Io(error)
    }
}

impl From<LLVMString> for CompilerError {
    fn from(error: LLVMString) -> Self {
        CompilerError::Llvm(error.to_string_lossy().to_string())
    }
}

impl From<BuilderError> for CompilerError {
    fn from(error: BuilderError) -> Self {
        CompilerError::Llvm(format!("LLVM builder error: {:?}", error))
    }
}

impl From<SemanticErrorKind> for CompilerError {
    fn from(error: SemanticErrorKind) -> Self {
        CompilerError::Semantic(error)
    }
}

impl CompilerError {
    fn from_aux(err: LLVMErrorRef) -> Self {
        unsafe {
            let message = LLVMGetErrorMessage(err);
            let e = CStr::from_ptr(message).to_string_lossy().into_owned();
            CompilerError::Llvm(e)
        }
    }
}

impl From<LLVMErrorRef> for CompilerError {
    fn from(err: LLVMErrorRef) -> Self {
        CompilerError::from_aux(err)
    }
}

impl std::error::Error for CompilerError {}
