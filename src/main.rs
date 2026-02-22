use std::env;
use std::fs;
use kaleidoscope_compiler::lexer::lexer::Lexer;
use kaleidoscope_compiler::grammar::ProgramParser;
use kaleidoscope_compiler::CompilerError;

fn compile()  -> Result<(), CompilerError>{
    let args: Vec<String> = env::args().collect();
    let file = &args[1];
    let contents = fs::read_to_string(file)?;
    let lexer = Lexer::new(&contents);
    let parser = ProgramParser::new();
    let ast = parser.parse(lexer)?;
    println!("{:?}", ast);
    Ok(())
}

fn main() {
    if let Err(error) = compile() {
        eprintln!("{error}");
    }
}
