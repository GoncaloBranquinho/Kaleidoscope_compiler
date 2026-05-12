use std::collections::HashMap;
use std::ffi::CString;
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
use crate::parser::expr::ConsKind;
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
        ExprKind::Projection(t1, idx, _) => {
            let ptr = get_left_value_ptr(t1, context)?;

            let n = idx.codegen(context)?;

            let load_from_alloca = context.builder.build_load(
                context.ctx.ptr_type(AddressSpace::default()),
                ptr,
                "load_from_alloca",
            )?;

            Ok(gc_proj_function(&load_from_alloca, n, context)?)
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
        /*TypeKind::Tuple(types) => {
            let mut tuple_types = Vec::new();
            for tuple_type in types.iter() {
                tuple_types.push(get_type(tuple_type, context));
            }
            context
                .ctx
                .struct_type(&tuple_types, false)
                .as_basic_type_enum()
        }*/
        TypeKind::List(_) | TypeKind::Tuple(_) | TypeKind::Nil => context
            .ctx
            .ptr_type(AddressSpace::default())
            .as_basic_type_enum(),
    }
}

fn type_to_int<'ctx>(t: &BasicTypeEnum<'ctx>) -> u64 {
    match t {
        BasicTypeEnum::FloatType(_) => 2,
        BasicTypeEnum::IntType(_) => 3,
        _ => unreachable!(),
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

    match name {
        "empty" => Ok(
            if let Some(proj) = context.module.get_function("gc_empty") {
                proj
            } else {
                let ptr_type = context.ctx.ptr_type(AddressSpace::default());
                let i64_type = context.ctx.i64_type().as_basic_type_enum();
                let return_type = i64_type;
                let fn_type = return_type.fn_type(&[ptr_type.into()], false);
                context.module.add_function("gc_empty", fn_type, None)
            },
        ),
        "car" | "cdr" => Ok(
            if let Some(proj) = context.module.get_function(&format!("gc_{}", name)) {
                proj
            } else {
                let ptr_type = context.ctx.ptr_type(AddressSpace::default());
                let return_type = ptr_type;
                let fn_type = return_type.fn_type(&[ptr_type.into()], false);
                context
                    .module
                    .add_function(&format!("gc_{}", name), fn_type, None)
            },
        ),
        _ => Err(CompilerError::Llvm(
            "Unknown function referenced".to_string(),
        )),
    }
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
        let fn_type = return_type.fn_type(
            &[context.ctx.i16_type().into(), context.ctx.i32_type().into()],
            false,
        );
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

fn gc_proj_function<'ctx>(
    l: &BasicValueEnum<'ctx>,
    val: BasicValueEnum<'ctx>,
    context: &mut CodeGenBuilder<'ctx>,
) -> Result<PointerValue<'ctx>, CompilerError> {
    let fn_value = if let Some(proj) = context.module.get_function("gc_proj") {
        proj
    } else {
        let ptr_type = context.ctx.ptr_type(AddressSpace::default());
        let return_type = ptr_type;
        let fn_type = return_type.fn_type(&[ptr_type.into(), context.ctx.i64_type().into()], false);
        context.module.add_function("gc_proj", fn_type, None)
    };

    let call = context
        .builder
        .build_call(fn_value, &[(*l).into(), val.into()], "proj_res")?;
    Ok(call
        .try_as_basic_value()
        .basic()
        .ok_or(CompilerError::Llvm(
            "Expected function to return a value".to_string(),
        ))?
        .into_pointer_value())
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

fn codegen_pair<'ctx>(
    context: &mut CodeGenBuilder<'ctx>,
    expr: &Expr,
    is_first_element: bool,
) -> Result<BasicValueEnum<'ctx>, CompilerError> {
    let exprs = match expr.as_ref() {
        ExprKind::Cons(exprs, _) => exprs,
        _ => unreachable!(),
    };
    if exprs.is_empty() {
        return Ok(context
            .ctx
            .ptr_type(AddressSpace::default())
            .const_null()
            .as_basic_value_enum());
    }
    let (prev_car, arg_kind) = match exprs[0].as_ref() {
        ExprKind::Cons(_, kind) => (codegen_pair(context, &exprs[0], true)?, Some(kind)),
        _ => (exprs[0].codegen(context)?, None),
    };
    let fn_value = gc_new_function(context);
    let arg = if prev_car.is_pointer_value() {
        context
            .ctx
            .i16_type()
            .const_int(1, false)
            .as_basic_value_enum()
    } else {
        context.ctx.i16_type().const_zero().as_basic_value_enum()
    };
    let arg1 = if let Some(kind) = arg_kind {
        match kind {
            ConsKind::List => 0,
            ConsKind::Tuple => 1,
        }
    } else {
        match exprs[0].as_ref() {
            ExprKind::Literal(Literal::Unit) => 4,
            _ => type_to_int(&prev_car.get_type()),
        }
    };

    let call = context.builder.build_call(
        fn_value,
        &[
            arg.into(),
            context.ctx.i32_type().const_int(arg1, false).into(),
        ],
        "car_ptr",
    )?;
    let prev_car_ptr = call
        .try_as_basic_value()
        .basic()
        .ok_or(CompilerError::Llvm(
            "Expected function to return a value".to_string(),
        ))?
        .into_pointer_value();

    context.builder.build_store(prev_car_ptr, prev_car)?;

    if is_first_element {
        let block = context
            .builder
            .get_insert_block()
            .ok_or_else(|| CompilerError::Llvm("No insert point".to_string()))?;
        let function = block
            .get_parent()
            .ok_or_else(|| CompilerError::Llvm("Basic Block has no parent function".to_string()))?;

        let aux_alloca = create_entry_block_alloca(
            context,
            function,
            "aux_alloca",
            context
                .ctx
                .ptr_type(AddressSpace::default())
                .as_basic_type_enum(),
        )?;

        context.builder.build_store(aux_alloca, prev_car_ptr)?;
    }

    let mut prev_cdr = unsafe {
        context.builder.build_gep(
            context.ctx.i8_type(),
            prev_car_ptr,
            &[context.ctx.i64_type().const_int(8, false)],
            "cdr_ptr",
        )
    }?;

    let size = exprs.len();
    let mut i = 1;
    while i < size {
        let (curr_car, arg_kind) = match exprs[i].as_ref() {
            ExprKind::Cons(_, kind) => (codegen_pair(context, &exprs[i], true)?, Some(kind)),
            _ => (exprs[i].codegen(context)?, None),
        };
        let arg = if curr_car.is_pointer_value() {
            context
                .ctx
                .i16_type()
                .const_int(1, false)
                .as_basic_value_enum()
        } else {
            context.ctx.i16_type().const_zero().as_basic_value_enum()
        };
        let arg1 = if let Some(kind) = arg_kind {
            match kind {
                ConsKind::List => 0,
                ConsKind::Tuple => 1,
            }
        } else {
            match exprs[i].as_ref() {
                ExprKind::Literal(Literal::Unit) => 4,
                _ => type_to_int(&curr_car.get_type()),
            }
        };

        let call = context.builder.build_call(
            fn_value,
            &[
                arg.into(),
                context.ctx.i32_type().const_int(arg1, false).into(),
            ],
            "car_ptr",
        )?;
        let curr_car_ptr = call
            .try_as_basic_value()
            .basic()
            .ok_or(CompilerError::Llvm(
                "Expected function to return a value".to_string(),
            ))?
            .into_pointer_value();

        context.builder.build_store(curr_car_ptr, curr_car)?;
        context.builder.build_store(prev_cdr, curr_car_ptr)?;

        prev_cdr = unsafe {
            context.builder.build_gep(
                context.ctx.i8_type(),
                curr_car_ptr,
                &[context.ctx.i64_type().const_int(8, false)],
                "cdr_ptr",
            )
        }?;
        i += 1;
    }
    context.builder.build_store(
        prev_cdr,
        context.ctx.ptr_type(AddressSpace::default()).const_null(),
    )?;
    Ok(prev_car_ptr.as_basic_value_enum())
}

