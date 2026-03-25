use std::fmt;

use logos::Logos;

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

    #[regex(r"[0-9]+(\.[0-9]+)?", |lex| lex.slice().parse::<f64>().unwrap())]
    Number(f64),

    #[regex(r"[ \t]+", |lex| { lex.extras.1 += lex.slice().len(); logos::Skip })]
    Whitespace,

    #[regex(r"\n", |lex| { lex.extras.0 += 1; lex.extras.1 = 1; logos::Skip })]
    Newline,
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
