use crate::lexer::keywords::Keyword;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Typ {
    Double(f64),
    Int(i64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Number(Typ),

    Identifier(String),

    Keyword(Keyword),

    Op(char),
}

impl TokenKind {
    pub fn into_op(self) -> char {
        if let TokenKind::Op(c) = self {
            c
        } else {
            panic!("Found {self:?} but expected the Op variant")
        }
    }

    pub fn into_identifier(self) -> String {
        if let TokenKind::Identifier(i) = self {
            i
        } else {
            panic!("Found {self:?} but expected the Identifier variant")
        }
    }

    pub fn into_keyword(self) -> Keyword {
        if let TokenKind::Keyword(k) = self {
            k
        } else {
            panic!("Found {self:?} but expected the Keyword variant")
        }
    }
}

impl Typ {
    pub fn into_int(self) -> i64 {
        if let Typ::Int(n) = self {
            n
        } else {
            panic!("Found {self:?} but expected the Int variant")
        }
    }
    pub fn into_double(self) -> f64 {
        if let Typ::Double(n) = self {
            n
        } else {
            panic!("Found {self:?} but expected the Double variant")
        }
    }
}
