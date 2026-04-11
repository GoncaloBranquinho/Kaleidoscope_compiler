use std::{iter::Peekable, str::CharIndices};

use crate::lexer::{Typ, keywords::Keyword, tokens::TokenKind};

pub type TokenResult = Result<TokenKind, LexerError>;

#[derive(Clone, Debug)]
pub struct Lexer<'s> {
    iter: Peekable<CharIndices<'s>>,
}

impl<'s> Lexer<'s> {
    pub fn new(iter: Peekable<CharIndices<'s>>) -> Self {
        Lexer { iter }
    }

    fn next_char(&mut self) -> Option<char> {
        if let Some(c) = self.iter.next() {
            let (_, ch) = c;
            return Some(ch);
        }
        None
    }

    fn peek_char(&mut self) -> Option<char> {
        if let Some(c) = self.iter.peek() {
            let &(_, ch) = c;
            return Some(ch);
        }
        None
    }
}

impl<'s> Iterator for Lexer<'s> {
    type Item = TokenResult;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(c) = self.peek_char() {
            if c.is_whitespace() {
                self.next_char();
            } else {
                break;
            }
        }

        let token = match self.peek_char() {
            Some(a) if a.is_alphabetic() => {
                let mut id_str = String::new();
                while let Some(a) = self.peek_char() {
                    if a.is_alphanumeric() {
                        id_str.push(a);
                        self.next_char();
                    } else {
                        break;
                    }
                }

                if let Ok(keyword) = id_str.parse::<Keyword>() {
                    Ok(TokenKind::Keyword(keyword))
                } else {
                    Ok(TokenKind::Identifier(id_str.clone()))
                }
            }

            Some(n) if n.is_numeric() => {
                let mut n_str = String::new();
                while let Some(n) = self.peek_char() {
                    if n.is_numeric() {
                        n_str.push(n);
                        self.next_char();
                    } else {
                        break;
                    }
                }
                if self.peek_char() == Some('.') {
                    n_str.push('.');
                    self.next_char();
                    while let Some(n) = self.peek_char() {
                        if n.is_numeric() {
                            n_str.push(n);
                            self.next_char();
                        } else {
                            break;
                        }
                    }
                    Ok(TokenKind::Number(Typ::Double(n_str.parse().unwrap())))
                } else {
                    Ok(TokenKind::Number(Typ::Int(n_str.parse().unwrap())))
                }
            }
            Some(op) => {
                self.next_char();
                Ok(TokenKind::Op(op))
            }
            None => {
                return None;
            }
        };
        Some(token)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LexerError {
    UnknownChar(char),
}
