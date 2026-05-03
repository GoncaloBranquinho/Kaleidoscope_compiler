use std::collections::HashMap;
use std::ffi::CString;
use std::mem::{forget, replace, transmute};

use inkwell::attributes::Attribute;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::types::{AnyType, BasicType, BasicTypeEnum};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue, PointerValue,
};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate, OptimizationLevel};
use llvm_sys::error::LLVMErrorRef;
use llvm_sys::orc2::lljit::{
    LLVMOrcCreateLLJIT, LLVMOrcCreateLLJITBuilder, LLVMOrcLLJITAddLLVMIRModuleWithRT,
    LLVMOrcLLJITBuilderRef, LLVMOrcLLJITGetMainJITDylib, LLVMOrcLLJITLookup, LLVMOrcLLJITRef,
};
use llvm_sys::orc2::{
    LLVMOrcCreateDynamicLibrarySearchGeneratorForPath, LLVMOrcCreateNewThreadSafeModule,
    LLVMOrcExecutorAddress, LLVMOrcJITDylibAddGenerator, LLVMOrcJITDylibCreateResourceTracker,
    LLVMOrcJITDylibGetDefaultResourceTracker, LLVMOrcResourceTrackerRef,
    LLVMOrcResourceTrackerRemove, LLVMOrcThreadSafeContextRef, LLVMOrcThreadSafeModuleRef,
};

use crate::error::CompilerError;
use crate::parser::op::BinaryOp;
use crate::parser::{DeclKind, Expr, ExprKind, Literal, Prototype, Type, TypeKind, UnaryOp};

fn get_left_value_ptr<'ctx>(
    expr: &Expr,
    context: &mut CodeGenBuilder<'ctx>,
) -> Result<PointerValue<'ctx>, CompilerError> {
    match expr.as_ref() {
        ExprKind::Identifier(name, _) => {
            if let Some(ptr) = context.map.get(name) {
                Ok(*ptr)
            } else {
                Err(CompilerError::Llvm(
                    "Left-hand side of assignment must be a variable or a tuple field".to_string(),
                ))
            }
        }
        ExprKind::Projection(t1, t2) => {
            let ptr = get_left_value_ptr(t1, context)?;
            let idx = match t2.as_ref() {
                ExprKind::Literal(Literal::I64(n)) => n,
                _ => unreachable!(),
            };
            let struct_type = t1.codegen(context)?.get_type().into_struct_type();
            Ok(context
                .builder
                .build_struct_gep(struct_type, ptr, *idx as u32, "gep_on_tuple")?)
        }
        _ => {
            unreachable!()
        }
    }
}

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
        TypeKind::List(_) => context
            .ctx
            .ptr_type(AddressSpace::default())
            .as_basic_type_enum(),
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
    let alloca = builder.build_alloca(t, var_name)?;
    if t.is_pointer_type() {
        let function_name = fn_value.get_name().to_string_lossy().to_string();
        let (size, roots) = context
            .function_prologue_info
            .get_mut(&function_name)
            .unwrap();
        *size += 1;
        roots.push(alloca);
        context.builder.build_store(
            alloca,
            context.ctx.ptr_type(AddressSpace::default()).const_null(),
        )?;
        let fn_value = gc_root_function(context);
        let ptr_value = context.ctx.ptr_type(AddressSpace::default()).const_null();
        builder.build_call(fn_value, &[alloca.into(), ptr_value.into()], "")?;
    }
    Ok(alloca)
}

fn gc_new_function<'ctx>(context: &mut CodeGenBuilder<'ctx>) -> FunctionValue<'ctx> {
    if let Some(gc_new) = context.module.get_function("gc_new") {
        gc_new
    } else {
        let return_type = context.ctx.ptr_type(AddressSpace::default());
        let fn_type = return_type.fn_type(&[context.ctx.i32_type().into()], false);
        context.module.add_function("gc_new", fn_type, None)
    }
}

fn gc_root_function<'ctx>(context: &mut CodeGenBuilder<'ctx>) -> FunctionValue<'ctx> {
    if let Some(gc_root) = context.module.get_function("llvm.gcroot") {
        gc_root
    } else {
        let ptr_type = context.ctx.ptr_type(AddressSpace::default());
        let return_type = context.ctx.void_type();
        let fn_type = return_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
        context.module.add_function("llvm.gcroot", fn_type, None)
    }
}

