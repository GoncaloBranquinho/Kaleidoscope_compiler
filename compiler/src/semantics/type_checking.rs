use std::collections::HashMap;

use crate::parser::{
    BinaryOp, DeclKind, Expr, ExprKind, Literal, Program, Prototype, Type, TypeKind,
    UnaryOp::UserDefined, expr::ConsKind,
};

pub fn compare_types(t1: &Type, t2: &Type) -> bool {
    match (t1.as_ref(), t2.as_ref()) {
        (TypeKind::Nil, TypeKind::List(_)) => true,
        (TypeKind::List(_), TypeKind::Nil) => true,
        _ => t1 == t2,
    }
}

pub fn is_runtime_function(
    name: &str,
    params: &mut [Expr],
    symbol_table: &mut SymbolTable,
) -> Result<Option<Type>, SemanticErrorKind> {
    match name {
        "empty" => {
            if params.len() != 1 {
                Err(SemanticErrorKind::InvalidArgumentSize {
                    expected: 1,
                    found: params.len(),
                })
            } else {
                let t = params[0].type_check(symbol_table)?;
                match t.as_ref() {
                    TypeKind::List(_) | TypeKind::Nil => Ok(Some(Box::new(TypeKind::I64))),
                    _ => Err(SemanticErrorKind::NotHeapObject { found: t }),
                }
            }
        }
        "car" => {
            if params.len() != 1 {
                Err(SemanticErrorKind::InvalidArgumentSize {
                    expected: 1,
                    found: params.len(),
                })
            } else {
                let t = params[0].type_check(symbol_table)?;
                match t.as_ref() {
                    TypeKind::List(list_t) => Ok(Some(list_t.clone())),
                    _ => Err(SemanticErrorKind::NotHeapObject { found: t }),
                }
            }
        }
        "cdr" => {
            if params.len() != 1 {
                Err(SemanticErrorKind::InvalidArgumentSize {
                    expected: 1,
                    found: params.len(),
                })
            } else {
                let t = params[0].type_check(symbol_table)?;
                match t.as_ref() {
                    TypeKind::List(_) => Ok(Some(t)),
                    _ => Err(SemanticErrorKind::NotHeapObject { found: t }),
                }
            }
        }
        "cons" => {
            if params.len() != 2 {
                Err(SemanticErrorKind::InvalidArgumentSize {
                    expected: 2,
                    found: params.len(),
                })
            } else {
                let t0 = params[0].type_check(symbol_table)?;
                let t1 = params[1].type_check(symbol_table)?;
                let t = Box::new(TypeKind::List(t0));
                if *t1 != TypeKind::Unit && !compare_types(&t, &t1) {
                    Err(SemanticErrorKind::TypeMismatch {
                        expected: t,
                        found: t1,
                    })
                } else {
                    match t1.as_ref() {
                        TypeKind::List(_) | TypeKind::Unit => Ok(Some(t)),
                        _ => Err(SemanticErrorKind::NotHeapObject { found: t1 }),
                    }
                }
            }
        }
        _ => Err(SemanticErrorKind::UnknownFunction {
            name: name.to_string(),
        }),
    }
}

