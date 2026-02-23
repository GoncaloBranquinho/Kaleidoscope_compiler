use logos::{Logos, Lexer as LogosLexer};

use crate::lexer::tokens::{Token, LexingError};

pub type Spanned<Tok, Loc, Error> = Result<(Loc, Tok, Loc), Error>;

pub struct Lexer<'input> {
    lexer: LogosLexer<'input, Token>,
    input: &'input str,
}

impl<'input> Lexer<'input> {
  pub fn new(input: &'input str) -> Self {
    let mut lexer = Token::lexer(input);
    lexer.extras = (1, 1);
    Self { lexer, input, }
  }
}

impl<'input> Iterator for Lexer<'input> {
  type Item = Spanned<Token, usize, LexingError>;

  fn next(&mut self) -> Option<Self::Item> {
      let cur = self.lexer.next()?;
      let span = self.lexer.span();
      let size = span.end - span.start;
      let row = self.lexer.extras.0;
      let col = self.lexer.extras.1;
      match cur {
          Ok(token) => {
            self.lexer.extras.1 += size;
            Some(Ok((row,token,col)))
          }
          Err(_) => {
            Some(Err(LexingError {
                   token: self.input[span.start..span.end].to_string(),
                   row: row,
                   col: col,
            }))
          }
      }
  }
}
