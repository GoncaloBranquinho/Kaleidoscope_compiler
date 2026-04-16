use std::collections::HashMap;
use std::ffi::CString;
use std::io::Write;
use std::mem::{forget, replace, transmute};

use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::types::{BasicType, BasicTypeEnum};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue, PointerValue,
};
use inkwell::{FloatPredicate, IntPredicate, OptimizationLevel};
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
use crate::parser::{DeclKind, Expr, ExprKind, Literal, Prototype, Type, TypeKind, UnaryOp};

fn get_type<'ctx>(t: &Type, context: &CodeGenBuilder<'ctx>) -> BasicTypeEnum<'ctx> {
    match t.as_ref() {
        TypeKind::F64 => context.ctx.f64_type().as_basic_type_enum(),
        TypeKind::I64 => context.ctx.i64_type().as_basic_type_enum(),
        TypeKind::Unit => context.ctx.bool_type().as_basic_type_enum(),
        TypeKind::Tuple(types) => {
            let mut tuple_types = Vec::new();
            for tuple_type in types.iter() {
                tuple_types.push(get_type(tuple_type, context));
            }
            context
                .ctx
                .struct_type(&tuple_types, false)
                .as_basic_type_enum()
        }
    }
}

// Prototypes must declare the type of each argument and as well the return type
fn get_function<'ctx>(
    name: &str,
    context: &mut CodeGenBuilder<'ctx>,
) -> Result<FunctionValue<'ctx>, CompilerError> {
    if let Some(callee) = context.module.get_function(name) {
        return Ok(callee);
    }
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
    t: BasicTypeEnum<'ctx>,
) -> Result<PointerValue<'ctx>, CompilerError> {
    let builder = context.ctx.create_builder();
    let block = fn_value
        .get_first_basic_block()
        .ok_or_else(|| CompilerError::Llvm("No basic block".to_string()))?;
    builder.position_at_end(block);
    if let Some(inst) = block.get_first_instruction() {
        builder.position_before(&inst);
    }
    Ok(builder.build_alloca(t, var_name)?)
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

        Target::initialize_all(&InitializationConfig {
            base: true,
            asm_printer: true,
            asm_parser: true,
            machine_code: true,
            info: true,
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

        let target_data = target_machine.get_target_data();
        let data_layout = target_data.get_data_layout();

        module.set_data_layout(&data_layout);
        module.set_triple(&triple);

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
            ExprKind::Literal(Literal::I64(value)) => {
                let i64_type = context.ctx.i64_type();
                Ok(i64_type
                    .const_int(*value as u64, true)
                    .as_basic_value_enum())
            }
            ExprKind::Literal(Literal::Unit) => {
                Ok(context.ctx.bool_type().const_zero().as_basic_value_enum())
            }
            ExprKind::Identifier(name, typ) => {
                if let Some(value) = context.map.get(name) {
                    let type_ctx = get_type(typ.as_ref().unwrap(), context);
                    Ok(context.builder.build_load(type_ctx, *value, name)?)
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

                for var in vars.clone() {
                    if var.t.is_none() {
                        old_bindings.push(None);
                        continue;
                    }
                    let init_val = if let Some(init_val) = var.val {
                        init_val.codegen(context)?
                    } else {
                        get_type(var.t.as_ref().unwrap(), context).const_zero()
                    };

                    let alloca = create_entry_block_alloca(
                        context,
                        function,
                        &var.name,
                        init_val.get_type(),
                    )?;
                    context.builder.build_store(alloca, init_val)?;
                    old_bindings.push(context.map.insert(var.name, alloca));
                }
                let body = body.codegen(context)?;
                for i in 0..vars.len() {
                    if let Some(binding) = old_bindings[i] {
                        context.map.insert(vars[i].name.clone(), binding);
                    } else {
                        context.map.remove(&vars[i].name);
                    }
                }
                Ok(body)
            }
            ExprKind::Unary(op, expr) => {
                let l = expr.codegen(context)?;

                match l {
                    BasicValueEnum::FloatValue(_) | BasicValueEnum::IntValue(_) => {
                        let res = match op {
                            UnaryOp::UserDefined(op) => {
                                let mut fn_name = "unary".to_string();
                                fn_name.push(*op);
                                let fn_value = get_function(&fn_name, context)?;
                                let call =
                                    context.builder.build_call(fn_value, &[l.into()], "unop")?;
                                call.try_as_basic_value()
                                    .basic()
                                    .ok_or(CompilerError::Llvm(
                                        "Expected function to return a value".to_string(),
                                    ))?
                            }
                        };
                        Ok(res)
                    }
                    _ => Err(CompilerError::Llvm(
                        "Expression must be of float type".to_string(),
                    )),
                }
            }
            ExprKind::Binary(op, left, right) => {
                if let BinaryOp::Assign = op {
                    let s = if let ExprKind::Identifier(s, _) = left.as_ref() {
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
                                .build_float_add(value_l, value_r, "addtmp")?
                                .as_basic_value_enum(),
                            BinaryOp::Sub => context
                                .builder
                                .build_float_sub(value_l, value_r, "subtmp")?
                                .as_basic_value_enum(),
                            BinaryOp::Mult => context
                                .builder
                                .build_float_mul(value_l, value_r, "multmp")?
                                .as_basic_value_enum(),
                            BinaryOp::Lt => {
                                let cmp = context.builder.build_float_compare(
                                    FloatPredicate::ULT,
                                    value_l,
                                    value_r,
                                    "cmptmp",
                                )?;
                                context
                                    .builder
                                    .build_int_z_extend(cmp, context.ctx.i64_type(), "booltmp")?
                                    .as_basic_value_enum()
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
                                value.into_float_value().as_basic_value_enum()
                            }
                            BinaryOp::Assign => unreachable!(
                                "Temporary: this code is unreachable until semantic analysis is implemented. It will be removed afterwards"
                            ),
                        };
                        Ok(res)
                    }
                    (BasicValueEnum::IntValue(value_l), BasicValueEnum::IntValue(value_r)) => {
                        let res = match op {
                            BinaryOp::Add => {
                                context.builder.build_int_add(value_l, value_r, "addtmp")?
                            }
                            BinaryOp::Sub => {
                                context.builder.build_int_sub(value_l, value_r, "subtmp")?
                            }
                            BinaryOp::Mult => {
                                context.builder.build_int_mul(value_l, value_r, "multmp")?
                            }
                            BinaryOp::Lt => {
                                let cmp = context.builder.build_int_compare(
                                    IntPredicate::SLT,
                                    value_l,
                                    value_r,
                                    "cmptmp",
                                )?;

                                context.builder.build_int_z_extend(
                                    cmp,
                                    context.ctx.i64_type(),
                                    "booltmp",
                                )?
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
                                value.into_int_value()
                            }
                            BinaryOp::Assign => unreachable!(
                                "Temporary: this code is unreachable until semantic analysis is implemented. It will be removed afterwards"
                            ),
                        };
                        Ok(res.into())
                    }
                    _ => Err(CompilerError::Llvm(
                        "Expression must have compatible types".to_string(),
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
                        Ok(value)
                    }
                    Err(e) => Err(e.into()),
                }
            }
            ExprKind::IfThenElse(cond, fst, snd) => {
                let cond_v = cond.codegen(context)?;

                let cond_v = match cond_v {
                    BasicValueEnum::IntValue(cond_v) => context.builder.build_int_compare(
                        IntPredicate::NE,
                        cond_v,
                        cond_v.get_type().const_zero(),
                        "ifcond",
                    )?,
                    BasicValueEnum::FloatValue(cond_v) => context.builder.build_float_compare(
                        FloatPredicate::ONE,
                        cond_v,
                        cond_v.get_type().const_zero(),
                        "ifcond",
                    )?,
                    _ => unimplemented!(),
                };
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
                let phi_node = context.builder.build_phi(then_v.get_type(), "iftmp")?;

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

                let start_v = start.codegen(context)?;

                let alloca =
                    create_entry_block_alloca(context, function, var_name, start_v.get_type())?;

                let old_v = context.map.insert(var_name.clone(), alloca);

                context.builder.build_store(alloca, start_v)?;

                let loop_cond_bb = context.ctx.append_basic_block(function, "loop_cond");

                context.builder.build_unconditional_branch(loop_cond_bb)?;
                context.builder.position_at_end(loop_cond_bb);

                let end_cond = if let BasicValueEnum::IntValue(end_cond) = end.codegen(context)? {
                    end_cond
                } else {
                    return Err(CompilerError::Llvm(
                        "Expected condition to be of type Int".to_string(),
                    ));
                };

                let loop_bb = context.ctx.append_basic_block(function, "loop_bb");

                let after_bb = context.ctx.append_basic_block(function, "afterloop");

                context
                    .builder
                    .build_conditional_branch(end_cond, loop_bb, after_bb)?;

                context.builder.position_at_end(loop_bb);

                body.codegen(context)?;

                let step_v = if let Some(s) = step {
                    s.codegen(context)?
                } else {
                    match start_v.get_type() {
                        BasicTypeEnum::IntType(t) => t.const_int(1, false).as_basic_value_enum(),
                        BasicTypeEnum::FloatType(t) => t.const_float(1.0).as_basic_value_enum(),
                        _ => unimplemented!(),
                    }
                };

                let cur_var = match start_v.get_type() {
                    BasicTypeEnum::IntType(t) => context.builder.build_load(t, alloca, var_name)?,
                    BasicTypeEnum::FloatType(t) => {
                        context.builder.build_load(t, alloca, var_name)?
                    }
                    _ => unimplemented!(),
                };

                let next_v = match (step_v, cur_var) {
                    (BasicValueEnum::IntValue(a), BasicValueEnum::IntValue(b)) => context
                        .builder
                        .build_int_add(a, b, "nextvar")?
                        .as_basic_value_enum(),
                    (BasicValueEnum::FloatValue(a), BasicValueEnum::FloatValue(b)) => context
                        .builder
                        .build_float_add(a, b, "nextvar")?
                        .as_basic_value_enum(),
                    _ => unimplemented!(),
                };

                context.builder.build_store(alloca, next_v)?;

                context.builder.build_unconditional_branch(loop_cond_bb)?;

                context.builder.position_at_end(after_bb);

                if let Some(val) = old_v {
                    context.map.insert(var_name.clone(), val);
                } else {
                    context.map.remove(var_name);
                }

                Ok(context.ctx.bool_type().const_zero().as_basic_value_enum())
            }
            ExprKind::Seq(exprs) => {
                let size = exprs.len();
                for (i, expr) in exprs.iter().enumerate() {
                    let expr_res = expr.codegen(context)?;
                    if i == size - 1 {
                        return Ok(expr_res);
                    }
                }
                Ok(context.ctx.bool_type().const_zero().as_basic_value_enum())
            }
            ExprKind::Tuple(_expr_kinds) => {
                todo!()
            }
        }
    }
}

