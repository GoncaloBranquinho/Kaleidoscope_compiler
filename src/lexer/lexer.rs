use logos::{Logos, SpannedIter};

use crate::lexer::tokens::{Token, LexingError};

pub type Spanned<Tok, Loc, Error> = Result<(Loc, Tok, Loc), Error>;

pub struct Lexer<'input> {
    token_stream: SpannedIter<'input, Token>,
    input: &'input str,
}

impl<'input> Lexer<'input> {
  pub fn new(input: &'input str) -> Self {
    Self { token_stream: Token::lexer(input).spanned(), input, }
  }

}

impl<'input> Iterator for Lexer<'input> {
  type Item = Spanned<Token, usize, LexingError>;

  fn next(&mut self) -> Option<Self::Item> {
      match self.token_stream.next() {
          Some((Ok(token),span)) => {   
            Some(Ok((span.start,token,span.end)))
          }
          Some((Err(_), span)) => {
            Some(Err(LexingError {
                    token: self.input[span.start..span.end].to_string(),
                    row: span.start,
                    col: span.end,
                }))
          }
          None => None
      }
  }
}
