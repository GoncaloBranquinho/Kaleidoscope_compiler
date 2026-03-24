use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::{
    FloatPredicate, OptimizationLevel,
    module::Module,
    passes::PassBuilderOptions,
    support::LLVMString,
    types::BasicMetadataTypeEnum,
    values::{BasicMetadataValueEnum, CallSiteValue, FloatValue, FunctionValue},
};
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

use crate::parser::ast::{BinaryOp, Decl, Expr, Prototype};
use std::collections::HashMap;
use std::ffi::CString;
use std::mem::{forget, replace, transmute};

fn get_function<'ctx>(
    name: &str,
    context: &mut CodeGenBuilder<'ctx>,
) -> Result<FunctionValue<'ctx>, IRError> {
    if let Some(callee) = context.module.get_function(name) {
        return Ok(callee);
    }

    let proto = context.function_protos.get(name);
    if let Some(proto) = proto {
        let proto = proto.clone();
        if let LlvmValue::Function(callee_res) = proto.codegen(context)? {
            return Ok(callee_res);
        };
    }

    Err(IRError {
        message_error: "Unknown function referenced".to_string(),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct IRError {
    pub message_error: String,
}

impl From<BuilderError> for IRError {
    fn from(error: BuilderError) -> Self {
        IRError {
            message_error: format!("LLVM builder error: {:?}", error),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LlvmValue<'ctx> {
    Float(FloatValue<'ctx>),
    Function(FunctionValue<'ctx>),
    Call(CallSiteValue<'ctx>),
    Void,
}

impl<'ctx> TryFrom<&LlvmValue<'ctx>> for BasicMetadataValueEnum<'ctx> {
    type Error = IRError;

    fn try_from(value: &LlvmValue<'ctx>) -> Result<Self, Self::Error> {
        match value {
            LlvmValue::Float(f) => Ok((*f).into()),
            LlvmValue::Function(_) => Err(IRError {
                message_error: "Cannot pass a function as a regular argument".to_string(),
            }),
            LlvmValue::Call(_) => Err(IRError {
                message_error: "Cannot pass a call as a regular argument".to_string(),
            }),
            LlvmValue::Void => Err(IRError {
                message_error: "Cannot pass void as argument".to_string(),
            }),
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

    pub fn add_module(
        &self,
        tsm: LLVMOrcThreadSafeModuleRef,
        rt: LLVMOrcResourceTrackerRef,
    ) -> Result<(), LLVMErrorRef> {
        unsafe {
            let err = LLVMOrcLLJITAddLLVMIRModuleWithRT(self.lljit, rt, tsm);
            if !err.is_null() { Err(err) } else { Ok(()) }
        }
    }

    pub fn lookup(&self, name: &str) -> Result<LLVMOrcExecutorAddress, LLVMErrorRef> {
        unsafe {
            let mut result: LLVMOrcExecutorAddress = 0;

            let cname = CString::new(name).unwrap();

            let err = LLVMOrcLLJITLookup(self.lljit, &mut result, cname.as_ptr());
            if !err.is_null() {
                return Err(err);
            }
            Ok(result)
        }
    }

    pub fn call(&self, addr: LLVMOrcExecutorAddress) -> f64 {
        unsafe {
            let function: extern "C" fn() -> f64 = transmute(addr as usize);
            function()
        }
    }

    pub fn remove(&self, rt: LLVMOrcResourceTrackerRef) -> Result<(), LLVMErrorRef> {
        unsafe {
            let err = LLVMOrcResourceTrackerRemove(rt);
            if !err.is_null() { Err(err) } else { Ok(()) }
        }
    }
}

pub struct CodeGenBuilder<'ctx> {
    pub ctx: &'ctx Context,
    pub tsc: &'ctx LLVMOrcThreadSafeContextRef,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub target_machine: TargetMachine,
    pub map: HashMap<String, LlvmValue<'ctx>>,
    pub function_protos: HashMap<String, Prototype>,
}

pub trait CodeGen<'ctx> {
    fn codegen(&self, context: &mut CodeGenBuilder<'ctx>) -> Result<LlvmValue<'ctx>, IRError>;
}

impl<'ctx> CodeGenBuilder<'ctx> {
    pub fn new(
        ctx: &'ctx Context,
        tsc: &'ctx LLVMOrcThreadSafeContextRef,
    ) -> Result<CodeGenBuilder<'ctx>, LLVMString> {
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

        let target = Target::from_triple(&triple)?;

        let target_machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                OptimizationLevel::None,
                RelocMode::Default,
                CodeModel::Default,
            )
            .unwrap();

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
    fn codegen(&self, context: &mut CodeGenBuilder<'ctx>) -> Result<LlvmValue<'ctx>, IRError> {
        match self {
            Expr::DoubleLit { value } => {
                let f64_type = context.ctx.f64_type();
                Ok(LlvmValue::Float(f64_type.const_float(*value)))
            }

            Expr::Var { name } => {
                if let Some(value) = context.map.get(name) {
                    Ok(value.clone())
                } else {
                    Err(IRError {
                        message_error: format!("Unknown variable name: {}", name),
                    })
                }
            }

            Expr::Binary { op, left, right } => {
                let l = left.codegen(context)?;
                let r = right.codegen(context)?;
                match (l, r) {
                    (LlvmValue::Float(value_l), LlvmValue::Float(value_r)) => {
                        let res = match op {
                            BinaryOp::Add => {
                                context.builder.build_float_add(value_l, value_r, "addtmp")
                            }

                            BinaryOp::Sub => {
                                context.builder.build_float_sub(value_l, value_r, "subtmp")
                            }

                            BinaryOp::Mult => {
                                context.builder.build_float_mul(value_l, value_r, "multmp")
                            }

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
                                    .build_unsigned_int_to_float(cmp, f64_type, "booltmp")
                            }
                        };
                        Ok(LlvmValue::Float(res?))
                    }
                    _ => Err(IRError {
                        message_error: "Expression must be of float type".to_string(),
                    }),
                }
            }
            Expr::Call { name, args } => {
                let callee = get_function(name, context)?;
                if args.len() != callee.count_params() as usize {
                    return Err(IRError {
                        message_error: "Incorrect # arguments passed".to_string(),
                    });
                }
                let mut vec: Vec<BasicMetadataValueEnum> = Vec::new();
                let tmp = 0..args.len();
                for i in tmp {
                    let arg_eval = args[i].codegen(context)?;
                    vec.push(TryFrom::try_from(&arg_eval)?);
                }

                let call = context.builder.build_call(callee, &vec, "calltmp");
                match call {
                    Ok(v) => {
                        let value = v.try_as_basic_value().basic().ok_or(IRError {
                            message_error: "Expected function to return a value".to_string(),
                        })?;
                        Ok(LlvmValue::Float(value.into_float_value()))
                    }
                    Err(e) => Err(e.into()),
                }
            }
        }
    }
}

impl<'ctx> CodeGen<'ctx> for Prototype {
    fn codegen(&self, context: &mut CodeGenBuilder<'ctx>) -> Result<LlvmValue<'ctx>, IRError> {
        let f64_type = context.ctx.f64_type();
        let args_type: Vec<BasicMetadataTypeEnum> = vec![f64_type.into(); self.args.len()];
        let fn_type = f64_type.fn_type(&args_type, false);

        if context.module.get_function(&self.name).is_some() {
            return Err(IRError {
                message_error: "Function cannot have multiple declarations".to_string(),
            });
        }

        let fn_value = context.module.add_function(&self.name, fn_type, None);

        for (i, arg) in fn_value.get_param_iter().enumerate() {
            arg.set_name(&self.args[i].name);
        }

        Ok(LlvmValue::Function(fn_value))
    }
}

impl<'ctx> CodeGen<'ctx> for Decl {
    fn codegen(&self, context: &mut CodeGenBuilder<'ctx>) -> Result<LlvmValue<'ctx>, IRError> {
        match self {
            Decl::Extern(proto) => proto.codegen(context),

            Decl::Function { proto, body } => {
                context
                    .function_protos
                    .insert(proto.name.clone(), proto.clone());

                let fn_value = get_function(&proto.name, context)?;
                if fn_value.get_first_basic_block().is_some() {
                    return Err(IRError {
                        message_error: "Function cannot be redefnied".to_string(),
                    });
                }

                let basic_block = context.ctx.append_basic_block(fn_value, "entry");

                context.builder.position_at_end(basic_block);
                context.map.clear();

                if proto.args.len() != fn_value.count_params() as usize {
                    return Err(IRError {
                        message_error: "Incorrect # arguments passed".to_string(),
                    });
                }

                for (i, arg) in fn_value.get_param_iter().enumerate() {
                    arg.set_name(&proto.args[i].name);
                    context.map.insert(
                        arg.get_name().to_string_lossy().to_string(),
                        LlvmValue::Float(arg.into_float_value()),
                    );
                }

                let body_value = body.codegen(context);

                match body_value {
                    Err(error) => {
                        unsafe {
                            fn_value.delete();
                        };
                        return Err(error);
                    }

                    Ok(LlvmValue::Float(value)) => {
                        let _ = context.builder.build_return(Some(&value));
                        fn_value.verify(true);
                    }

                    Ok(LlvmValue::Void) => {
                        let _ = context.builder.build_return(None);
                        fn_value.verify(true);
                    }
                    _ => {
                        fn_value.verify(true);
                    }
                }

                // optimizing the newly created funciton

                let options = PassBuilderOptions::create();

                if let Err(e) = context.module.run_passes(
                    "function(instcombine,reassociate,gvn,simplifycfg)",
                    &context.target_machine,
                    options,
                ) {
                    return Err(IRError {
                        message_error: e.to_string_lossy().to_string(),
                    });
                }

                Ok(LlvmValue::Function(fn_value))
            }
        }
    }
}

pub trait JitCompiler<'ctx> {
    fn compile(
        &self,
        codegen_builder: &mut CodeGenBuilder<'ctx>,
        jit: &mut KaleidoscopeJIT,
    ) -> Option<LLVMOrcResourceTrackerRef>;

    fn run(&self, rt: LLVMOrcResourceTrackerRef, jit: &mut KaleidoscopeJIT);
}

impl<'ctx> JitCompiler<'ctx> for Decl {
    fn compile(
        &self,
        codegen_builder: &mut CodeGenBuilder<'ctx>,
        jit: &mut KaleidoscopeJIT,
    ) -> Option<LLVMOrcResourceTrackerRef> {
        match self {
            Decl::Extern(proto) => {
                codegen_builder
                    .function_protos
                    .insert(proto.name.clone(), proto.clone());
                None
            }

            Decl::Function { proto, body: _ } => {
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
                        jit.add_module(tsm, rt).unwrap();
                        Some(rt)
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
                        jit.add_module(tsm, rt).unwrap();
                        None
                    }
                }
            }
        }
    }

    fn run(&self, rt: LLVMOrcResourceTrackerRef, jit: &mut KaleidoscopeJIT) {
        match self {
            Decl::Extern(_proto) => {}

            Decl::Function { .. } => {
                let executer_addr = jit.lookup("__anon_expr").unwrap();
                let res = jit.call(executer_addr);
                println!("Evaluated to {res}");
                jit.remove(rt).unwrap();
            }
        }
    }
}
