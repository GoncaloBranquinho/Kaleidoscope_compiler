use std::collections::HashMap;
use std::ffi::CString;
use std::io::Write;
use std::mem::{forget, replace, transmute};

use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::types::BasicMetadataTypeEnum;
use inkwell::values::{
    BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue, PointerValue,
};
use inkwell::{FloatPredicate, OptimizationLevel};
use llvm_sys::error::LLVMErrorRef;
use llvm_sys::orc2::lljit::{
    LLVMOrcCreateLLJIT, LLVMOrcCreateLLJITBuilder, LLVMOrcLLJITAddLLVMIRModuleWithRT,
    LLVMOrcLLJITBuilderRef, LLVMOrcLLJITGetMainJITDylib, LLVMOrcLLJITLookup, LLVMOrcLLJITRef,
};
use llvm_sys::orc2::{
    LLVMOrcCreateNewThreadSafeModule, LLVMOrcExecutorAddress, LLVMOrcJITDylibCreateResourceTracker,
    LLVMOrcJITDylibGetDefaultResourceTracker, LLVMOrcResourceTrackerRef,
    LLVMOrcResourceTrackerRemove, LLVMOrcThreadSafeContextRef, LLVMOrcThreadSafeModuleRef,
};

use crate::error::CompilerError;
use crate::parser::op::BinaryOp;
use crate::parser::{DeclKind, Expr, ExprKind, Literal, Prototype, UnaryOp};

fn get_function<'ctx>(
    name: &str,
    context: &mut CodeGenBuilder<'ctx>,
) -> Result<FunctionValue<'ctx>, CompilerError> {
    if let Some(callee) = context.module.get_function(name) {
        return Ok(callee);
    }
    println!("{name}");
    let proto = context.function_protos.get(name);
    if let Some(proto) = proto {
        let proto = proto.clone();
        return proto.codegen(context);
    }

    Err(CompilerError::Llvm(
        "Unknown function referenced".to_string(),
    ))
}

fn create_entry_block_alloca<'ctx>(
    context: &mut CodeGenBuilder<'ctx>,
    fn_value: FunctionValue<'ctx>,
    var_name: &str,
) -> Result<PointerValue<'ctx>, CompilerError> {
    let builder = context.ctx.create_builder();
    let block = fn_value
        .get_first_basic_block()
        .ok_or_else(|| CompilerError::Llvm("No basic block".to_string()))?;
    builder.position_at_end(block);
    if let Some(inst) = block.get_first_instruction() {
        builder.position_before(&inst);
    }
    Ok(builder.build_alloca(context.ctx.f64_type(), var_name)?)
}

pub struct CodeGenBuilder<'ctx> {
    pub ctx: &'ctx Context,
    pub tsc: &'ctx LLVMOrcThreadSafeContextRef,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub target_machine: TargetMachine,
    pub map: HashMap<String, PointerValue<'ctx>>,
    pub function_protos: HashMap<String, Prototype>,
}

pub trait CodeGen<'ctx> {
    type Item;
    fn codegen(&self, context: &mut CodeGenBuilder<'ctx>) -> Result<Self::Item, CompilerError>;
}

impl<'ctx> CodeGenBuilder<'ctx> {
    pub fn new(
        ctx: &'ctx Context,
        tsc: &'ctx LLVMOrcThreadSafeContextRef,
    ) -> Result<CodeGenBuilder<'ctx>, CompilerError> {
        let module: Module = ctx.create_module("main");

        let _ = Target::initialize_native(&InitializationConfig {
            base: true,
            asm_printer: true,
            asm_parser: true,
            machine_code: true,
            info: false,
            disassembler: false,
        });

        let triple = TargetMachine::get_default_triple();

        let target = Target::from_triple(&triple).map_err(CompilerError::from)?;

        let target_machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                OptimizationLevel::None,
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or_else(|| CompilerError::Llvm("Unable to create a target machine".to_string()))?;

        let builder = ctx.create_builder();
        let map = HashMap::new();
        let function_protos = HashMap::new();

        Ok(CodeGenBuilder {
            ctx,
            tsc,
            module,
            builder,
            target_machine,
            map,
            function_protos,
        })
    }
}

