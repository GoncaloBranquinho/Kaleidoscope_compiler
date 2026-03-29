use std::{collections::HashMap, iter::Peekable};

use crate::{
    lexer::{
        core::{LexerError, TokenResult},
        keywords::Keyword::*,
        tokens::TokenKind,
    },
    parser::{DeclKind, Expr, ExprKind, Literal, Program, Prototype, TypeKind, decl::Arg},
};

pub struct Parser<I: Iterator> {
    errors: Vec<ParserErrorKind>,
    token_stream: Peekable<I>,
    binop_precedence: HashMap<char, i8>,
}

impl<I: Iterator<Item = TokenResult>> Parser<I> {
    pub fn new(token_stream: I) -> Self {
        let mut binop_precedence = HashMap::new();
        binop_precedence.insert('<', 10);
        binop_precedence.insert('+', 20);
        binop_precedence.insert('-', 20);
        binop_precedence.insert('*', 40);

        Parser {
            errors: Vec::new(),
            token_stream: token_stream.peekable(),
            binop_precedence,
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, Vec<ParserErrorKind>> {
        let mut program = Vec::new();

        loop {
            let token = match self.opt_peek_token() {
                Ok(Some(t)) => t,
                Ok(None) => break,
                Err(e) => {
                    let _ = self.consume_token();
                    self.errors.push(e);
                    continue;
                }
            };

            match token {
                TokenKind::Keyword(Def) => self.error_recovery(&mut program, Self::parse_def),
                TokenKind::Keyword(Extern) => self.error_recovery(&mut program, Self::parse_extern),
                TokenKind::Op(';') => {
                    let _ = self.consume_token();
                }
                _ => self.error_recovery(&mut program, Self::parse_top_level_expr),
            }
        }

        if self.errors.is_empty() {
            Ok(program)
        } else {
            Err(self.errors.clone())
        }
    }

    fn parse_prototype(&mut self) -> Result<Prototype, ParserErrorKind> {
        let id = match self.peek_token()? {
            TokenKind::Identifier(id) => {
                self.consume_token()?;
                id.clone()
            }
            t => return Err(ParserErrorKind::UnexpectedToken(t)),
        };

        match self.peek_token()? {
            TokenKind::Op('(') => {
                self.consume_token()?;
            }
            t => return Err(ParserErrorKind::UnexpectedToken(t)),
        }

        let mut args = Vec::new();

        loop {
            match self.peek_token()? {
                TokenKind::Identifier(i) => {
                    self.consume_token()?;
                    args.push(Arg::new(i.clone(), Box::new(TypeKind::F64)));
                }
                TokenKind::Op(')') => break,
                t => return Err(ParserErrorKind::UnexpectedToken(t)),
            }
        }

        self.consume_token()?;
        Ok(Prototype::new(id, args))
    }

    fn parse_extern(&mut self) -> Result<DeclKind, ParserErrorKind> {
        self.consume_token()?;
        Ok(DeclKind::Extern(self.parse_prototype()?))
    }

    fn parse_def(&mut self) -> Result<DeclKind, ParserErrorKind> {
        self.consume_token()?;
        let proto = self.parse_prototype()?;
        let e = self.parse_expr()?;
        Ok(DeclKind::Function(proto, e))
    }

    fn parse_top_level_expr(&mut self) -> Result<DeclKind, ParserErrorKind> {
        let e = self.parse_expr()?;
        let proto = Prototype::new("__anon_expr".to_string(), Vec::new());
        Ok(DeclKind::Function(proto, e))
    }

    fn parse_primary(&mut self) -> Result<Expr, ParserErrorKind> {
        match self.peek_token()? {
            TokenKind::Identifier(_) => self.parse_identifier(),
            TokenKind::Number(_) => self.parse_number(),
            TokenKind::Op('(') => self.parse_paren(),
            TokenKind::Keyword(If) => self.parse_if(),
            TokenKind::Keyword(For) => self.parse_for(),
            t => Err(ParserErrorKind::UnexpectedToken(t)),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, ParserErrorKind> {
        let lhs = self.parse_primary()?;
        self.parse_bin_op_rhs(0, lhs)
    }

    fn parse_bin_op_rhs(&mut self, expr_prec: i8, mut lhs: Expr) -> Result<Expr, ParserErrorKind> {
        loop {
            let tok_prec = self.get_token_precedence()?;
            if tok_prec < expr_prec {
                return Ok(lhs);
            }

            let bin_op = self.consume_token()?;
            let mut rhs = self.parse_primary()?;
            let next_prec = self.get_token_precedence()?;
            if tok_prec < next_prec {
                rhs = self.parse_bin_op_rhs(tok_prec + 1, rhs)?;
            }
            lhs = Box::new(ExprKind::Binary(bin_op.into_op().into(), lhs, rhs));
        }
    }

    fn get_token_precedence(&mut self) -> Result<i8, ParserErrorKind> {
        let token = match self.opt_peek_token()? {
            Some(t) => t,
            None => return Ok(-1),
        };
        if let TokenKind::Op(c) = token {
            let tok_prec = self.binop_precedence.get(&c);
            if c.is_ascii()
                && let Some(prec) = tok_prec
            {
                return Ok(*prec);
            }
        }
        Ok(-1)
    }

    fn consume_token(&mut self) -> Result<TokenKind, ParserErrorKind> {
        match self.token_stream.next() {
            Some(Ok(t)) => Ok(t),
            Some(Err(e)) => e.into(),
            None => Err(ParserErrorKind::UnexpectedEof),
        }
    }

    fn opt_peek_token(&mut self) -> Result<Option<TokenKind>, ParserErrorKind> {
        match self.token_stream.peek() {
            Some(Ok(t)) => Ok(Some(t.clone())),
            Some(Err(e)) => (*e).into(),
            None => Ok(None),
        }
    }

    fn peek_token(&mut self) -> Result<TokenKind, ParserErrorKind> {
        match self.opt_peek_token()? {
            Some(t) => Ok(t),
            None => Err(ParserErrorKind::UnexpectedEof),
        }
    }

    fn parse_for(&mut self) -> Result<Expr, ParserErrorKind> {
        self.consume_token()?;

        let id = match self.peek_token()? {
            TokenKind::Identifier(id) => {
                self.consume_token()?;
                id.clone()
            }
            t => return Err(ParserErrorKind::UnexpectedToken(t)),
        };

        match self.peek_token()? {
            TokenKind::Op('=') => {
                self.consume_token()?;
            }
            t => return Err(ParserErrorKind::UnexpectedToken(t)),
        }

        let start = self.parse_expr()?;

        match self.peek_token()? {
            TokenKind::Op(',') => {
                self.consume_token()?;
            }
            t => return Err(ParserErrorKind::UnexpectedToken(t)),
        }

        let end = self.parse_expr()?;

        let mut step = None;

        if let TokenKind::Op(',') = self.peek_token()? {
            self.consume_token()?;
            step = Some(self.parse_expr()?);
        }
        match self.peek_token()? {
            TokenKind::Keyword(In) => {
                self.consume_token()?;
            }
            t => return Err(ParserErrorKind::UnexpectedToken(t)),
        }

        let body = self.parse_expr()?;

        Ok(Box::new(ExprKind::ForLoop(id, start, end, step, body)))
    }

    fn parse_if(&mut self) -> Result<Expr, ParserErrorKind> {
        self.consume_token()?;

        let cond = self.parse_expr()?;

        match self.peek_token()? {
            TokenKind::Keyword(Then) => {
                self.consume_token()?;
            }
            t => return Err(ParserErrorKind::UnexpectedToken(t)),
        }
        let then = self.parse_expr()?;

        match self.peek_token()? {
            TokenKind::Keyword(Else) => {
                self.consume_token()?;
            }
            t => return Err(ParserErrorKind::UnexpectedToken(t)),
        }
        let elsee = self.parse_expr()?;

        Ok(Box::new(ExprKind::IfThenElse(cond, then, elsee)))
    }

    fn parse_paren(&mut self) -> Result<Expr, ParserErrorKind> {
        self.consume_token()?;
        let v = self.parse_expr()?;

        match self.peek_token()? {
            TokenKind::Op(')') => {
                self.consume_token()?;
                Ok(v)
            }
            t => Err(ParserErrorKind::UnexpectedToken(t)),
        }
    }

    fn parse_number(&mut self) -> Result<Expr, ParserErrorKind> {
        let number = self.consume_token()?;
        let literal = Literal::F64(number.into_number().parse().unwrap());
        Ok(Box::new(literal.into()))
    }

    fn parse_identifier(&mut self) -> Result<Expr, ParserErrorKind> {
        let identifier = self.consume_token()?.into_identifier();
        let token = self.opt_peek_token()?;
        if token.is_none() || !matches!(token, Some(TokenKind::Op('('))) {
            let expr_var = ExprKind::Var(identifier);
            return Ok(Box::new(expr_var));
        }

        self.consume_token()?;

        let mut args: Vec<Expr> = Vec::new();

        if !matches!(self.peek_token()?, TokenKind::Op(')')) {
            loop {
                args.push(self.parse_expr()?);
                match self.peek_token()? {
                    TokenKind::Op(')') => break,
                    TokenKind::Op(',') => {
                        self.consume_token()?;
                    }
                    t => return Err(ParserErrorKind::UnexpectedToken(t)),
                }
            }
        }
        self.consume_token()?;
        let expr_call = ExprKind::Call(identifier.clone(), args);
        Ok(Box::new(expr_call))
    }

    fn error_recovery<F>(&mut self, program: &mut Vec<DeclKind>, f: F)
    where
        F: Fn(&mut Self) -> Result<DeclKind, ParserErrorKind>,
    {
        match f(self) {
            Ok(decl) => program.push(decl),
            Err(err) => {
                self.errors.push(err);
                let _ = self.consume_token();
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum ParserErrorKind {
    LexerError(LexerError),
    UnexpectedToken(TokenKind),
    UnexpectedEof,
}

impl<T> From<LexerError> for Result<T, ParserErrorKind> {
    fn from(e: LexerError) -> Self {
        Err(ParserErrorKind::LexerError(e))
    }
}
