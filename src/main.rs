use inkwell::context::Context;
use kaleidoscope_compiler::{CodeGen, CodeGenBuilder, CompilerError, Lexer, ProgramParser};
use std::io::{self, Write};

fn compile(content: &str, codegen_builder: &mut CodeGenBuilder) -> Result<(), CompilerError> {
    let lexer = Lexer::new(content);
    let parser = ProgramParser::new();
    let ast = parser.parse(lexer)?;
    println!("{:?}", ast);

    for decl in ast.iter() {
        decl.codegen(codegen_builder)?;
    }

    println!("{}", codegen_builder.module.to_string());

    // Delete the anonymous function created to evaluate the top-level expression,
    // so future top-level expressions don't trigger a "Function cannot be redefined" error.
    if let Some(f) = codegen_builder.module.get_function("__anon_expr") {
        unsafe {
            f.delete();
        }
    }
    Ok(())
}

fn main() -> Result<(), CompilerError> {
    let context = Context::create();
    let mut codegen_builder = CodeGenBuilder::new(&context)?;

    loop {
        print!("ready> ");
        io::stdout().flush().unwrap();
        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input)?;
        if let Err(error) = compile(&input, &mut codegen_builder) {
            eprintln!("{error}");
        }
    }
}