impl<'ctx> CodeGen<'ctx> for Expr {
    type Item = BasicValueEnum<'ctx>;
    fn codegen(&self, context: &mut CodeGenBuilder<'ctx>) -> Result<Self::Item, CompilerError> {
        match self.as_ref() {
            ExprKind::Literal(Literal::F64(value)) => {
                let f64_type = context.ctx.f64_type();
                Ok(f64_type.const_float(*value).as_basic_value_enum())
            }

            ExprKind::Identifier(name) => {
                if let Some(value) = context.map.get(name) {
                    Ok(context
                        .builder
                        .build_load(context.ctx.f64_type(), *value, name)?)
                } else {
                    Err(CompilerError::Llvm(format!(
                        "Unknown variable name: {}",
                        name
                    )))
                }
            }

            ExprKind::Var(vars, body) => {
                let mut old_bindings: Vec<Option<PointerValue<'ctx>>> = Vec::new();
                let block = context
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompilerError::Llvm("No insert point".to_string()))?;
                let function = block.get_parent().ok_or_else(|| {
                    CompilerError::Llvm("Basic Block has no parent function".to_string())
                })?;

                for (name, init) in vars.clone() {
                    let init_val = if let Some(init_val) = init {
                        init_val.codegen(context)?
                    } else {
                        context.ctx.f64_type().const_zero().into()
                    };

                    let alloca = create_entry_block_alloca(context, function, &name)?;
                    context.builder.build_store(alloca, init_val)?;
                    old_bindings.push(context.map.insert(name, alloca));
                }
                let body = body.codegen(context)?;
                for i in 0..vars.len() {
                    if let Some(binding) = old_bindings[i] {
                        context.map.insert(vars[i].0.clone(), binding);
                    } else {
                        context.map.remove(&vars[i].0);
                    }
                }
                Ok(body)
            }

            ExprKind::Unary(op, expr) => {
                let l = expr.codegen(context)?;

                match l {
                    BasicValueEnum::FloatValue(_) => {
                        let res = match op {
                            UnaryOp::UserDefined(op) => {
                                let mut fn_name = "unary".to_string();
                                fn_name.push(*op);
                                let fn_value = get_function(&fn_name, context)?;
                                let call =
                                    context.builder.build_call(fn_value, &[l.into()], "unop")?;
                                let value = call.try_as_basic_value().basic().ok_or(
                                    CompilerError::Llvm(
                                        "Expected function to return a value".to_string(),
                                    ),
                                )?;
                                value.into_float_value()
                            }
                        };
                        Ok(res.into())
                    }
                    _ => Err(CompilerError::Llvm(
                        "Expression must be of float type".to_string(),
                    )),
                }
            }

            ExprKind::Binary(op, left, right) => {
                if let BinaryOp::Assign = op {
                    let s = if let ExprKind::Identifier(s) = left.as_ref() {
                        s
                    } else {
                        return Err(CompilerError::Llvm(
                            "Left-hand side of assignment must be a variable".to_string(),
                        ));
                    };
                    let r = right.codegen(context)?;
                    if let Some(var) = context.map.get(s) {
                        context.builder.build_store(*var, r)?;
                        return Ok(r);
                    } else {
                        return Err(CompilerError::Llvm(format!("Unknown variable name: {}", s)));
                    }
                }
                let l = left.codegen(context)?;
                let r = right.codegen(context)?;
                match (l, r) {
                    (BasicValueEnum::FloatValue(value_l), BasicValueEnum::FloatValue(value_r)) => {
                        let res = match op {
                            BinaryOp::Add => context
                                .builder
                                .build_float_add(value_l, value_r, "addtmp")?,
                            BinaryOp::Sub => context
                                .builder
                                .build_float_sub(value_l, value_r, "subtmp")?,
                            BinaryOp::Mult => context
                                .builder
                                .build_float_mul(value_l, value_r, "multmp")?,
                            BinaryOp::Lt => {
                                let f64_type = context.ctx.f64_type();
                                let cmp = context.builder.build_float_compare(
                                    FloatPredicate::ULT,
                                    value_l,
                                    value_r,
                                    "cmptmp",
                                )?;
                                context
                                    .builder
                                    .build_unsigned_int_to_float(cmp, f64_type, "booltmp")?
                            }
                            BinaryOp::UserDefined(op) => {
                                let mut fn_name = "binary".to_string();
                                fn_name.push(*op);
                                let fn_value = get_function(&fn_name, context)?;
                                let call = context.builder.build_call(
                                    fn_value,
                                    &[l.into(), r.into()],
                                    "binop",
                                )?;
                                let value = call.try_as_basic_value().basic().ok_or(
                                    CompilerError::Llvm(
                                        "Expected function to return a value".to_string(),
                                    ),
                                )?;
                                value.into_float_value()
                            }
                            BinaryOp::Assign => unreachable!(
                                "Temporary: this code is unreachable until semantic analysis is implemented. It will be removed afterwards"
                            ),
                        };
                        Ok(res.into())
                    }
                    _ => Err(CompilerError::Llvm(
                        "Expression must be of float type".to_string(),
                    )),
                }
            }

            ExprKind::Call(name, args) => {
                let callee = get_function(name, context)?;
                if args.len() != callee.count_params() as usize {
                    return Err(CompilerError::Llvm(
                        "Incorrect # arguments passed".to_string(),
                    ));
                }
                let mut vec: Vec<BasicMetadataValueEnum> = Vec::new();
                let tmp = 0..args.len();
                for i in tmp {
                    let arg_eval = args[i].codegen(context)?;
                    vec.push(arg_eval.into());
                }

                let call = context.builder.build_call(callee, &vec, "calltmp");
                match call {
                    Ok(v) => {
                        let value = v.try_as_basic_value().basic().ok_or(CompilerError::Llvm(
                            "Expected function to return a value".to_string(),
                        ))?;
                        Ok(value.into_float_value().into())
                    }
                    Err(e) => Err(e.into()),
                }
            }

            ExprKind::IfThenElse(cond, fst, snd) => {
                let cond_v = if let BasicValueEnum::FloatValue(cond_v) = cond.codegen(context)? {
                    cond_v
                } else {
                    return Err(CompilerError::Llvm(
                        "Expected condition to be of type float".to_string(),
                    ));
                };

                let cond_v = context.builder.build_float_compare(
                    FloatPredicate::ONE,
                    cond_v,
                    context.ctx.f64_type().const_zero(),
                    "ifcond",
                )?;
                let block = context
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompilerError::Llvm("No insert point".to_string()))?;
                let function = block.get_parent().ok_or_else(|| {
                    CompilerError::Llvm("Basic Block has no parent function".to_string())
                })?;

                let then_bb = context.ctx.append_basic_block(function, "then");
                let else_bb = context.ctx.append_basic_block(function, "else");
                let merge_bb = context.ctx.append_basic_block(function, "ifcont");

                context
                    .builder
                    .build_conditional_branch(cond_v, then_bb, else_bb)?;

                // Codegen then_basic_block
                context.builder.position_at_end(then_bb);
                let then_v = fst.codegen(context)?;
                context.builder.build_unconditional_branch(merge_bb)?;
                let new_then_bb = context
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompilerError::Llvm("No insert point".to_string()))?;