fn codegen_cons<'ctx>(
    exprs: &[Expr],
    ret_type: Option<&Type>,
    context: &mut CodeGenBuilder<'ctx>,
    is_first_element: bool,
) -> Result<BasicValueEnum<'ctx>, CompilerError> {
    let expr1 = exprs[0].codegen(context)?;
    println!("{:?}\n {:?}", exprs[0], expr1);
    let fn_value = gc_new_function(context);
    let arg = if expr1.is_pointer_value() {
        context
            .ctx
            .i16_type()
            .const_int(1, false)
            .as_basic_value_enum()
    } else {
        context.ctx.i16_type().const_zero().as_basic_value_enum()
    };

    let ret_type = match ret_type.unwrap().as_ref() {
        TypeKind::List(t) => t,
        _ => unreachable!(),
    };

    let arg1 = match ret_type.as_ref() {
        TypeKind::Tuple(_) => 1,
        TypeKind::List(_) => 0,
        TypeKind::Nil => 0,
        TypeKind::Unit => 4,
        _ => type_to_int(&expr1.get_type()),
    };
    let call = context.builder.build_call(
        fn_value,
        &[
            arg.into(),
            context.ctx.i32_type().const_int(arg1, false).into(),
        ],
        "car_ptr",
    )?;
    let expr1_ptr = call
        .try_as_basic_value()
        .basic()
        .ok_or(CompilerError::Llvm(
            "Expected function to return a value".to_string(),
        ))?
        .into_pointer_value();

    context.builder.build_store(expr1_ptr, expr1)?;

    if is_first_element {
        let block = context
            .builder
            .get_insert_block()
            .ok_or_else(|| CompilerError::Llvm("No insert point".to_string()))?;
        let function = block
            .get_parent()
            .ok_or_else(|| CompilerError::Llvm("Basic Block has no parent function".to_string()))?;

        let aux_alloca = create_entry_block_alloca(
            context,
            function,
            "aux_alloca",
            context
                .ctx
                .ptr_type(AddressSpace::default())
                .as_basic_type_enum(),
        )?;

        context.builder.build_store(aux_alloca, expr1_ptr)?;
    }

    let expr2 = match exprs[1].as_ref() {
        ExprKind::Call(name, args, kind) if name == "cons" => {
            codegen_cons(args, kind.as_ref(), context, false)?
        }
        ExprKind::Literal(Literal::Unit) => context
            .ctx
            .ptr_type(AddressSpace::default())
            .const_null()
            .as_basic_value_enum(),
        _ => exprs[1].codegen(context)?,
    };

    let expr2_ptr = unsafe {
        context.builder.build_gep(
            context.ctx.i8_type(),
            expr1_ptr,
            &[context.ctx.i64_type().const_int(8, false)],
            "cdr_ptr",
        )
    }?;

    context.builder.build_store(expr2_ptr, expr2)?;
    Ok(expr1_ptr.as_basic_value_enum())
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
                    return Ok(context.ctx.bool_type().const_zero().as_basic_value_enum());
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
                        "Expression must have supported types".to_string(),
                    )),
                }
            }
            ExprKind::Call(name, args, ret_type) => {
                if name == "cons" {
                    return codegen_cons(args, ret_type.as_ref(), context, true);
                }
                let callee = get_function(name, context)?;
                let args_len = args.len();
                if args_len != callee.count_params() as usize {
                    return Err(CompilerError::Llvm(
                        "Incorrect # arguments passed".to_string(),
                    ));
                }
                let mut vec: Vec<BasicMetadataValueEnum> = Vec::new();

                for arg in args.iter() {
                    let arg_eval = arg.codegen(context)?;
                    vec.push(arg_eval.into());
                }

                let call = context.builder.build_call(callee, &vec, "calltmp");

                match call {
                    Ok(v) => {
                        let v = v.try_as_basic_value().basic().ok_or(CompilerError::Llvm(
                            "Expected function to return a value".to_string(),
                        ))?;
                        let v = if name == "car" {
                            let car_result = v.into_pointer_value();
                            let ptr_type = get_type(ret_type.as_ref().unwrap(), context);
                            let value = context.builder.build_load(
                                ptr_type,
                                car_result,
                                "extracted_value",
                            )?;
                            value.as_basic_value_enum()
                        } else {
                            v
                        };
                        Ok(v)
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
            ExprKind::Cons(_, _) => codegen_pair(context, self, true),
            ExprKind::Projection(val, idx, t) => {
                let val1 = val.codegen(context)?;
                let n = idx.codegen(context)?;
                let proj_result = gc_proj_function(&val1, n, context)?;
                let ptr_type = get_type(t.as_ref().unwrap(), context);
                let value = context
                    .builder
                    .build_load(ptr_type, proj_result, "extracted_value")?;
                Ok(value.as_basic_value_enum())
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

        let return_type = get_type(self.ret_type.as_ref().unwrap(), context);
        let fn_type = return_type.fn_type(&args_type, false);

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
                if context.module.get_global("llvm_gc_root_chain").is_none() {
                    let ptr_type = context.ctx.ptr_type(AddressSpace::default());
                    let global = context
                        .module
                        .add_global(ptr_type, None, "llvm_gc_root_chain");
                    global.set_linkage(inkwell::module::Linkage::WeakAny);
                    global.set_initializer(&ptr_type.const_null());
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

                let args_len = proto.args.len();
                if args_len != fn_value.count_params() as usize {
                    return Err(CompilerError::Llvm(
                        "Incorrect # arguments passed".to_string(),
                    ));
                }
                for (i, arg) in fn_value.get_param_iter().enumerate() {
                    arg.set_name(&proto.args[i].name);
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

                let body_value = body.codegen(context);
                let block = context
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompilerError::Llvm("No insert point".to_string()))?;

                context.builder.position_at_end(prologue_block);
                generate_prologue(context)?;
                context.builder.build_unconditional_branch(basic_block)?;

                context.builder.position_at_end(block);
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
                        let res = jit.call::<*const u8>(executer_addr);
                        print_cons(res, 1);
                        println!();
                    }
                    TypeKind::List(_) => {
                        let res = jit.call::<*const u8>(executer_addr);
                        print_cons(res, 0);
                        println!();
                    }
                    TypeKind::Nil => {
                        let res = jit.call::<*const u8>(executer_addr);
                        print_cons(res, 0);
                        println!();
                    }
                }
                jit.remove(rt)
            }
            _ => Ok(()),
        }
    }
}

fn print_cons(res: *const u8, t: i32) {
    if t == 0 {
        print!("[");
    } else {
        print!("(");
    }
    unsafe {
        let mut cell = res;
        let mut flag = false;
        while !cell.is_null() {
            if flag {
                print!(",");
            }
            let is_pointer = *(cell.sub(6) as *const i16) == 1;
            let cell_t = *(cell.sub(4) as *const i32);

            if is_pointer {
                print_cons(*(cell as *const *const u8), cell_t);
            } else {
                let car = *(cell as *const u64);
                if cell_t == 4 {
                    print!("()");
                } else {
                    print!("{}", car);
                }
            }
            let cdr = *(cell.add(8) as *const *const u8);
            cell = cdr;
            flag = true;
        }
    }
    if t == 0 {
        print!("]");
    } else {
        print!(")");
    }
}
