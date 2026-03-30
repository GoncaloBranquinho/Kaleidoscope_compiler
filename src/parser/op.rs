#[derive(Clone, Debug, PartialEq)]
pub enum BinaryOp {
    Add,
    Lt,
    Mult,
    Sub,
    UserDefined(char),
}

impl BinaryOp {
    pub fn as_str(&self) -> String {
        match self {
            BinaryOp::Add => "+".to_string(),
            BinaryOp::Lt => "<".to_string(),
            BinaryOp::Mult => "*".to_string(),
            BinaryOp::Sub => "-".to_string(),
            BinaryOp::UserDefined(char) => char.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum UnaryOp {
    UserDefined(char),
}

impl UnaryOp {
    pub fn as_str(&self) -> String {
        match self {
            UnaryOp::UserDefined(char) => char.to_string(),
        }
    }
}

impl From<char> for BinaryOp {
    fn from(tok: char) -> Self {
        match tok {
            '+' => BinaryOp::Add,
            '-' => BinaryOp::Sub,
            '*' => BinaryOp::Mult,
            '<' => BinaryOp::Lt,
            c => BinaryOp::UserDefined(c),
        }
    }
}

impl From<char> for UnaryOp {
    fn from(tok: char) -> Self {
        UnaryOp::UserDefined(tok)
    }
}
