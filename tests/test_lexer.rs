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
fn test_invalid() {
    let input = "";
    let mut lexer = Lexer::new(input.char_indices().peekable());
    let tokens: Vec<TokenResult> = vec![];
    assert_tokens(&mut lexer, &tokens);
}
