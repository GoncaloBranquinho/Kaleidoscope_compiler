use logos::{Logos};

#[derive(Debug, Clone, PartialEq)]
pub struct LexingError {
    pub token: String,
    pub row: usize,
    pub col: usize, 
}

#[derive(Debug)]
pub struct TokenInfo {
   pub token: Token,
   pub row: usize,
   pub col: usize,
}


#[derive(Logos, Debug, PartialEq, Clone)]
#[logos( extras = (usize, usize))]
pub enum Token {
    
    #[token("def")]
    Def,
    
    #[token("extern")]
    Extern,
    
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token(",")]
    Comma,

    #[token(">=")]
    GreaterEq,
    #[token("<=")]
    LessEq,
    #[token("<")]
    Less,
    #[token(">")]
    Greater,
    #[token("=")]
    Equal,
    #[token("/=")]
    NotEqual,

    #[token("+")]
    Add,
    #[token("-")]
    Sub,
    #[token("/")]
    Div,
    #[token("*")]
    Mult,

    #[regex(r"[a-zA-Z][a-zA-Z0-9]*", |lex| lex.slice().to_string())]    
    Identifier(String),
    
    #[regex(r"[0-9]+\.[0-9]+", |lex| lex.slice().parse::<f64>().unwrap())]
    Number(f64),

    #[regex(r"[ \t]+", logos::skip)]
    Whitespace,

    #[regex(r"\n", logos::skip)]
    Newline,
}