impl<'ctx> CodeGen<'ctx> for Prototype {
    type Item = FunctionValue<'ctx>;
    fn codegen(&self, context: &mut CodeGenBuilder<'ctx>) -> Result<Self::Item, CompilerError> {
        let mut args_type = Vec::new();
        for arg in self.args.iter() {
            let arg_type = get_type(&arg.typ, context);
            args_type.push(arg_type.into());
        }
        let fn_type = get_type(self.ret_type.as_ref().unwrap(), context).fn_type(&args_type, false);
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
                    let alloca = create_entry_block_alloca(
                        context,
                        fn_value,
                        &proto.args[i].name,
                        fn_value.get_nth_param(i as u32).unwrap().get_type(),
                    )?;
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

                    Ok(BasicValueEnum::IntValue(value)) => {
                        let _ = context.builder.build_return(Some(&value));
                        fn_value.verify(true);
                    }

                    _ => {
                        fn_value.verify(true);
                    }
                }

                // optimizing the newly created function

                /*let options = PassBuilderOptions::create();

                if let Err(e) = fn_value.run_passes(
                    "mem2reg,instcombine,reassociate,gvn,simplifycfg",
                    &context.target_machine,
                    options,
                ) {
                    return Err(CompilerError::Llvm(e.to_string_lossy().to_string()));
                }*/

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

