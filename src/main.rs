use inkwell::context::Context;
use kaleipl::{
    CodeGen, CodeGenBuilder, CompilerError, JitCompiler, KaleidoscopeJIT, lexer::Lexer,
    parser::Parser,
};
use llvm_sys::orc2::{LLVMOrcCreateNewThreadSafeContext, LLVMOrcThreadSafeContextGetContext};
use owo_colors::OwoColorize;
use std::io::{self, Write};

pub fn compile(
    content: &str,
    codegen_builder: &mut CodeGenBuilder,
    jit: &mut KaleidoscopeJIT,
) -> Result<(), CompilerError> {
    let lexer = Lexer::new(content.char_indices().peekable());
    /*let tokens: Result<Vec<TokenKind>, _> = lexer.into_iter().collect();
    let tokens = tokens.unwrap();
    println!("{tokens:?}");
    return Ok(());*/
    let mut parser = Parser::new(lexer);
    let ast = match parser.parse_program() {
        Ok(a) => a,
        Err(errors) => {
            for e in errors {
                println!("{e}");
            }
            return Ok(());
        }
    };

    println!("{:?}", ast);

    for decl in ast.iter() {
        let fn_value = decl.codegen(codegen_builder)?;
        fn_value.print_to_stderr();
        decl.compile(codegen_builder, jit)?;
    }

    Ok(())
}

fn main() -> Result<(), CompilerError> {
    let (tsc, context) = unsafe {
        let tsc = LLVMOrcCreateNewThreadSafeContext();
        (tsc, Context::new(LLVMOrcThreadSafeContextGetContext(tsc)))
    };
    let mut codegen_builder = CodeGenBuilder::new(&context, &tsc)?;

    let mut jit = KaleidoscopeJIT::new().unwrap();

    println!("Welcome to kaleipl. For help, type :help");

    loop {
        print!("{}", ">> ".yellow().bold());
        io::stdout().flush().unwrap();
        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input)?;
        if let Err(error) = compile(&input, &mut codegen_builder, &mut jit) {
            eprintln!("{error}");
        }
    }
}

//separar jit e codegenbuilder. Para poder resetar
