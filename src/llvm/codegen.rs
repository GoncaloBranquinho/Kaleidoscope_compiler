use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::execution_engine::{ExecutionEngine, JitFunction};
use inkwell::module::Module;
use inkwell::OptimizationLevel;

use std::error::Error;

struct CodeGenBuilder<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    execution_engine: ExecutionEngine<'ctx>,
}

trait CodeGen {
    fn codegen(&self, ctx: &CodeGenBuilder) -> String;
}

impl<'ctx> CodeGenBuilder<'ctx> {
    pub fn new(context: &'ctx Context) -> Result<CodeGenBuilder<'ctx>, Box<dyn std::error::Error>> {
        let module = context.create_module("main");
        let builder = context.create_builder();
        let execution_engine = module.create_jit_execution_engine(OptimizationLevel::None)?;

        Ok(CodeGenBuilder {
            context,
            module,
            builder,
            execution_engine
        })
    }
}