                // Codegen else_basic_block
                context.builder.position_at_end(else_bb);
                let else_v = snd.codegen(context)?;
                context.builder.build_unconditional_branch(merge_bb)?;
                let new_else_bb = context
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompilerError::Llvm("No insert point".to_string()))?;

                // Codegen merge_basic_block
                context.builder.position_at_end(merge_bb);
                let phi_node = context.builder.build_phi(context.ctx.f64_type(), "iftmp")?;

                let then_v: &dyn BasicValue = &then_v;
                let else_v: &dyn BasicValue = &else_v;

                let incoming: Vec<(&dyn BasicValue, BasicBlock)> =
                    vec![(then_v, new_then_bb), (else_v, new_else_bb)];

                phi_node.add_incoming(&incoming);
                Ok(phi_node.as_basic_value())
            }
            ExprKind::ForLoop(var_name, start, end, step, body) => {
                let block = context
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompilerError::Llvm("No insert point".to_string()))?;

                let function = block.get_parent().ok_or_else(|| {
                    CompilerError::Llvm("Basic Block has no parent function".to_string())
                })?;

                let alloca = create_entry_block_alloca(context, function, var_name)?;

                let start_v = start.codegen(context)?;

                context.builder.build_store(alloca, start_v)?;

                let loop_bb = context.ctx.append_basic_block(function, "loop");

                context.builder.build_unconditional_branch(loop_bb)?;
                context.builder.position_at_end(loop_bb);

                let old_v = context.map.insert(var_name.clone(), alloca);

                body.codegen(context)?;

                let step_v = if let Some(s) = step {
                    s.codegen(context)?
                } else {
                    let f64_type = context.ctx.f64_type();
                    f64_type.const_float(1.0).as_basic_value_enum()
                };

                let step_v_as_f = if let BasicValueEnum::FloatValue(step_v_as_f) = step_v {
                    step_v_as_f
                } else {
                    return Err(CompilerError::Llvm(
                        "Step value must be of type float".to_string(),
                    ));
                };

                let end_cond = if let BasicValueEnum::FloatValue(end_cond) = end.codegen(context)? {
                    end_cond
                } else {
                    return Err(CompilerError::Llvm(
                        "Expected condition to be of type float".to_string(),
                    ));
                };

                let cur_var =
                    context
                        .builder
                        .build_load(context.ctx.f64_type(), alloca, var_name)?;

                let cur_v_as_f = if let BasicValueEnum::FloatValue(cur_v_as_f) = cur_var {
                    cur_v_as_f
                } else {
                    return Err(CompilerError::Llvm(
                        "Pointer value must be of type float".to_string(),
                    ));
                };

                let next_v = context
                    .builder
                    .build_float_add(cur_v_as_f, step_v_as_f, "nextvar")?;

                context.builder.build_store(alloca, next_v)?;

                let end_cond = context.builder.build_float_compare(
                    FloatPredicate::ONE,
                    end_cond,
                    context.ctx.f64_type().const_zero(),
                    "loopcond",
                )?;

                let after_bb = context.ctx.append_basic_block(function, "afterloop");

                context
                    .builder
                    .build_conditional_branch(end_cond, loop_bb, after_bb)?;
                context.builder.position_at_end(after_bb);

                if let Some(val) = old_v {
                    context.map.insert(var_name.clone(), val);
                } else {
                    context.map.remove(var_name);
                }

                Ok(context.ctx.f64_type().const_zero().as_basic_value_enum())
            }
        }
    }
}

