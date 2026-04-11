use inkwell::context::Context;
use kaleipl::{
    CodeGen, CodeGenBuilder, CompilerError, JitCompiler, KaleidoscopeJIT, lexer::Lexer,
    parser::Parser,
};
use llvm_sys::orc2::{LLVMOrcCreateNewThreadSafeContext, LLVMOrcThreadSafeContextGetContext};
use owo_colors::OwoColorize;
use std::{
    collections::HashMap,
    io::{self, Write},
};

pub fn compile(
    content: &str,
    codegen_builder: &mut CodeGenBuilder,
    jit: &mut KaleidoscopeJIT,
    binop_precedence: &mut HashMap<char, i8>,
) -> Result<(), CompilerError> {
    let lexer = Lexer::new(content.char_indices().peekable());
    /*let tokens: Result<Vec<kaleipl::lexer::tokens::TokenKind>, kaleipl::lexer::core::LexerError> =
        lexer.into_iter().collect();
    let tokens = tokens.unwrap();
    println!("{tokens:?}");
    return Ok(());*/
    let mut parser = Parser::new(lexer, binop_precedence);
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

    let mut binop_precedence: HashMap<char, i8> = HashMap::new();

    binop_precedence.insert('=', 2);
    binop_precedence.insert('<', 10);
    binop_precedence.insert('+', 20);
    binop_precedence.insert('-', 20);
    binop_precedence.insert('*', 40);

    println!("Welcome to kaleipl.");

    loop {
        print!("{}", ">> ".yellow().bold());
        io::stdout().flush().unwrap();
        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input)?;

        if let Err(error) = compile(
            &input,
            &mut codegen_builder,
            &mut jit,
            &mut binop_precedence,
        ) {
            eprintln!("{error}");
        }
    }
}