fn generate_epilogue<'ctx>(context: &mut CodeGenBuilder<'ctx>) -> Result<(), CompilerError> {
    let fn_value = if let Some(gc_pop) = context.module.get_function("gc_pop") {
        gc_pop
    } else {
        let return_type = context.ctx.void_type();
        let fn_type = return_type.fn_type(&[], false);
        context.module.add_function("gc_pop", fn_type, None)
    };
    context
        .builder
        .build_call(fn_value, &[], "curr_gc_root_chain")?;

    Ok(())
}

fn generate_prologue<'ctx>(context: &mut CodeGenBuilder<'ctx>) -> Result<(), CompilerError> {
    let i32_type = context.ctx.i32_type();
    let ptr_type = context.ctx.ptr_type(AddressSpace::default());
    let frame_map_type = context.ctx.struct_type(&[i32_type.into()], false);
    let block = context
        .builder
        .get_insert_block()
        .ok_or_else(|| CompilerError::Llvm("No insert point".to_string()))?;
    let function = block
        .get_parent()
        .ok_or_else(|| CompilerError::Llvm("Basic Block has no parent function".to_string()))?;

    let function_name = function.get_name().to_string_lossy().to_string();

    let num_roots = context
        .function_prologue_info
        .get(&function_name)
        .unwrap()
        .0;

    let frame_map_val = frame_map_type.const_named_struct(&[context
        .ctx
        .i32_type()
        .const_int(num_roots as u64, false)
        .into()]);

    let frame_map_ptr =
        create_entry_block_alloca(context, function, "frame_map_ptr", frame_map_type.into())?;

    context.builder.build_store(frame_map_ptr, frame_map_val)?;

    let roots_vec_type = ptr_type.array_type(num_roots as u32);

    let stack_entry_type = context.ctx.struct_type(
        &[ptr_type.into(), ptr_type.into(), roots_vec_type.into()],
        false,
    );

    let stack_entry_ptr =
        create_entry_block_alloca(context, function, "gc_stack_entry", stack_entry_type.into())?;

    let map_ptr = context.builder.build_struct_gep(
        stack_entry_type,
        stack_entry_ptr,
        1,
        "stack_entry_map",
    )?;
    context.builder.build_store(map_ptr, frame_map_ptr)?;

    if num_roots > 0 {
        let roots = &context
            .function_prologue_info
            .get(&function_name)
            .unwrap()
            .1;

        for (i, root) in roots.iter().enumerate() {
            let ith_element = unsafe {
                context.builder.build_in_bounds_gep(
                    stack_entry_type,
                    stack_entry_ptr,
                    &[
                        context.ctx.i32_type().const_zero(),
                        context.ctx.i32_type().const_int(2, false),
                        context.ctx.i32_type().const_int(i as u64, false),
                    ],
                    &format!("roots_{i}"),
                )?
            };
            context.builder.build_store(ith_element, *root)?;
        }
    }

    let gc_push = if let Some(gc_push) = context.module.get_function("gc_push") {
        gc_push
    } else {
        let return_type = context.ctx.void_type();
        let fn_type = return_type.fn_type(&[ptr_type.into()], false);
        context.module.add_function("gc_push", fn_type, None)
    };
    context
        .builder
        .build_call(gc_push, &[stack_entry_ptr.into()], "")?;
    Ok(())
}