impl<'ctx> CodeGen<'ctx> for Prototype {
    type Item = FunctionValue<'ctx>;
    fn codegen(&self, context: &mut CodeGenBuilder<'ctx>) -> Result<Self::Item, CompilerError> {
        let f64_type = context.ctx.f64_type();
        let args_type: Vec<BasicMetadataTypeEnum> = vec![f64_type.into(); self.args.len()];
        let fn_type = f64_type.fn_type(&args_type, false);

        if context.module.get_function(&self.name).is_some() {
            return Err(CompilerError::Llvm(
                "Function cannot have multiple declarations".to_string(),
            ));
        }

        let fn_value = context.module.add_function(&self.name, fn_type, None);

        for (i, arg) in fn_value.get_param_iter().enumerate() {
            arg.set_name(&self.args[i].name);
        }

        Ok(fn_value)
    }
}

impl<'ctx> CodeGen<'ctx> for DeclKind {
    type Item = FunctionValue<'ctx>;
    fn codegen(&self, context: &mut CodeGenBuilder<'ctx>) -> Result<Self::Item, CompilerError> {
        match self {
            DeclKind::Extern(proto) => proto.codegen(context),

            DeclKind::Function(proto, body) => {
                context
                    .function_protos
                    .insert(proto.name.clone(), proto.clone());

                let fn_value = get_function(&proto.name, context)?;
                if fn_value.get_first_basic_block().is_some() {
                    return Err(CompilerError::Llvm(
                        "Function cannot be redefnied".to_string(),
                    ));
                }

                let basic_block = context.ctx.append_basic_block(fn_value, "entry");

                context.builder.position_at_end(basic_block);
                context.map.clear();

                if proto.args.len() != fn_value.count_params() as usize {
                    return Err(CompilerError::Llvm(
                        "Incorrect # arguments passed".to_string(),
                    ));
                }

                for (i, arg) in fn_value.get_param_iter().enumerate() {
                    arg.set_name(&proto.args[i].name);
                    let alloca = create_entry_block_alloca(context, fn_value, &proto.name)?;
                    context.builder.build_store(alloca, arg)?;
                    context
                        .map
                        .insert(arg.get_name().to_string_lossy().to_string(), alloca);
                }

                let body_value = body.codegen(context);

                match body_value {
                    Err(error) => {
                        unsafe {
                            fn_value.delete();
                        };
                        return Err(error);
                    }

                    Ok(BasicValueEnum::FloatValue(value)) => {
                        let _ = context.builder.build_return(Some(&value));
                        fn_value.verify(true);
                    }
                    _ => {
                        fn_value.verify(true);
                    }
                }

                // optimizing the newly created function

                let options = PassBuilderOptions::create();

                if let Err(e) = context.module.run_passes(
                    "function(mem2reg,instcombine,reassociate,gvn,simplifycfg)",
                    &context.target_machine,
                    options,
                ) {
                    return Err(CompilerError::Llvm(e.to_string_lossy().to_string()));
                }

                Ok(fn_value)
            }
        }
    }
}

pub struct KaleidoscopeJIT {
    pub lljit: LLVMOrcLLJITRef,
}

impl KaleidoscopeJIT {
    pub fn new() -> Result<KaleidoscopeJIT, LLVMErrorRef> {
        unsafe {
            let mut lljit: LLVMOrcLLJITRef = std::ptr::null_mut();
            let builder: LLVMOrcLLJITBuilderRef = LLVMOrcCreateLLJITBuilder();
            let err = LLVMOrcCreateLLJIT(&mut lljit, builder);
            if !err.is_null() {
                return Err(err);
            }

            Ok(KaleidoscopeJIT { lljit })
        }
    }