    fn call<T>(&self, addr: LLVMOrcExecutorAddress) -> T {
        unsafe {
            let function: extern "C" fn() -> T = transmute(addr as usize);
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
        ret_type: &TypeKind,
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
                        self.run(rt, jit, proto.ret_type.as_deref().unwrap())
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
        ret_type: &TypeKind,
    ) -> Result<(), CompilerError> {
        match self {
            DeclKind::Function { .. } => {
                let executer_addr = jit.lookup("__anon_expr")?;
                match ret_type {
                    TypeKind::F64 => {
                        let res = jit.call::<f64>(executer_addr);
                        println!("{res}");
                    }
                    TypeKind::I64 => {
                        let res = jit.call::<i64>(executer_addr);
                        println!("{res}");
                    }
                    TypeKind::Unit => {
                        jit.call::<bool>(executer_addr);
                        println!("()");
                    }
                    TypeKind::Tuple(_) => {
                        // let res = jit.call::<_>(executer_addr);
                        // println!("{res}");
                    }
                }
                jit.remove(rt)
            }
            _ => Ok(()),
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn putchard(x: f64) {
    print!("{}", x as u8 as char);
    std::io::stdout().flush().unwrap();
}

#[unsafe(no_mangle)]
pub extern "C" fn printd(x: f64) {
    println!("{}", x);
}