pub struct CodeGenBuilder<'ctx> {
    pub ctx: &'ctx Context,
    pub tsc: &'ctx LLVMOrcThreadSafeContextRef,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub target_machine: TargetMachine,
    pub map: HashMap<String, PointerValue<'ctx>>,
    pub function_protos: HashMap<String, Prototype>,
    pub function_prologue_info: HashMap<String, (i32, Vec<PointerValue<'ctx>>)>,
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
        let function_prologue_info = HashMap::new();

        Ok(CodeGenBuilder {
            ctx,
            tsc,
            module,
            builder,
            target_machine,
            map,
            function_protos,
            function_prologue_info,
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
                    /*let s = if let ExprKind::Identifier(s, _) = left.as_ref() {
                        s
                    } else {
                        return Err(CompilerError::Llvm(
                            "Left-hand side of assignment must be a variable".to_string(),
                        ));
                    };*/
                    let s = get_left_value_ptr(left, context)?;
                    let r = right.codegen(context)?;
                    context.builder.build_store(s, r)?;
                    return Ok(r);
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
                let flag = callee.get_type().get_return_type().is_some();
                let args_len = if flag { args.len() } else { args.len() + 1 };
                if args_len != callee.count_params() as usize {
                    return Err(CompilerError::Llvm(
                        "Incorrect # arguments passed".to_string(),
                    ));
                }
                let mut vec: Vec<BasicMetadataValueEnum> = Vec::new();

                let proto = context.function_protos.get(name).unwrap();
                let return_type = get_type(proto.ret_type.as_ref().unwrap(), context);

                let alloca = if flag {
                    None
                } else {
                    let block = context
                        .builder
                        .get_insert_block()
                        .ok_or_else(|| CompilerError::Llvm("No insert point".to_string()))?;

                    let function = block.get_parent().ok_or_else(|| {
                        CompilerError::Llvm("Basic Block has no parent function".to_string())
                    })?;

                    let alloca =
                        create_entry_block_alloca(context, function, "__sret_var", return_type)?;
                    vec.push(alloca.into());
                    Some(alloca)
                };

                for arg in args.iter() {
                    let arg_eval = arg.codegen(context)?;
                    vec.push(arg_eval.into());
                }

                let call = context.builder.build_call(callee, &vec, "calltmp");

                if let Some(alloca) = alloca {
                    call?;
                    Ok(context
                        .builder
                        .build_load(return_type, alloca, "sret_load")?)
                } else {
                    match call {
                        Ok(v) => Ok(v.try_as_basic_value().basic().unwrap()),
                        Err(e) => Err(e.into()),
                    }
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

                let end_cond = end.codegen(context)?.into_int_value();

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
            ExprKind::Tuple(exprs) => {
                let mut tuple_types = Vec::new();
                let mut tuple_values = Vec::new();
                for expr in exprs.iter() {
                    let expr_value = expr.codegen(context)?;
                    tuple_types.push(expr_value.get_type());
                    tuple_values.push(expr_value);
                }

                let mut struct_value_undef =
                    context.ctx.struct_type(&tuple_types, false).get_undef();

                for (i, tuple_value) in tuple_values.iter().enumerate() {
                    struct_value_undef = context
                        .builder
                        .build_insert_value(struct_value_undef, *tuple_value, i as u32, "tuple")?
                        .into_struct_value();
                }

                Ok(struct_value_undef.as_basic_value_enum())
            }
            ExprKind::Projection(val, idx) => {
                let val = val.codegen(context)?;
                let n = match idx.as_ref() {
                    ExprKind::Literal(Literal::I64(n)) => n,
                    _ => unreachable!(),
                };
                Ok(context.builder.build_extract_value(
                    val.into_struct_value(),
                    *n as u32,
                    "extract",
                )?)
            }
            ExprKind::Pair(car, cdr) => {
                let fn_value = gc_new_function(context);
                let arg = if let Some(car) = car.as_ref() {
                    if let ExprKind::Pair(_, _) = car.as_ref() {
                        context
                            .ctx
                            .i32_type()
                            .const_int(1, false)
                            .as_basic_value_enum()
                    } else {
                        context.ctx.i32_type().const_zero().as_basic_value_enum()
                    }
                } else {
                    context.ctx.bool_type().const_zero().as_basic_value_enum()
                };

                let call = context
                    .builder
                    .build_call(fn_value, &[arg.into()], "car_ptr")?;
                let car_ptr = call
                    .try_as_basic_value()
                    .basic()
                    .ok_or(CompilerError::Llvm(
                        "Expected function to return a value".to_string(),
                    ))?
                    .into_pointer_value();
                let block = context
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompilerError::Llvm("No insert point".to_string()))?;
                let function = block.get_parent().ok_or_else(|| {
                    CompilerError::Llvm("Basic Block has no parent function".to_string())
                })?;

                let aux_alloca = create_entry_block_alloca(
                    context,
                    function,
                    "aux_alloca",
                    context
                        .ctx
                        .ptr_type(AddressSpace::default())
                        .as_basic_type_enum(),
                )?;
                if let Some(car) = car {
                    let car_val = car.codegen(context)?;
                    context.builder.build_store(car_ptr, car_val)?;
                    context.builder.build_store(aux_alloca, car_ptr)?;
                } else {
                    let null_ptr = context.ctx.ptr_type(AddressSpace::default()).const_null();
                    context.builder.build_store(car_ptr, null_ptr)?;
                    context.builder.build_store(aux_alloca, car_ptr)?;
                }
                let cdr_val = cdr.codegen(context)?;
                let cdr_ptr = unsafe {
                    context.builder.build_gep(
                        context.ctx.i8_type(),
                        car_ptr,
                        &[context.ctx.i64_type().const_int(8, false)],
                        "cdr_ptr",
                    )
                }?;
                if let ExprKind::Literal(Literal::Unit) = cdr.as_ref() {
                    context.builder.build_store(
                        cdr_ptr,
                        context.ctx.ptr_type(AddressSpace::default()).const_null(),
                    )?;
                } else {
                    context.builder.build_store(cdr_ptr, cdr_val)?;
                }
                Ok(car_ptr.as_basic_value_enum())
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

        let flag = matches!(
            self.ret_type.as_ref().unwrap().as_ref(),
            &TypeKind::Tuple(_)
        );
        let return_type = get_type(self.ret_type.as_ref().unwrap(), context);
        let fn_type = if flag {
            args_type.insert(0, context.ctx.ptr_type(AddressSpace::default()).into());
            context.ctx.void_type().fn_type(&args_type, false)
        } else {
            return_type.fn_type(&args_type, false)
        };

        if context.module.get_function(&self.name).is_some() {
            return Err(CompilerError::Llvm(
                "Function cannot have multiple declarations".to_string(),
            ));
        }

        let fn_value = context.module.add_function(&self.name, fn_type, None);

        if flag {
            fn_value.add_attribute(
                inkwell::attributes::AttributeLoc::Param(0),
                context.ctx.create_type_attribute(
                    Attribute::get_named_enum_kind_id("sret"),
                    return_type.as_any_type_enum(),
                ),
            );
        }

        let ptr = if flag { 1 } else { 0 };

        for (i, arg) in fn_value.get_param_iter().enumerate() {
            if i == 0 && flag {
                arg.set_name("__sret_var");
            } else {
                arg.set_name(&self.args[i - ptr].name);
            }
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
                if context.module.get_global("llvm_gc_root_chain").is_none() {
                    let ptr_type = context.ctx.ptr_type(AddressSpace::default());
                    let global = context
                        .module
                        .add_global(ptr_type, None, "llvm_gc_root_chain");
                    global.set_linkage(inkwell::module::Linkage::External);
                    global.set_initializer(&ptr_type.const_null()); // maybe delete?
                }
                context
                    .function_protos
                    .insert(proto.name.clone(), proto.clone());

                let fn_value = get_function(&proto.name, context)?;
                fn_value.set_gc("shadow-stack");

                if fn_value.get_first_basic_block().is_some() {
                    return Err(CompilerError::Llvm(
                        "Function cannot be redefnied".to_string(),
                    ));
                }
                context
                    .function_prologue_info
                    .insert(proto.name.clone(), (0, Vec::new()));

                let prologue_block = context.ctx.append_basic_block(fn_value, "prologue");
                let basic_block = context.ctx.append_basic_block(fn_value, "entry");
                let epilogue_block = context.ctx.append_basic_block(fn_value, "epilogue");
                context.builder.position_at_end(basic_block);
                context.map.clear();

                let flag = fn_value.get_type().get_return_type().is_some();
                let args_len = if flag {
                    proto.args.len()
                } else {
                    proto.args.len() + 1
                };
                if args_len != fn_value.count_params() as usize {
                    return Err(CompilerError::Llvm(
                        "Incorrect # arguments passed".to_string(),
                    ));
                }
                for (i, arg) in fn_value.get_param_iter().enumerate() {
                    //arg.set_name(&proto.args[i].name);
                    if flag || i != 0 {
                        let alloca = create_entry_block_alloca(
                            context,
                            fn_value,
                            &arg.get_name().to_string_lossy(),
                            fn_value.get_nth_param(i as u32).unwrap().get_type(),
                        )?;
                        context.builder.build_store(alloca, arg)?;
                        context
                            .map
                            .insert(arg.get_name().to_string_lossy().to_string(), alloca);
                    }
                }

                let body_value = body.codegen(context);
                context.builder.position_at_end(prologue_block);
                generate_prologue(context)?;
                context.builder.build_unconditional_branch(basic_block)?;

                context.builder.position_at_end(basic_block);
                context.builder.build_unconditional_branch(epilogue_block)?;
                context.builder.position_at_end(epilogue_block);
                generate_epilogue(context)?;

                match body_value {
                    Err(error) => {
                        unsafe {
                            fn_value.delete();
                        };
                        return Err(error);
                    }

                    Ok(BasicValueEnum::FloatValue(value)) => {
                        context.builder.build_return(Some(&value))?;
                    }

                    Ok(BasicValueEnum::IntValue(value)) => {
                        context.builder.build_return(Some(&value))?;
                    }
                    Ok(BasicValueEnum::StructValue(value)) => {
                        let arg = fn_value.get_first_param().unwrap().into_pointer_value();
                        context.builder.build_store(arg, value)?;
                        context.builder.build_return(None)?;
                    }
                    Ok(BasicValueEnum::PointerValue(value)) => {
                        context.builder.build_return(Some(&value))?;
                    }

                    _ => {}
                }
                fn_value.verify(true);

                // The following block is commented for debugging purposes

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
            let dylib = LLVMOrcLLJITGetMainJITDylib(lljit);
            let mut generator = std::ptr::null_mut();
            let path_to_lib = CString::new("target/debug/libruntime.so").unwrap();
            LLVMOrcCreateDynamicLibrarySearchGeneratorForPath(
                &mut generator,
                path_to_lib.as_ptr(),
                0,
                None,
                std::ptr::null_mut(),
            );
            LLVMOrcJITDylibAddGenerator(dylib, generator);

            let mut generator = std::ptr::null_mut();
            let path_to_lib = CString::new("target/debug/libgc.so").unwrap();
            LLVMOrcCreateDynamicLibrarySearchGeneratorForPath(
                &mut generator,
                path_to_lib.as_ptr(),
                0,
                None,
                std::ptr::null_mut(),
            );
            LLVMOrcJITDylibAddGenerator(dylib, generator);

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
        context: &CodeGenBuilder<'ctx>,
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
                        self.run(rt, jit, proto.ret_type.as_deref().unwrap(), codegen_builder)
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
        context: &CodeGenBuilder<'ctx>,
    ) -> Result<(), CompilerError> {
        match self {
            DeclKind::Function { .. } => {
                print_result(jit, ret_type, context)?;
                jit.remove(rt)
            }
            _ => Ok(()),
        }
    }
}

fn print_result<'ctx>(
    jit: &mut KaleidoscopeJIT,
    ret_type: &TypeKind,
    context: &CodeGenBuilder<'ctx>,
) -> Result<(), CompilerError> {
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
        TypeKind::Tuple(tuple_types) => {
            let mut tuple_types_basic = Vec::new();
            for t in tuple_types.iter() {
                tuple_types_basic.push(get_type(t, context));
            }
            let struct_type = context.ctx.struct_type(&tuple_types_basic, false);
            let abi_size = context
                .target_machine
                .get_target_data()
                .get_abi_size(&struct_type);
            let mut buf = vec![0u8; abi_size as usize];
            unsafe {
                let function: extern "C" fn(*mut u8) = transmute(executer_addr as usize);
                function(buf.as_mut_ptr());
            }
            print_tuple(&TypeKind::Tuple(tuple_types.clone()), buf, context)?;
            println!();
        }
        TypeKind::List(_) => {
            let res = jit.call::<*const u8>(executer_addr);
            print_pair(res);
            println!();
        }
    }
    Ok(())
}

fn print_tuple<'ctx>(
    t: &TypeKind,
    offsets: Vec<u8>,
    context: &CodeGenBuilder<'ctx>,
) -> Result<(), CompilerError> {
    match t {
        TypeKind::F64 => {
            let value = f64::from_ne_bytes(offsets[..8].try_into().unwrap());
            print!("{}", value);
        }
        TypeKind::I64 => {
            let value = i64::from_ne_bytes(offsets[..8].try_into().unwrap());
            print!("{}", value);
        }
        TypeKind::Unit => {
            print!("()");
        }
        TypeKind::Tuple(tuple_types) => {
            print!("(");
            let mut tuple_types_basic = Vec::new();
            for t in tuple_types.iter() {
                tuple_types_basic.push(get_type(t, context));
            }
            let struct_type = context.ctx.struct_type(&tuple_types_basic, false);
            for (i, tuple_type) in tuple_types.iter().enumerate() {
                if i > 0 {
                    print!(",")
                }
                let target_data = context.target_machine.get_target_data();
                let offset = target_data
                    .offset_of_element(&struct_type, i as u32)
                    .unwrap() as usize;
                print_tuple(tuple_type, offsets[offset..].to_vec(), context)?;
            }
            print!(")");
        }
        TypeKind::List(_) => {
            todo!()
        }
    }
    Ok(())
}

fn print_pair(res: *const u8) {
    print!("[");
    unsafe {
        let mut cell = res;
        let mut flag = false;
        while !cell.is_null() {
            if flag {
                print!(",");
            }
            let is_pointer = *(cell.sub(4) as *const i32) == 1;
            if is_pointer {
                print_pair(*(cell as *const *const u8));
            } else {
                let car = *(cell as *const u64);
                print!("{}", car);
            }
            let cdr = *(cell.add(8) as *const *const u8);
            cell = cdr;
            flag = true;
        }
    }
    print!("]");
}
