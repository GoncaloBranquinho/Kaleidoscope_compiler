use std::collections::HashMap;

use kaleipl::{
    lexer::Lexer,
    parser::{
        Arg, BinaryOp, DeclKind, ExprKind, Literal, Parser, Program, Prototype, TypeKind, UnaryOp,
    },
};

fn assert_ast(ast: &Program, expected_ast: &Program) {
    for (i, node) in ast.iter().enumerate() {
        assert_eq!(*node, expected_ast[i]);
    }
}

#[test]
fn test_program() {
    let input = "extern putchard(x: double) : double;

def binary| 5 (LHS: double RHS: double) : double if LHS then 1.0 else if RHS then 1.0 else 0.0;

def binary> 10 (x: double y: double): double if y < x then 1.0 else 0.0;

def printdensity(d: double): double if d > 8.0 then putchard(32.0) else if d > 4.0 then putchard(46.0) else if d > 2.0 then putchard(43.0) else putchard(42.0); 

def binary: 1 (x: double y: double): double y;

def mandelconverger(real: double imag: double iters: double creal: double cimag: double): double if iters > 255.0 | (real*real + imag*imag > 4.0) then iters else mandelconverger(real*real - imag*imag + creal, 2.0*real*imag + cimag, iters+1.0, creal, cimag);

def mandelconverge(real: double imag: double): double mandelconverger(real, imag, 0.0, real, imag);

def mandelhelp(xmin: double xmax: double xstep: double ymin: double ymax: double ystep: double): double for y = ymin, y < ymax, ystep in (( for x = xmin, x < xmax, xstep in printdensity(mandelconverge(x,y))) : putchard(10.0));

def mandel(realstart: double imagstart: double realmag: double imagmag: double): double mandelhelp(realstart, realstart + realmag*78.0, realmag, imagstart, imagstart+imagmag*40.0, imagmag);

def unary- (v: double): double 0.0-v;

mandel(-2.3, -1.3, 0.05, 0.07);
";
    let mut binop_precedence: HashMap<char, i8> = HashMap::new();
    binop_precedence.insert('=', 2);
    binop_precedence.insert('<', 10);
    binop_precedence.insert('+', 20);
    binop_precedence.insert('-', 20);
    binop_precedence.insert('*', 40);

    let lexer = Lexer::new(input.char_indices().peekable());
    let mut parser = Parser::new(lexer, &mut binop_precedence);
    let ast = parser.parse_program().unwrap();

    let expected_ast: Vec<DeclKind> = vec![
        DeclKind::Extern(Prototype::new(
            "putchard".to_string(),
            vec![Arg::new("x".to_string(), Box::new(TypeKind::F64))],
            Some(Box::new(TypeKind::F64)),
        )),
        DeclKind::Function(
            Prototype::new(
                "binary|".to_string(),
                vec![
                    Arg::new("LHS".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("RHS".to_string(), Box::new(TypeKind::F64)),
                ],
                Some(Box::new(TypeKind::F64)),
            ),
            Box::new(ExprKind::IfThenElse(
                Box::new(ExprKind::Identifier("LHS".to_string(), None)),
                Box::new(ExprKind::Literal(Literal::F64(1.0))),
                Box::new(ExprKind::IfThenElse(
                    Box::new(ExprKind::Identifier("RHS".to_string(), None)),
                    Box::new(ExprKind::Literal(Literal::F64(1.0))),
                    Box::new(ExprKind::Literal(Literal::F64(0.0))),
                )),
            )),
        ),
        DeclKind::Function(
            Prototype::new(
                "binary>".to_string(),
                vec![
                    Arg::new("x".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("y".to_string(), Box::new(TypeKind::F64)),
                ],
                Some(Box::new(TypeKind::F64)),
            ),
            Box::new(ExprKind::IfThenElse(
                Box::new(ExprKind::Binary(
                    BinaryOp::Lt,
                    Box::new(ExprKind::Identifier("y".to_string(), None)),
                    Box::new(ExprKind::Identifier("x".to_string(), None)),
                )),
                Box::new(ExprKind::Literal(Literal::F64(1.0))),
                Box::new(ExprKind::Literal(Literal::F64(0.0))),
            )),
        ),
        DeclKind::Function(
            Prototype::new(
                "printdensity".to_string(),
                vec![Arg::new("d".to_string(), Box::new(TypeKind::F64))],
                Some(Box::new(TypeKind::F64)),
            ),
            Box::new(ExprKind::IfThenElse(
                Box::new(ExprKind::Binary(
                    BinaryOp::UserDefined('>'),
                    Box::new(ExprKind::Identifier("d".to_string(), None)),
                    Box::new(ExprKind::Literal(Literal::F64(8.0))),
                )),
                Box::new(ExprKind::Call(
                    "putchard".to_string(),
                    vec![Box::new(ExprKind::Literal(Literal::F64(32.0)))],
                )),
                Box::new(ExprKind::IfThenElse(
                    Box::new(ExprKind::Binary(
                        BinaryOp::UserDefined('>'),
                        Box::new(ExprKind::Identifier("d".to_string(), None)),
                        Box::new(ExprKind::Literal(Literal::F64(4.0))),
                    )),
                    Box::new(ExprKind::Call(
                        "putchard".to_string(),
                        vec![Box::new(ExprKind::Literal(Literal::F64(46.0)))],
                    )),
                    Box::new(ExprKind::IfThenElse(
                        Box::new(ExprKind::Binary(
                            BinaryOp::UserDefined('>'),
                            Box::new(ExprKind::Identifier("d".to_string(), None)),
                            Box::new(ExprKind::Literal(Literal::F64(2.0))),
                        )),
                        Box::new(ExprKind::Call(
                            "putchard".to_string(),
                            vec![Box::new(ExprKind::Literal(Literal::F64(43.0)))],
                        )),
                        Box::new(ExprKind::Call(
                            "putchard".to_string(),
                            vec![Box::new(ExprKind::Literal(Literal::F64(42.0)))],
                        )),
                    )),
                )),
            )),
        ),
        DeclKind::Function(
            Prototype::new(
                "binary:".to_string(),
                vec![
                    Arg::new("x".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("y".to_string(), Box::new(TypeKind::F64)),
                ],
                Some(Box::new(TypeKind::F64)),
            ),
            Box::new(ExprKind::Identifier("y".to_string(), None)),
        ),
        DeclKind::Function(
            Prototype::new(
                "mandelconverger".to_string(),
                vec![
                    Arg::new("real".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("imag".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("iters".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("creal".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("cimag".to_string(), Box::new(TypeKind::F64)),
                ],
                Some(Box::new(TypeKind::F64)),
            ),
            Box::new(ExprKind::IfThenElse(
                Box::new(ExprKind::Binary(
                    BinaryOp::UserDefined('|'),
                    Box::new(ExprKind::Binary(
                        BinaryOp::UserDefined('>'),
                        Box::new(ExprKind::Identifier("iters".to_string(), None)),
                        Box::new(ExprKind::Literal(Literal::F64(255.0))),
                    )),
                    Box::new(ExprKind::Binary(
                        BinaryOp::UserDefined('>'),
                        Box::new(ExprKind::Binary(
                            BinaryOp::Add,
                            Box::new(ExprKind::Binary(
                                BinaryOp::Mult,
                                Box::new(ExprKind::Identifier("real".to_string(), None)),
                                Box::new(ExprKind::Identifier("real".to_string(), None)),
                            )),
                            Box::new(ExprKind::Binary(
                                BinaryOp::Mult,
                                Box::new(ExprKind::Identifier("imag".to_string(), None)),
                                Box::new(ExprKind::Identifier("imag".to_string(), None)),
                            )),
                        )),
                        Box::new(ExprKind::Literal(Literal::F64(4.0))),
                    )),
                )),
                Box::new(ExprKind::Identifier("iters".to_string(), None)),
                Box::new(ExprKind::Call(
                    "mandelconverger".to_string(),
                    vec![
                        Box::new(ExprKind::Binary(
                            BinaryOp::Add,
                            Box::new(ExprKind::Binary(
                                BinaryOp::Sub,
                                Box::new(ExprKind::Binary(
                                    BinaryOp::Mult,
                                    Box::new(ExprKind::Identifier("real".to_string(), None)),
                                    Box::new(ExprKind::Identifier("real".to_string(), None)),
                                )),
                                Box::new(ExprKind::Binary(
                                    BinaryOp::Mult,
                                    Box::new(ExprKind::Identifier("imag".to_string(), None)),
                                    Box::new(ExprKind::Identifier("imag".to_string(), None)),
                                )),
                            )),
                            Box::new(ExprKind::Identifier("creal".to_string(), None)),
                        )),
                        Box::new(ExprKind::Binary(
                            BinaryOp::Add,
                            Box::new(ExprKind::Binary(
                                BinaryOp::Mult,
                                Box::new(ExprKind::Binary(
                                    BinaryOp::Mult,
                                    Box::new(ExprKind::Literal(Literal::F64(2.0))),
                                    Box::new(ExprKind::Identifier("real".to_string(), None)),
                                )),
                                Box::new(ExprKind::Identifier("imag".to_string(), None)),
                            )),
                            Box::new(ExprKind::Identifier("cimag".to_string(), None)),
                        )),
                        Box::new(ExprKind::Binary(
                            BinaryOp::Add,
                            Box::new(ExprKind::Identifier("iters".to_string(), None)),
                            Box::new(ExprKind::Literal(Literal::F64(1.0))),
                        )),
                        Box::new(ExprKind::Identifier("creal".to_string(), None)),
                        Box::new(ExprKind::Identifier("cimag".to_string(), None)),
                    ],
                )),
            )),
        ),
        DeclKind::Function(
            Prototype::new(
                "mandelconverge".to_string(),
                vec![
                    Arg::new("real".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("imag".to_string(), Box::new(TypeKind::F64)),
                ],
                Some(Box::new(TypeKind::F64)),
            ),
            Box::new(ExprKind::Call(
                "mandelconverger".to_string(),
                vec![
                    Box::new(ExprKind::Identifier("real".to_string(), None)),
                    Box::new(ExprKind::Identifier("imag".to_string(), None)),
                    Box::new(ExprKind::Literal(Literal::F64(0.0))),
                    Box::new(ExprKind::Identifier("real".to_string(), None)),
                    Box::new(ExprKind::Identifier("imag".to_string(), None)),
                ],
            )),
        ),
        DeclKind::Function(
            Prototype::new(
                "mandelhelp".to_string(),
                vec![
                    Arg::new("xmin".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("xmax".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("xstep".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("ymin".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("ymax".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("ystep".to_string(), Box::new(TypeKind::F64)),
                ],
                Some(Box::new(TypeKind::F64)),
            ),
            Box::new(ExprKind::ForLoop(
                "y".to_string(),
                Box::new(ExprKind::Identifier("ymin".to_string(), None)),
                Box::new(ExprKind::Binary(
                    BinaryOp::Lt,
                    Box::new(ExprKind::Identifier("y".to_string(), None)),
                    Box::new(ExprKind::Identifier("ymax".to_string(), None)),
                )),
                Some(Box::new(ExprKind::Identifier("ystep".to_string(), None))),
                Box::new(ExprKind::Binary(
                    BinaryOp::UserDefined(':'),
                    Box::new(ExprKind::ForLoop(
                        "x".to_string(),
                        Box::new(ExprKind::Identifier("xmin".to_string(), None)),
                        Box::new(ExprKind::Binary(
                            BinaryOp::Lt,
                            Box::new(ExprKind::Identifier("x".to_string(), None)),
                            Box::new(ExprKind::Identifier("xmax".to_string(), None)),
                        )),
                        Some(Box::new(ExprKind::Identifier("xstep".to_string(), None))),
                        Box::new(ExprKind::Call(
                            "printdensity".to_string(),
                            vec![Box::new(ExprKind::Call(
                                "mandelconverge".to_string(),
                                vec![
                                    Box::new(ExprKind::Identifier("x".to_string(), None)),
                                    Box::new(ExprKind::Identifier("y".to_string(), None)),
                                ],
                            ))],
                        )),
                    )),
                    Box::new(ExprKind::Call(
                        "putchard".to_string(),
                        vec![Box::new(ExprKind::Literal(Literal::F64(10.0)))],
                    )),
                )),
            )),
        ),
        DeclKind::Function(
            Prototype::new(
                "mandel".to_string(),
                vec![
                    Arg::new("realstart".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("imagstart".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("realmag".to_string(), Box::new(TypeKind::F64)),
                    Arg::new("imagmag".to_string(), Box::new(TypeKind::F64)),
                ],
                Some(Box::new(TypeKind::F64)),
            ),
            Box::new(ExprKind::Call(
                "mandelhelp".to_string(),
                vec![
                    Box::new(ExprKind::Identifier("realstart".to_string(), None)),
                    Box::new(ExprKind::Binary(
                        BinaryOp::Add,
                        Box::new(ExprKind::Identifier("realstart".to_string(), None)),
                        Box::new(ExprKind::Binary(
                            BinaryOp::Mult,
                            Box::new(ExprKind::Identifier("realmag".to_string(), None)),
                            Box::new(ExprKind::Literal(Literal::F64(78.0))),
                        )),
                    )),
                    Box::new(ExprKind::Identifier("realmag".to_string(), None)),
                    Box::new(ExprKind::Identifier("imagstart".to_string(), None)),
                    Box::new(ExprKind::Binary(
                        BinaryOp::Add,
                        Box::new(ExprKind::Identifier("imagstart".to_string(), None)),
                        Box::new(ExprKind::Binary(
                            BinaryOp::Mult,
                            Box::new(ExprKind::Identifier("imagmag".to_string(), None)),
                            Box::new(ExprKind::Literal(Literal::F64(40.0))),
                        )),
                    )),
                    Box::new(ExprKind::Identifier("imagmag".to_string(), None)),
                ],
            )),
        ),
        DeclKind::Function(
            Prototype::new(
                "unary-".to_string(),
                vec![Arg::new("v".to_string(), Box::new(TypeKind::F64))],
                Some(Box::new(TypeKind::F64)),
            ),
            Box::new(ExprKind::Binary(
                BinaryOp::Sub,
                Box::new(ExprKind::Literal(Literal::F64(0.0))),
                Box::new(ExprKind::Identifier("v".to_string(), None)),
            )),
        ),
        DeclKind::Function(
            Prototype::new("__anon_expr".to_string(), vec![], None),
            Box::new(ExprKind::Call(
                "mandel".to_string(),
                vec![
                    Box::new(ExprKind::Unary(
                        UnaryOp::UserDefined('-'),
                        Box::new(ExprKind::Literal(Literal::F64(2.3))),
                    )),
                    Box::new(ExprKind::Unary(
                        UnaryOp::UserDefined('-'),
                        Box::new(ExprKind::Literal(Literal::F64(1.3))),
                    )),
                    Box::new(ExprKind::Literal(Literal::F64(0.05))),
                    Box::new(ExprKind::Literal(Literal::F64(0.07))),
                ],
            )),
        ),
    ];
    assert_ast(&ast, &expected_ast);
}
