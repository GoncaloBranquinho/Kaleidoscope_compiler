use std::str::FromStr;

#[derive(Copy, Debug, PartialEq, Clone)]
pub enum Symbol {}

impl FromStr for Symbol {
    type Err = String;

    fn from_str(s: &str) -> Result<Symbol, String> {
        match s {
            s => Err(format!("Invalid Token: {}", s)),
        }
    }
}