pub struct SymbolTable {
    symbol_table: Vec<HashMap<String, Option<Type>>>,
    prototype_table: HashMap<String, Prototype>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            symbol_table: Vec::new(),
            prototype_table: HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: String, t: Option<Type>, scope: usize) -> Option<Option<Type>> {
        self.symbol_table.get_mut(scope).unwrap().insert(name, t)
    }

    pub fn push(&mut self) {
        self.symbol_table.push(HashMap::new());
    }

    pub fn pop(&mut self) {
        self.symbol_table.pop();
    }

    pub fn get(&self, name: &String) -> (Option<Option<Type>>, usize) {
        for (i, scope) in self.symbol_table.iter().rev().enumerate() {
            if let Some(t) = scope.get(name) {
                return (Some(t.clone()), i);
            }
        }
        (None, 0)
    }

    pub fn get_proto(&self, name: &String) -> Option<Prototype> {
        self.prototype_table.get(name).cloned()
    }

    fn insert_proto(&mut self, name: String, proto: Prototype) -> Option<Prototype> {
        self.prototype_table.insert(name, proto)
    }

    fn len(&self) -> usize {
        self.symbol_table.len()
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

pub trait TypeCheck {
    type Output;
    fn type_check(
        &mut self,
        symbol_table: &mut SymbolTable,
    ) -> Result<Self::Output, SemanticErrorKind>;
}

impl TypeCheck for Program {
    type Output = ();

    fn type_check(
        &mut self,
        symbol_table: &mut SymbolTable,
    ) -> Result<Self::Output, SemanticErrorKind> {
        symbol_table.push();
        for decl in self.iter_mut() {
            decl.type_check(symbol_table)?;
        }
        symbol_table.pop();
        Ok(())
    }
}

impl TypeCheck for DeclKind {
    type Output = ();

    fn type_check(
        &mut self,
        symbol_table: &mut SymbolTable,
    ) -> Result<Self::Output, SemanticErrorKind> {
        match self {
            DeclKind::Extern(prototype) => {
                symbol_table.insert(
                    prototype.name.clone(),
                    prototype.ret_type.clone(),
                    symbol_table.len() - 1,
                );
                symbol_table.insert_proto(prototype.name.clone(), prototype.clone());
                symbol_table.push();
                prototype.type_check(symbol_table)?;
                symbol_table.pop();
            }
            DeclKind::Function(prototype, body) => {
                if prototype.name != "__anon_expr" {
                    symbol_table.insert(
                        prototype.name.clone(),
                        prototype.ret_type.clone(),
                        symbol_table.len() - 1,
                    );
                    symbol_table.insert_proto(prototype.name.clone(), prototype.clone());
                }
                symbol_table.push();
                prototype.type_check(symbol_table)?;
                let body_type = body.type_check(symbol_table)?;
                if let Some(ret_type) = &prototype.ret_type {
                    if !compare_types(ret_type, &body_type) {
                        return Err(SemanticErrorKind::TypeMismatch {
                            expected: prototype.ret_type.clone().unwrap(),
                            found: body_type,
                        });
                    }
                } else {
                    prototype.ret_type = Some(body_type);
                }
                symbol_table.pop();
            }
        }
        Ok(())
    }
}

impl TypeCheck for Prototype {
    type Output = ();

    fn type_check(
        &mut self,
        symbol_table: &mut SymbolTable,
    ) -> Result<Self::Output, SemanticErrorKind> {
        for arg in self.args.iter() {
            symbol_table.insert(
                arg.name.clone(),
                Some(arg.typ.clone()),
                symbol_table.len() - 1,
            );
        }
        Ok(())
    }
}

impl TypeCheck for Expr {
    type Output = Type;

    fn type_check(
        &mut self,
        symbol_table: &mut SymbolTable,
    ) -> Result<Self::Output, SemanticErrorKind> {
        match self.as_mut() {
            ExprKind::Identifier(name, type_kind) => {
                let (t, _) = symbol_table.get(name);
                if let Some(Some(typ)) = t {
                    *type_kind = Some(typ.clone());
                    Ok(typ)
                } else if let Some(None) = t {
                    Err(SemanticErrorKind::UninitializedVariable { name: name.clone() })
                } else {
                    Err(SemanticErrorKind::UndefinedVariable { name: name.clone() })
                }
            }
            ExprKind::Var(vars, body) => {
                symbol_table.push();
                for var in vars.iter_mut() {
                    if let Some(var_body) = var.val.as_mut() {
                        let body_type = var_body.type_check(symbol_table)?;
                        if let Some(var_type) = var.t.as_ref()
                            && !compare_types(var_type, &body_type)
                        {
                            return Err(SemanticErrorKind::TypeMismatch {
                                expected: var_type.clone(),
                                found: body_type,
                            });
                        }
                        symbol_table.insert(
                            var.name.clone(),
                            Some(body_type.clone()),
                            symbol_table.len() - 1,
                        );
                        var.t = Some(body_type);
                    } else {
                        symbol_table.insert(var.name.clone(), None, symbol_table.len() - 1);
                    }
                }
                let body_type = body.type_check(symbol_table);
                for var in vars.iter_mut() {
                    if var.t.is_none()
                        && let (Some(Some(typ)), _) = symbol_table.get(&var.name)
                    {
                        var.t = Some(typ.clone());
                    }
                }
                symbol_table.pop();
                body_type
            }
            ExprKind::Literal(literal) => literal.type_check(symbol_table),
            ExprKind::Binary(op, left, right) => match op {
                BinaryOp::Add | BinaryOp::Lt | BinaryOp::Mult | BinaryOp::Sub => {
                    let left_type = left.type_check(symbol_table)?;
                    let right_type = right.type_check(symbol_table)?;
                    if !compare_types(&left_type, &right_type) {
                        return Err(SemanticErrorKind::TypeMismatch {
                            expected: left_type,
                            found: right_type,
                        });
                    }
                    if let BinaryOp::Lt = op {
                        Ok(Box::new(TypeKind::I64))
                    } else {
                        Ok(left_type)
                    }
                }
                BinaryOp::Assign => {
                    let left_type = left.type_check(symbol_table);
                    let right_type = right.type_check(symbol_table)?;
                    let name = match left.as_ref() {
                        ExprKind::Identifier(name, _) => name,
                        ExprKind::Projection(id, _, _) => {
                            let left_type = left_type?;
                            if !compare_types(&left_type, &right_type) {
                                return Err(SemanticErrorKind::TypeMismatch {
                                    expected: left_type,
                                    found: right_type,
                                });
                            }
                            if !matches!(
                                id.as_ref(),
                                ExprKind::Identifier(_, _) | ExprKind::Projection(_, _, _)
                            ) {
                                return Err(SemanticErrorKind::Immutable { val: left.clone() });
                            }
                            return Ok(Box::new(TypeKind::Unit));
                        }
                        _ => unreachable!(),
                    };
                    let (id_typ, scope) = symbol_table.get(name);
                    if let Some(Some(typ)) = id_typ {
                        if !compare_types(&right_type, &typ) {
                            return Err(SemanticErrorKind::TypeMismatch {
                                expected: typ,
                                found: right_type,
                            });
                        }
                        if let TypeKind::Nil = typ.as_ref() {
                            symbol_table.insert(
                                name.clone(),
                                Some(right_type.clone()),
                                symbol_table.len() - 1 - scope,
                            );
                        }
                        Ok(Box::new(TypeKind::Unit))
                    } else if let Some(None) = id_typ {
                        symbol_table.insert(
                            name.clone(),
                            Some(right_type.clone()),
                            symbol_table.len() - 1 - scope,
                        );
                        Ok(Box::new(TypeKind::Unit))
                    } else {
                        Err(SemanticErrorKind::UndefinedVariable { name: name.clone() })
                    }
                }
                BinaryOp::UserDefined(c) => {
                    let mut name = "binary".to_string();
                    name.push(*c);
                    if let Some(proto) = symbol_table.get_proto(&name) {
                        let left_type = left.type_check(symbol_table)?;
                        let right_type = right.type_check(symbol_table)?;
                        if !compare_types(&left_type, &proto.args[0].typ) {
                            return Err(SemanticErrorKind::TypeMismatch {
                                expected: proto.args[0].typ.clone(),
                                found: left_type,
                            });
                        }
                        if !compare_types(&right_type, &proto.args[1].typ) {
                            return Err(SemanticErrorKind::TypeMismatch {
                                expected: proto.args[1].typ.clone(),
                                found: right_type,
                            });
                        }
                        Ok(proto.ret_type.unwrap())
                    } else {
                        Err(SemanticErrorKind::UnknownOperator { name: *c })
                    }
                }
            },
            ExprKind::Unary(op, expr) => match op {
                UserDefined(c) => {
                    let mut name = "unary".to_string();
                    name.push(*c);
                    if let Some(proto) = symbol_table.get_proto(&name) {
                        let param_type = expr.type_check(symbol_table)?;
                        if !compare_types(&param_type, &proto.args[0].typ) {
                            return Err(SemanticErrorKind::TypeMismatch {
                                expected: proto.args[0].typ.clone(),
                                found: param_type,
                            });
                        }
                        Ok(proto.ret_type.unwrap())
                    } else {
                        Err(SemanticErrorKind::UnknownOperator { name: *c })
                    }
                }
            },
            ExprKind::IfThenElse(cond, expr1, expr2) => {
                let cond_type = cond.type_check(symbol_table)?;
                symbol_table.push();
                let expr1_type = expr1.type_check(symbol_table)?;
                symbol_table.pop();
                symbol_table.push();
                let expr2_type = expr2.type_check(symbol_table)?;
                symbol_table.pop();
                if !matches!(cond_type.as_ref(), &TypeKind::I64 | &TypeKind::F64) {
                    Err(SemanticErrorKind::TypeMismatch {
                        expected: Box::new(TypeKind::I64),
                        found: cond_type,
                    })
                } else if !compare_types(&expr1_type, &expr2_type) {
                    Err(SemanticErrorKind::TypeMismatch {
                        expected: expr1_type,
                        found: expr2_type,
                    })
                } else {
                    Ok(expr1_type)
                }
            }
            ExprKind::Call(name, params, ret) => {
                if let Some(proto) = symbol_table.get_proto(name) {
                    if proto.args.len() != params.len() {
                        return Err(SemanticErrorKind::InvalidArgumentSize {
                            expected: proto.args.len(),
                            found: params.len(),
                        });
                    }
                    for (i, param) in params.iter_mut().enumerate() {
                        let param_type = param.type_check(symbol_table)?;
                        if !compare_types(&param_type, &proto.args[i].typ) {
                            return Err(SemanticErrorKind::TypeMismatch {
                                expected: proto.args[i].typ.clone(),
                                found: param_type,
                            });
                        }
                    }
                    let t = proto.ret_type.unwrap();
                    *ret = Some(t.clone());
                    Ok(t)
                } else if let Some(ret_type) = is_runtime_function(name, params, symbol_table)? {
                    *ret = Some(ret_type.clone());
                    Ok(ret_type)
                } else {
                    Err(SemanticErrorKind::UnknownFunction { name: name.clone() })
                }
            }
            ExprKind::ForLoop(id, start, end, step, body) => {
                symbol_table.push();
                let start_type = start.type_check(symbol_table)?;

                symbol_table.insert(id.clone(), Some(start_type.clone()), symbol_table.len() - 1);

                let end_type = end.type_check(symbol_table)?;
                if end_type.as_ref() != &TypeKind::I64 {
                    return Err(SemanticErrorKind::TypeMismatch {
                        expected: Box::new(TypeKind::I64),
                        found: end_type,
                    });
                }

                if let Some(step) = step {
                    let step_type = step.type_check(symbol_table)?;
                    if !compare_types(&step_type, &start_type) {
                        return Err(SemanticErrorKind::TypeMismatch {
                            expected: start_type,
                            found: step_type,
                        });
                    }
                }

                body.type_check(symbol_table)?;
                symbol_table.pop();
                Ok(Box::new(TypeKind::Unit))
            }
            ExprKind::Seq(exprs) => {
                let size = exprs.len();
                for (i, expr) in exprs.iter_mut().enumerate() {
                    let expr_type = expr.type_check(symbol_table)?;
                    if i != size - 1 {
                        if expr_type.as_ref() != &TypeKind::Unit {
                            return Err(SemanticErrorKind::TypeMismatch {
                                expected: Box::new(TypeKind::Unit),
                                found: expr_type,
                            });
                        }
                    } else {
                        return Ok(expr_type);
                    }
                }
                Ok(Box::new(TypeKind::Unit))
            }
            ExprKind::Cons(exprs, kind) => {
                let mut types = Vec::new();
                for expr in exprs.iter_mut() {
                    types.push(expr.type_check(symbol_table)?);
                }
                match kind {
                    ConsKind::List => {
                        if !types.is_empty() {
                            let t1 = types[0].clone();
                            let mut i = 1;
                            while i < types.len() {
                                if !compare_types(&t1, &types[i]) {
                                    return Err(SemanticErrorKind::UniqueTypes);
                                }
                                i += 1;
                            }
                            Ok(Box::new(TypeKind::List(t1)))
                        } else {
                            Ok(Box::new(TypeKind::Nil))
                        }
                    }
                    ConsKind::Tuple => Ok(Box::new(TypeKind::Tuple(types))),
                }
            }
            ExprKind::Projection(val, idx, result_typ) => {
                let val_type = val.type_check(symbol_table)?;

                let idx_type = idx.type_check(symbol_table)?;

                let n = match idx.as_ref() {
                    ExprKind::Literal(Literal::I64(n)) => Some(*n),
                    _ => None,
                };

                if idx_type.as_ref() != &TypeKind::I64 {
                    return Err(SemanticErrorKind::TypeMismatch {
                        expected: Box::new(TypeKind::I64),
                        found: idx_type,
                    });
                }

                let t = match val_type.as_ref() {
                    TypeKind::Tuple(t) => t,
                    TypeKind::List(t) => {
                        *result_typ = Some(t.clone());
                        return Ok(t.clone());
                    }
                    TypeKind::Nil => {
                        *result_typ = Some(val_type.clone());
                        return Ok(val_type);
                    }
                    _ => {
                        //change errorKind
                        return Err(SemanticErrorKind::TypeMismatch {
                            expected: Box::new(TypeKind::Tuple(vec![])),
                            found: val_type,
                        });
                    }
                };

                let n_val = n.unwrap();

                if n_val < 0 || n_val >= t.len() as i64 {
                    return Err(SemanticErrorKind::InvalidField {
                        idx: n_val,
                        on: Box::new(TypeKind::Tuple(t.clone())),
                    });
                }
                let result = t[n_val as usize].clone();
                *result_typ = Some(result.clone());
                Ok(result)
            }
        }
    }
}

impl TypeCheck for Literal {
    type Output = Type;

    fn type_check(
        &mut self,
        _symbol_table: &mut SymbolTable,
    ) -> Result<Self::Output, SemanticErrorKind> {
        match self {
            Literal::F64(_) => Ok(Box::new(TypeKind::F64)),
            Literal::I64(_) => Ok(Box::new(TypeKind::I64)),
            Literal::Unit => Ok(Box::new(TypeKind::Unit)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum SemanticErrorKind {
    TypeMismatch { expected: Type, found: Type },
    UndefinedVariable { name: String },
    UninitializedVariable { name: String },
    InvalidArgumentSize { expected: usize, found: usize },
    UnknownFunction { name: String },
    UnknownOperator { name: char },
    InvalidField { idx: i64, on: Type },
    Immutable { val: Expr },
    UniqueTypes,
    NotHeapObject { found: Type },
}
