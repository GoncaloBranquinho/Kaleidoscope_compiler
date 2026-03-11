use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::{
    FloatPredicate, OptimizationLevel,
    execution_engine::ExecutionEngine,
    module::Module,
    support::LLVMString,
    types::BasicMetadataTypeEnum,
    values::{BasicMetadataValueEnum, CallSiteValue, FloatValue, FunctionValue},
};

use crate::parser::ast::{BinaryOp, Decl, Expr, Prototype};
use std::collections::HashMap;

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

pub struct CodeGenBuilder<'ctx> {
    pub ctx: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub execution_engine: ExecutionEngine<'ctx>,
    pub map: HashMap<String, LlvmValue<'ctx>>,
}

#[allow(dead_code)]
pub trait CodeGen<'ctx> {
    fn codegen(&self, context: &mut CodeGenBuilder<'ctx>) -> Result<LlvmValue<'ctx>, IRError>;
}

impl<'ctx> CodeGenBuilder<'ctx> {
    pub fn new(ctx: &'ctx Context) -> Result<CodeGenBuilder<'ctx>, LLVMString> {
        let module = ctx.create_module("main");
        let builder = ctx.create_builder();
        let execution_engine = module.create_jit_execution_engine(OptimizationLevel::None)?;
        let map = HashMap::new();

        Ok(CodeGenBuilder {
            ctx,
            module,
            builder,
            execution_engine,
            map,
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
                let callee = context.module.get_function(name).ok_or(IRError {
                    message_error: "Unknown function referenced".to_string(),
                })?;
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
                    Ok(v) => Ok(LlvmValue::Call(v)),
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
                let fn_value = match context.module.get_function(&proto.name) {
                    Some(f) => f,
                    None => match proto.codegen(context)? {
                        LlvmValue::Function(f) => f,
                        _ => {
                            return Err(IRError {
                                message_error: "Function LlvmValue expected".to_string(),
                            });
                        }
                    },
                };

                if fn_value.get_first_basic_block().is_some() {
                    return Err(IRError {
                        message_error: "Function cannot be redefnied".to_string(),
                    });
                }

                let basic_block = context.ctx.append_basic_block(fn_value, "entry");

                context.builder.position_at_end(basic_block);
                context.map.clear();

                for arg in fn_value.get_param_iter() {
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

                Ok(LlvmValue::Function(fn_value))
            }
        }
    }
}
