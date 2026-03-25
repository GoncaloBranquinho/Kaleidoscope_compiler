use inkwell::context::Context;
use kaleipl::{
    CodeGen, CodeGenBuilder, CompilerError, JitCompiler, KaleidoscopeJIT, Lexer, LlvmValue,
    ProgramParser,
};
use llvm_sys::orc2::{LLVMOrcCreateNewThreadSafeContext, LLVMOrcThreadSafeContextGetContext};
use owo_colors::OwoColorize;
use std::io::{self, Write};

pub fn compile(
    content: &str,
    codegen_builder: &mut CodeGenBuilder,
    jit: &mut KaleidoscopeJIT,
) -> Result<(), CompilerError> {
    let lexer = Lexer::new(content);
    let parser = ProgramParser::new();
    let ast = parser.parse(lexer)?;
    println!("{:?}", ast);

    for decl in ast.iter() {
        let decl_ir = decl.codegen(codegen_builder)?;
        if let LlvmValue::Function(fn_value) = decl_ir {
            fn_value.print_to_stderr();
        }
        decl.compile(codegen_builder, jit)?;
    }

    /*
    // Delete the anonymous function created to evaluate the top-level expression,
    // so future top-level expressions don't trigger a "Function cannot be redefined" error.
    if let Some(f) = codegen_builder.module.get_function("__anon_expr") {
        unsafe {
            f.delete();
        }
    }
    */
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