    fn add_module(
        &self,
        tsm: LLVMOrcThreadSafeModuleRef,
        rt: LLVMOrcResourceTrackerRef,
    ) -> Result<(), CompilerError> {
        unsafe {
            let err = LLVMOrcLLJITAddLLVMIRModuleWithRT(self.lljit, rt, tsm);
            if !err.is_null() {
                Err(err.into())
            } else {
                Ok(())
            }
        }
    }

    fn lookup(&self, name: &str) -> Result<LLVMOrcExecutorAddress, CompilerError> {
        unsafe {
            let mut result: LLVMOrcExecutorAddress = 0;

            let cname = CString::new(name).map_err(|_| {
                CompilerError::Llvm("Function name contains a null byte".to_string())
            })?;

            let err = LLVMOrcLLJITLookup(self.lljit, &mut result, cname.as_ptr());
            if !err.is_null() {
                return Err(err.into());
            }
            Ok(result)
        }
    }

    fn call(&self, addr: LLVMOrcExecutorAddress) -> f64 {
        unsafe {
            let function: extern "C" fn() -> f64 = transmute(addr as usize);
            function()
        }
    }

    fn remove(&self, rt: LLVMOrcResourceTrackerRef) -> Result<(), CompilerError> {
        unsafe {
            let err = LLVMOrcResourceTrackerRemove(rt);
            if !err.is_null() {
                Err(err.into())
            } else {
                Ok(())
            }
        }
    }
}

pub trait JitCompiler<'ctx> {
    fn compile(
        &self,
        codegen_builder: &mut CodeGenBuilder<'ctx>,
        jit: &mut KaleidoscopeJIT,
    ) -> Result<(), CompilerError>;

    fn run(
        &self,
        rt: LLVMOrcResourceTrackerRef,
        jit: &mut KaleidoscopeJIT,
    ) -> Result<(), CompilerError>;
}

impl<'ctx> JitCompiler<'ctx> for DeclKind {
    fn compile(
        &self,
        codegen_builder: &mut CodeGenBuilder<'ctx>,
        jit: &mut KaleidoscopeJIT,
    ) -> Result<(), CompilerError> {
        match self {
            DeclKind::Extern(proto) => {
                codegen_builder
                    .function_protos
                    .insert(proto.name.clone(), proto.clone());
                Ok(())
            }

            DeclKind::Function(proto, _) => {
                if proto.name == "__anon_expr" {
                    unsafe {
                        let dylib = LLVMOrcLLJITGetMainJITDylib(jit.lljit);
                        let rt = LLVMOrcJITDylibCreateResourceTracker(dylib);
                        let ptr = codegen_builder.module.as_mut_ptr();

                        let old_module = replace(
                            &mut codegen_builder.module,
                            codegen_builder.ctx.create_module("main"),
                        );

                        forget(old_module);

                        codegen_builder.builder = codegen_builder.ctx.create_builder();

                        let tsm = LLVMOrcCreateNewThreadSafeModule(ptr, *codegen_builder.tsc);
                        jit.add_module(tsm, rt)?;
                        self.run(rt, jit)
                    }
                } else {
                    unsafe {
                        let dylib = LLVMOrcLLJITGetMainJITDylib(jit.lljit);
                        let rt = LLVMOrcJITDylibGetDefaultResourceTracker(dylib);
                        let ptr = codegen_builder.module.as_mut_ptr();

                        let old_module = replace(
                            &mut codegen_builder.module,
                            codegen_builder.ctx.create_module("main"),
                        );

                        forget(old_module);

                        codegen_builder.builder = codegen_builder.ctx.create_builder();

                        let tsm = LLVMOrcCreateNewThreadSafeModule(ptr, *codegen_builder.tsc);
                        jit.add_module(tsm, rt)?;
                        Ok(())
                    }
                }
            }
        }
    }

    fn run(
        &self,
        rt: LLVMOrcResourceTrackerRef,
        jit: &mut KaleidoscopeJIT,
    ) -> Result<(), CompilerError> {
        match self {
            DeclKind::Function { .. } => {
                let executer_addr = jit.lookup("__anon_expr")?;
                let res = jit.call(executer_addr);
                println!("Evaluated to {res}");
                jit.remove(rt)
            }
            _ => Ok(()),
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn putchard(x: f64) -> f64 {
    print!("{}", x as u8 as char);
    std::io::stdout().flush().unwrap();
    0.0
}

#[unsafe(no_mangle)]
pub extern "C" fn printd(x: f64) -> f64 {
    println!("{}", x);
    0.0
}
