use crate::lexer::keywords::Keyword;

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Number(String),

    Identifier(String),

    Keyword(Keyword),

    Op(char),
}

impl TokenKind {
    pub fn into_op(self) -> char {
        if let TokenKind::Op(c) = self {
            c
        } else {
            panic!("Found {self:?} but expected the Op variatnt")
        }
    }

    pub fn into_identifier(self) -> String {
        if let TokenKind::Identifier(i) = self {
            i
        } else {
            panic!("Found {self:?} but expected the Identifier variatnt")
        }
    }

    pub fn into_keyword(self) -> Keyword {
        if let TokenKind::Keyword(k) = self {
            k
        } else {
            panic!("Found {self:?} but expected the Keyword variatnt")
        }
    }

    pub fn into_number(self) -> String {
        if let TokenKind::Number(n) = self {
            n
        } else {
            panic!("Found {self:?} but expected the Op variatnt")
        }
    }
}
