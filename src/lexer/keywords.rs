use std::str::FromStr;

#[derive(Copy, Debug, PartialEq, Clone)]
pub enum Keyword {
    Def,
    Extern,
    If,
    Then,
    Else,
    For,
    In,
    Unary,
    Binary,
}

impl FromStr for Keyword {
    type Err = ();

    fn from_str(s: &str) -> Result<Keyword, ()> {
        match s {
            "def" => Ok(Keyword::Def),
            "extern" => Ok(Keyword::Extern),
            "if" => Ok(Keyword::If),
            "then" => Ok(Keyword::Then),
            "else" => Ok(Keyword::Else),
            "for" => Ok(Keyword::For),
            "in" => Ok(Keyword::In),
            "binary" => Ok(Keyword::Binary),
            "unary" => Ok(Keyword::Unary),
            _ => Err(()),
        }
    }
}
