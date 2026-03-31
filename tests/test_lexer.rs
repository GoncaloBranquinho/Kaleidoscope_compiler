use kaleipl::lexer::{Lexer, core::TokenResult, keywords::Keyword::*, tokens::TokenKind::*};

fn assert_tokens(lexer: &mut Lexer, tokens: &[TokenResult]) {
    for (i, token) in lexer.enumerate() {
        assert_eq!(token, tokens[i]);
    }
}

#[test]
fn test_valid_tokens() {
    let input = "1 1.0 1. if then else for in def extern olaaaa x XSSAS l s + - **? ~";
    let mut lexer = Lexer::new(input.char_indices().peekable());
    let tokens: Vec<TokenResult> = vec![
        Ok(Number("1".to_string())),
        Ok(Number("1.0".to_string())),
        Ok(Number("1.".to_string())),
        Ok(Keyword(If)),
        Ok(Keyword(Then)),
        Ok(Keyword(Else)),
        Ok(Keyword(For)),
        Ok(Keyword(In)),
        Ok(Keyword(Def)),
        Ok(Keyword(Extern)),
        Ok(Identifier("olaaaa".to_string())),
        Ok(Identifier("x".to_string())),
        Ok(Identifier("XSSAS".to_string())),
        Ok(Identifier("l".to_string())),
        Ok(Identifier("s".to_string())),
        Ok(Op('+')),
        Ok(Op('-')),
        Ok(Op('*')),
        Ok(Op('*')),
        Ok(Op('?')),
        Ok(Op('~')),
    ];
    assert_tokens(&mut lexer, &tokens);
}

#[test]
fn test_for() {
    let input = "def mandelhelp(xmin xmax xstep ymin ymax ystep) 
                    for y = ymin, y < ymax, ystep in (
                        ( 
                          for x = xmin, x < xmax, xstep in 
                          printdensity(mandelconverge(x,y))
                        ) : putchard(10)
                    );
";
    let mut lexer = Lexer::new(input.char_indices().peekable());
    let tokens: Vec<TokenResult> = vec![
        Ok(Keyword(Def)),
        Ok(Identifier("mandelhelp".to_string())),
        Ok(Op('(')),
        Ok(Identifier("xmin".to_string())),
        Ok(Identifier("xmax".to_string())),
        Ok(Identifier("xstep".to_string())),
        Ok(Identifier("ymin".to_string())),
        Ok(Identifier("ymax".to_string())),
        Ok(Identifier("ystep".to_string())),
        Ok(Op(')')),
        Ok(Keyword(For)),
        Ok(Identifier("y".to_string())),
        Ok(Op('=')),
        Ok(Identifier("ymin".to_string())),
        Ok(Op(',')),
        Ok(Identifier("y".to_string())),
        Ok(Op('<')),
        Ok(Identifier("ymax".to_string())),
        Ok(Op(',')),
        Ok(Identifier("ystep".to_string())),
        Ok(Keyword(In)),
        Ok(Op('(')),
        Ok(Op('(')),
        Ok(Keyword(For)),
        Ok(Identifier("x".to_string())),
        Ok(Op('=')),
        Ok(Identifier("xmin".to_string())),
        Ok(Op(',')),
        Ok(Identifier("x".to_string())),
        Ok(Op('<')),
        Ok(Identifier("xmax".to_string())),
        Ok(Op(',')),
        Ok(Identifier("xstep".to_string())),
        Ok(Keyword(In)),
        Ok(Identifier("printdensity".to_string())),
        Ok(Op('(')),
        Ok(Identifier("mandelconverge".to_string())),
        Ok(Op('(')),
        Ok(Identifier("x".to_string())),
        Ok(Op(',')),
        Ok(Identifier("y".to_string())),
        Ok(Op(')')),
        Ok(Op(')')),
        Ok(Op(')')),
        Ok(Op(':')),
        Ok(Identifier("putchard".to_string())),
        Ok(Op('(')),
        Ok(Number("10".to_string())),
        Ok(Op(')')),
        Ok(Op(')')),
        Ok(Op(';')),
    ];
    assert_tokens(&mut lexer, &tokens);
}

#[test]
fn test_if() {
    let input = "def mandelconverger(real imag iters creal cimag) 
                    if iters > 255 | (real*real + imag*imag > 4) then 
                        iters 
                    else 
                        mandelconverger(real*real - imag*imag + creal, 2*real*imag + cimag, iters+1, creal, cimag);
";
    let mut lexer = Lexer::new(input.char_indices().peekable());
    let tokens: Vec<TokenResult> = vec![
        Ok(Keyword(Def)),
        Ok(Identifier("mandelconverger".to_string())),
        Ok(Op('(')),
        Ok(Identifier("real".to_string())),
        Ok(Identifier("imag".to_string())),
        Ok(Identifier("iters".to_string())),
        Ok(Identifier("creal".to_string())),
        Ok(Identifier("cimag".to_string())),
        Ok(Op(')')),
        Ok(Keyword(If)),
        Ok(Identifier("iters".to_string())),
        Ok(Op('>')),
        Ok(Number("255".to_string())),
        Ok(Op('|')),
        Ok(Op('(')),
        Ok(Identifier("real".to_string())),
        Ok(Op('*')),
        Ok(Identifier("real".to_string())),
        Ok(Op('+')),
        Ok(Identifier("imag".to_string())),
        Ok(Op('*')),
        Ok(Identifier("imag".to_string())),
        Ok(Op('>')),
        Ok(Number("4".to_string())),
        Ok(Op(')')),
        Ok(Keyword(Then)),
        Ok(Identifier("iters".to_string())),
        Ok(Keyword(Else)),
        Ok(Identifier("mandelconverger".to_string())),
        Ok(Op('(')),
        Ok(Identifier("real".to_string())),
        Ok(Op('*')),
        Ok(Identifier("real".to_string())),
        Ok(Op('-')),
        Ok(Identifier("imag".to_string())),
        Ok(Op('*')),
        Ok(Identifier("imag".to_string())),
        Ok(Op('+')),
        Ok(Identifier("creal".to_string())),
        Ok(Op(',')),
        Ok(Number("2".to_string())),
        Ok(Op('*')),
        Ok(Identifier("real".to_string())),
        Ok(Op('*')),
        Ok(Identifier("imag".to_string())),
        Ok(Op('+')),
        Ok(Identifier("cimag".to_string())),
        Ok(Op(',')),
        Ok(Identifier("iters".to_string())),
        Ok(Op('+')),
        Ok(Number("1".to_string())),
        Ok(Op(',')),
        Ok(Identifier("creal".to_string())),
        Ok(Op(',')),
        Ok(Identifier("cimag".to_string())),
        Ok(Op(')')),
        Ok(Op(';')),
    ];
    assert_tokens(&mut lexer, &tokens);
}
