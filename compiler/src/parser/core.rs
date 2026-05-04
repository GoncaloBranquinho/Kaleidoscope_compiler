use std::{collections::HashMap, iter::Peekable};

use crate::{
    lexer::{
        Typ,
        core::{LexerError, TokenResult},
        keywords::Keyword::*,
        tokens::TokenKind,
    },
    parser::{
        DeclKind, Expr, ExprKind, Literal, Program, Prototype, Type, TypeKind,
        decl::Arg,
        expr::{ConsKind, VarInfo},
    },
};

pub struct Parser<'a, I: Iterator> {
    errors: Vec<ParserErrorKind>,
    token_stream: Peekable<I>,
    binop_precedence: &'a mut HashMap<char, i8>,
}

impl<'a, I: Iterator<Item = TokenResult>> Parser<'a, I> {
    pub fn new(token_stream: I, binop_precedence: &'a mut HashMap<char, i8>) -> Self {
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
                /*TokenKind::Op(';') => {
                    let _ = self.consume_token();
                }*/
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
        let kind: u8;

        let id = match self.peek_token()? {
            TokenKind::Identifier(id) => {
                self.consume_token()?;
                kind = 0;
                id.clone()
            }
            TokenKind::Keyword(Binary) => {
                self.consume_token()?;
                match self.peek_token()? {
                    TokenKind::Op(c) if c.is_ascii() => {
                        kind = 2;
                        self.consume_token()?;
                        if let TokenKind::Number(n) = self.peek_token()? {
                            let num: i64 = n.into_int();
                            if !(1..=100).contains(&num) {
                                return Err(ParserErrorKind::UnexpectedToken(TokenKind::Number(n)));
                            }
                            self.binop_precedence.insert(c, num as i8);
                            self.consume_token()?;
                        } else {
                            self.binop_precedence.insert(c, 30);
                        }
                        let mut s = "binary".to_string();
                        s.push(c);
                        s
                    }
                    t => return Err(ParserErrorKind::UnexpectedToken(t)),
                }
            }
            TokenKind::Keyword(Unary) => {
                self.consume_token()?;
                match self.peek_token()? {
                    TokenKind::Op(c) if c.is_ascii() => {
                        self.consume_token()?;
                        kind = 1;
                        let mut s = "unary".to_string();
                        s.push(c);
                        s
                    }
                    t => return Err(ParserErrorKind::UnexpectedToken(t)),
                }
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
            let arg = match self.peek_token()? {
                TokenKind::Identifier(i) => {
                    self.consume_token()?;
                    i
                }
                TokenKind::Op(')') => break,
                t => return Err(ParserErrorKind::UnexpectedToken(t)),
            };

            let typ = match self.peek_token()? {
                TokenKind::Op(':') => {
                    self.consume_token()?;
                    self.parse_type()?
                }
                t => return Err(ParserErrorKind::UnexpectedToken(t)),
            };
            args.push(Arg::new(arg.clone(), typ));
        }

        self.consume_token()?;
        if kind > 0 && args.len() != kind as usize {
            return Err(ParserErrorKind::InvalidSignatuere);
        }
        match self.opt_peek_token()? {
            Some(TokenKind::Op('-')) => {
                self.consume_token()?;
                let typ = match self.peek_token()? {
                    TokenKind::Op('>') => {
                        self.consume_token()?;

                        self.parse_type()?
                    }
                    t => return Err(ParserErrorKind::UnexpectedToken(t)),
                };
                Ok(Prototype::new(id, args, Some(typ)))
            }
            _ => Ok(Prototype::new(id, args, Some(Box::new(TypeKind::Unit)))),
        }
    }

    fn parse_extern(&mut self) -> Result<DeclKind, ParserErrorKind> {
        self.consume_token()?;
        Ok(DeclKind::Extern(self.parse_prototype()?))
    }

    fn parse_def(&mut self) -> Result<DeclKind, ParserErrorKind> {
        self.consume_token()?;
        let proto = self.parse_prototype()?;
        let e = self.parse_block()?;
        Ok(DeclKind::Function(proto, e))
    }

    fn parse_top_level_expr(&mut self) -> Result<DeclKind, ParserErrorKind> {
        let e = self.parse_expr()?;
        let proto = Prototype::new("__anon_expr".to_string(), Vec::new(), None);
        Ok(DeclKind::Function(proto, e))
    }

    fn parse_primary(&mut self) -> Result<Expr, ParserErrorKind> {
        let expr = match self.peek_token()? {
            TokenKind::Identifier(_) => self.parse_identifier(),
            TokenKind::Number(_) => self.parse_number(),
            TokenKind::Op('(') => self.parse_paren(),
            TokenKind::Op('[') => self.parse_square_brackets(),
            TokenKind::Keyword(If) => self.parse_if(),
            TokenKind::Keyword(For) => self.parse_for(),
            TokenKind::Keyword(Var) => self.parse_var(),
            t => Err(ParserErrorKind::UnexpectedToken(t)),
        }?;

        self.parse_post_primary(expr)
    }

    fn parse_expr(&mut self) -> Result<Expr, ParserErrorKind> {
        let lhs = self.parse_unary()?;
        self.parse_bin_op_rhs(0, lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParserErrorKind> {
        if !matches!(self.peek_token()?, TokenKind::Op(c) if c.is_ascii() && c != '(' && c != ',' && c != '[')
        {
            return self.parse_primary();
        }

        let op = self.consume_token()?;
        let expr = self.parse_unary()?;
        Ok(Box::new(ExprKind::Unary(op.into_op().into(), expr)))
    }

    fn parse_bin_op_rhs(&mut self, expr_prec: i8, mut lhs: Expr) -> Result<Expr, ParserErrorKind> {
        loop {
            let tok_prec = self.get_token_precedence()?;
            if tok_prec < expr_prec {
                return Ok(lhs);
            }

            let bin_op = self.consume_token()?;
            let mut rhs = self.parse_unary()?;
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

        let body = self.parse_block()?;

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
        let then = self.parse_block()?;

        match self.peek_token()? {
            TokenKind::Keyword(Else) => {
                self.consume_token()?;
            }
            t => return Err(ParserErrorKind::UnexpectedToken(t)),
        }
        let elsee = self.parse_block()?;

        Ok(Box::new(ExprKind::IfThenElse(cond, then, elsee)))
    }

    fn parse_paren(&mut self) -> Result<Expr, ParserErrorKind> {
        self.consume_token()?;
        if let TokenKind::Op(')') = self.peek_token()? {
            self.consume_token()?;
            return Ok(Box::new(Literal::Unit.into()));
        }
        let v = self.parse_expr()?;
        match self.peek_token()? {
            TokenKind::Op(',') => {
                let v = self.parse_cons(v, ConsKind::Tuple)?;
                match self.peek_token()? {
                    TokenKind::Op(')') => {
                        self.consume_token()?;
                        Ok(v)
                    }
                    t => Err(ParserErrorKind::UnexpectedToken(t)),
                }
            }
            TokenKind::Op(')') => {
                self.consume_token()?;
                Ok(v)
            }
            t => Err(ParserErrorKind::UnexpectedToken(t)),
        }
    }

    fn parse_number(&mut self) -> Result<Expr, ParserErrorKind> {
        let number = self.consume_token()?;
        let literal = match number {
            TokenKind::Number(Typ::Double(n)) => Literal::F64(n.parse().unwrap()),
            TokenKind::Number(Typ::Int(n)) => Literal::I64(n),
            _ => unimplemented!(),
        };
        Ok(Box::new(literal.into()))
    }

    fn parse_identifier(&mut self) -> Result<Expr, ParserErrorKind> {
        let identifier = self.consume_token()?.into_identifier();
        let token = self.opt_peek_token()?;
        if token.is_none() || !matches!(token, Some(TokenKind::Op('('))) {
            let expr_var = ExprKind::Identifier(identifier, None);
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

    fn parse_var(&mut self) -> Result<Expr, ParserErrorKind> {
        self.consume_token()?;
        let mut vars: Vec<VarInfo> = Vec::new();
        let mut token = self.peek_token()?;

        if !matches!(token, TokenKind::Identifier(_)) {
            return Err(ParserErrorKind::UnexpectedToken(token));
        }

        loop {
            let name = self.consume_token()?.into_identifier();

            let typ = match self.peek_token()? {
                TokenKind::Op(':') => {
                    self.consume_token()?;
                    Some(self.parse_type()?)
                }
                _ => None,
            };

            let init = match self.peek_token()? {
                TokenKind::Op('=') => {
                    self.consume_token()?;
                    Some(self.parse_expr()?)
                }
                _ => None,
            };
            vars.push(VarInfo {
                name,
                t: typ,
                val: init,
            });
            if !matches!(self.peek_token()?, TokenKind::Op(',')) {
                break;
            }
            self.consume_token()?;

            token = self.peek_token()?;

            if !matches!(token, TokenKind::Identifier(_)) {
                return Err(ParserErrorKind::UnexpectedToken(token));
            }
        }
        token = self.peek_token()?;
        if !matches!(token, TokenKind::Keyword(In)) {
            return Err(ParserErrorKind::UnexpectedToken(token));
        }
        self.consume_token()?;
        let body = self.parse_block()?;
        Ok(Box::new(ExprKind::Var(vars, body)))
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

    fn parse_type(&mut self) -> Result<Type, ParserErrorKind> {
        match self.peek_token()? {
            TokenKind::Keyword(Double) => {
                self.consume_token()?;
                Ok(Box::new(TypeKind::F64))
            }
            TokenKind::Keyword(Int) => {
                self.consume_token()?;
                Ok(Box::new(TypeKind::I64))
            }
            TokenKind::Op('(') => {
                self.consume_token()?;
                match self.peek_token()? {
                    TokenKind::Op(')') => {
                        self.consume_token()?;
                        Ok(Box::new(TypeKind::Unit))
                    }
                    _ => self.parse_tuple_type(),
                }
            }
            TokenKind::Op('[') => {
                self.consume_token()?;
                let typ = self.parse_type()?;
                match self.peek_token()? {
                    TokenKind::Op(']') => {
                        self.consume_token()?;
                        Ok(Box::new(TypeKind::List(typ)))
                    }
                    t => Err(ParserErrorKind::UnexpectedToken(t)),
                }
            }
            t => Err(ParserErrorKind::UnexpectedToken(t)),
        }
    }

    fn parse_block(&mut self) -> Result<Expr, ParserErrorKind> {
        let mut exprs: Vec<Expr> = Vec::new();
        while self.opt_peek_token()?.is_some() {
            exprs.push(self.parse_expr()?);
            if let Some(TokenKind::Op(';')) = self.opt_peek_token()? {
                self.consume_token()?;
            } else {
                break;
            }
        }
        Ok(Box::new(ExprKind::Seq(exprs)))
    }

    fn parse_cons(&mut self, e: Expr, kind: ConsKind) -> Result<Expr, ParserErrorKind> {
        let mut exprs = vec![e];
        loop {
            if !matches!(self.peek_token()?, TokenKind::Op(',')) {
                return Ok(Box::new(ExprKind::Cons(exprs, kind)));
            }
            self.consume_token()?;
            exprs.push(self.parse_expr()?);
        }
    }

    fn parse_tuple_type(&mut self) -> Result<Type, ParserErrorKind> {
        let mut types = Vec::new();
        loop {
            types.push(self.parse_type()?);
            if !matches!(self.peek_token()?, TokenKind::Op(',')) {
                break;
            }
            self.consume_token()?;
        }
        match self.peek_token()? {
            TokenKind::Op(')') => {
                self.consume_token()?;
                Ok(Box::new(TypeKind::Tuple(types)))
            }
            t => Err(ParserErrorKind::UnexpectedToken(t)),
        }
    }

    fn parse_post_primary(&mut self, e: Expr) -> Result<Box<ExprKind>, ParserErrorKind> {
        match self.opt_peek_token()? {
            Some(TokenKind::Op('.')) => {
                self.consume_token()?;
                self.parse_field_number(&e)
            }
            Some(TokenKind::Op('[')) => {
                self.consume_token()?;
                let idx = self.parse_expr()?;
                match self.peek_token()? {
                    TokenKind::Op(']') => {
                        self.consume_token()?;
                        let e = Box::new(ExprKind::Projection(e, idx, None));
                        self.parse_post_primary(e)
                    }
                    t => Err(ParserErrorKind::UnexpectedToken(t)),
                }
            }
            _ => Ok(e),
        }
    }

    fn parse_field_number(&mut self, expr: &Expr) -> Result<Expr, ParserErrorKind> {
        let number = self.consume_token()?;
        match number {
            TokenKind::Number(Typ::Double(n)) => {
                let mut numbers = n.split('.');
                let fst_int = numbers.next().unwrap().parse().unwrap();
                let snd_int = numbers.next().unwrap().parse().unwrap();
                let fst_proj = Box::new(ExprKind::Projection(
                    expr.clone(),
                    Box::new(ExprKind::Literal(Literal::I64(fst_int))),
                    None,
                ));
                let snd_proj = Box::new(ExprKind::Projection(
                    fst_proj,
                    Box::new(ExprKind::Literal(Literal::I64(snd_int))),
                    None,
                ));
                self.parse_post_primary(snd_proj)
            }
            TokenKind::Number(Typ::Int(n)) => {
                let proj = Box::new(ExprKind::Projection(
                    expr.clone(),
                    Box::new(ExprKind::Literal(Literal::I64(n))),
                    None,
                ));
                self.parse_post_primary(proj)
            }
            _ => unimplemented!(),
        }
    }
    fn parse_square_brackets(&mut self) -> Result<Expr, ParserErrorKind> {
        self.consume_token()?;
        if let TokenKind::Op(']') = self.peek_token()? {
            self.consume_token()?;
            return Ok(Box::new(ExprKind::Cons(Vec::new(), ConsKind::List)));
        }

        let v = self.parse_expr()?;
        let v = self.parse_cons(v, ConsKind::List)?;

        match self.peek_token()? {
            TokenKind::Op(']') => {
                self.consume_token()?;
                Ok(v)
            }
            t => Err(ParserErrorKind::UnexpectedToken(t)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ParserErrorKind {
    LexerError(LexerError),
    UnexpectedToken(TokenKind),
    UnexpectedEof,
    InvalidSignatuere,
}

impl<T> From<LexerError> for Result<T, ParserErrorKind> {
    fn from(e: LexerError) -> Self {
        Err(ParserErrorKind::LexerError(e))
    }
}
