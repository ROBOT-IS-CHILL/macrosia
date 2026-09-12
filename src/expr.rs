use std::collections::VecDeque;

use crate::{MacroError, Number, VariableRegistry, intern::InternerEntry};
use num_complex::Complex64;

const STEP_LIMIT: usize = 256 * 1024;
const DEPTH_LIMIT: usize = 1024;
const RED_ZONE: usize = 16 * 1024;
const STACK_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone)]
enum Node {
    Operator(Operator),
    Input(u32),
    Number(Number),
    FuncCall(InternerEntry),
}

#[derive(Debug, Copy, Clone)]
enum Operator {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,
    Less,
    Leq,
    Great,
    Geq,
    Eq,
    Neq,
    Cmp,
    Tern,
    LogicAnd,
    LogicOr,
    And,
    Or,
    Xor,
    Not,
    Shl,
    Shr,
    AShr,
    Pow,
    Log,
    Abs,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Real,
    Imag,
    Arg,
    Copy,
    Swap,
    Rotate,
    StackLength,
}

#[derive(Debug, Clone)]
pub struct ExprAST {
    nodes: ASTNode,
    arg_count: u32,
    name: InternerEntry,
}

#[derive(Debug, Clone)]
enum Step {
    Operator(Operator),
    Input(u32),
    Number(Number),
    FuncCall(InternerEntry),
    Branch(usize),
    Jump(usize),
}

impl Operator {
    const fn argument_count(&self) -> u32 {
        use Operator::*;
        match self {
            Copy | Swap | Rotate | StackLength => 0,
            Add | Sub | Mul | Div | Mod | Less | Leq | Great | Geq | Eq | Neq | Cmp | LogicAnd
            | LogicOr | And | Or | Xor | Shl | Shr | AShr | Pow | Log => 2,
            Tern => 3,
            _ => 1,
        }
    }
    fn try_parse(string: &[u8]) -> Option<Self> {
        use Operator::*;
        Some(match string {
            b"**" => Pow,
            b"log" => Log,
            b"abs" => Abs,
            b"<=>" => Cmp,
            b"!=" => Neq,
            b"==" => Eq,
            b"<=" => Leq,
            b">=" => Geq,
            b"<<" => Shl,
            b">>" => AShr,
            b">>>" => Shr,
            b"<" => Less,
            b">" => Great,
            b"+" => Add,
            b"-" => Sub,
            b"*" => Mul,
            b"/" => Div,
            b"%" => Mod,
            b"~" => Neg,
            b"?" => Tern,
            b"&&" => LogicAnd,
            b"||" => LogicOr,
            b"&" => And,
            b"|" => Or,
            b"^" => Xor,
            b"!" => Not,
            b"sin" => Sin,
            b"cos" => Cos,
            b"tan" => Tan,
            b"asin" => Asin,
            b"acos" => Acos,
            b"atan" => Atan,
            b"real" => Real,
            b"imag" => Imag,
            b"arg" => Arg,
            b"->" => Copy,
            b"<>" => Swap,
            b"<>>" => Rotate,
            b"@" => StackLength,
            _ => return None,
        })
    }
}

impl Node {
    fn try_parse(mut string: &[u8]) -> Option<Self> {
        if let Some(s) = string.strip_prefix(b"$") {
            string = s;
            if !string.iter().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let num = str::from_utf8(&string).ok()?.parse::<u32>().ok()?;
            return Some(Self::Input(num));
        }
        if let Ok(r) = Number::try_from(string) {
            return Some(Node::Number(r));
        }
        if let Some(s) = string.strip_prefix(b"#") {
            string = s;
            if !string
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                return None;
            }
            let ident = str::from_utf8(string).ok()?;
            return Some(Node::FuncCall(ident.into()));
        }
        Operator::try_parse(string).map(Node::Operator)
    }
}

#[derive(Debug, Clone)]
enum ASTNode {
    FuncCall {
        name: InternerEntry,
        args: Vec<ASTNode>,
    },
    Operation {
        operator: Operator,
        args: Vec<ASTNode>,
    },
    Argument(u32),
    Literal(Number),
}

impl ExprAST {
    pub fn parse(
        string: &[u8],
        reg: &VariableRegistry,
        name: InternerEntry,
    ) -> Result<Self, MacroError> {
        let nodes = string
            .split(|b| b.is_ascii_whitespace())
            .filter(|s| s.len() != 0)
            .map(Node::try_parse);
        let mut node_vec = Vec::new();
        let mut arg_count = 0;
        for (i, node) in nodes.enumerate() {
            let Some(node) = node else {
                return Err(format!("failed to parse node {i}"))?;
            };
            if let Node::Input(i) = node {
                if i == 0 {
                    return Err("input with index 0 is not allowed in expression functions")?;
                }
                arg_count = arg_count.max(i);
            }
            node_vec.push(node);
        }
        if node_vec.is_empty() {
            return Err("empty expression")?;
        }
        Self::construct_ast(node_vec, arg_count, reg, name)
    }

    fn construct_ast(
        nodes: Vec<Node>,
        arg_count: u32,
        reg: &VariableRegistry,
        name: InternerEntry,
    ) -> Result<Self, MacroError> {
        let mut stack = Vec::new();
        for node in nodes {
            match node {
                Node::Operator(opr) => {
                    if stack.len() < opr.argument_count() as usize {
                        return Err(format!(
                            "in {}: operator {opr:?} not given enough arguments (expected {})",
                            name,
                            opr.argument_count()
                        ))?;
                    }
                    let args = stack.split_off(stack.len() - (opr.argument_count() as usize));
                    stack.push(ASTNode::Operation {
                        operator: opr,
                        args,
                    });
                }
                Node::Input(number) => {
                    stack.push(ASTNode::Argument(number));
                }
                Node::Number(num) => {
                    stack.push(ASTNode::Literal(num));
                }
                Node::FuncCall(fun) => {
                    let func = reg.load_fn(fun).ok_or_else(|| {
                        format!("in {}: function with name {} does not exist", name, fun)
                    })?;
                    if stack.len() < func.arg_count as usize {
                        return Err(format!(
                            "in {}: function {} takes {} arguments, {} given",
                            name,
                            fun,
                            func.arg_count,
                            stack.len()
                        ))?;
                    }
                    let args = stack
                        .split_off(stack.len() - (func.arg_count as usize))
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>();
                    stack.push(ASTNode::FuncCall { name: fun, args }.into());
                }
            }
        }
        let res = stack
            .pop()
            .ok_or_else(|| format!("in {}: stack empty", name))?;
        Ok(Self {
            nodes: res,
            arg_count,
            name,
        })
    }
}

impl ASTNode {
    pub(crate) fn to_bytecode(&self) -> Vec<Step> {
        match self {
            ASTNode::Operation {
                operator: Operator::LogicAnd,
                args,
            } => {
                let left = &args[0];
                let right = &args[1];
                let right_code = right.to_bytecode();
                let right_len = right_code.len();
                let mut res = left.to_bytecode();
                res.push(Step::Branch(2));
                res.push(Step::Number(Number::Integer(0)));
                res.push(Step::Jump(right_len));
                res.extend(right_code);
                res
            }
            ASTNode::Operation {
                operator: Operator::LogicOr,
                args,
            } => {
                let left = &args[0];
                let right = &args[1];
                let right_code = right.to_bytecode();
                let right_len = right_code.len();
                let mut res = left.to_bytecode();
                res.push(Step::Branch(right_len + 1));
                res.extend(right_code);
                res.push(Step::Jump(1));
                res.push(Step::Number(Number::Integer(1)));
                res
            }
            ASTNode::Operation {
                operator: Operator::Tern,
                args,
            } => {
                let cond = &args[0];
                let left = &args[1];
                let right = &args[2];
                let left_code = left.to_bytecode();
                let left_len = left_code.len();
                let right_code = right.to_bytecode();
                let right_len = right_code.len();
                let mut res = cond.to_bytecode();
                res.push(Step::Branch(right_len + 1));
                res.extend(right_code);
                res.push(Step::Jump(left_len));
                res.extend(left_code);
                res
            }
            ASTNode::FuncCall { name, args } => args
                .iter()
                .flat_map(ASTNode::to_bytecode)
                .chain(std::iter::once(Step::FuncCall(*name)))
                .collect(),
            ASTNode::Operation { operator, args } => args
                .iter()
                .flat_map(ASTNode::to_bytecode)
                .chain(std::iter::once(Step::Operator(*operator)))
                .collect(),
            ASTNode::Argument(index) => vec![Step::Input(*index)],
            ASTNode::Literal(number) => vec![Step::Number(*number)],
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompiledExpr {
    arg_count: u32,
    bytecode: Vec<Step>,
    name: InternerEntry,
}

impl CompiledExpr {
    pub const fn forward(arg_count: u32, name: InternerEntry) -> Self {
        Self {
            arg_count,
            bytecode: Vec::new(),
            name,
        }
    }

    pub fn execute(
        &self,
        inputs: Vec<Number>,
        var_reg: &VariableRegistry,
    ) -> Result<Number, MacroError> {
        self._execute(inputs, var_reg, 0, &mut 0)
    }
    
    fn _execute(
        &self,
        inputs: Vec<Number>,
        var_reg: &VariableRegistry,
        depth: usize,
        steps: &mut usize
    ) -> Result<Number, MacroError> {
        if depth > DEPTH_LIMIT {
            return Err(format!("in {}: exceeded depth limit", self.name))?;
        }
        if self.bytecode.len() == 0 {
            return Err(format!("function {} is not ready yet", self.name))?;
        }
        let mut bytecode = VecDeque::from(self.bytecode.clone());
        let mut stack = Vec::<Number>::new();
        macro_rules! pop {
            () => {
                stack
                    .pop()
                    .ok_or_else(|| MacroError::from(format!("in {}: stack exhausted", self.name)))
            };
        }
        loop {
            let Some(step) = bytecode.pop_front() else {
                return pop!();
            };
            *steps += 1;
            if *steps > STEP_LIMIT {
                return Err(format!("in {}: exceeded step limit", self.name))?;
            }
            match step {
                Step::Input(i) => stack.push(inputs.get((i - 1) as usize).copied().ok_or_else(|| {
                    format!("in {}: tried to get nonexistent argument {i}", self.name)
                })?),
                Step::Number(number) => stack.push(number),
                Step::FuncCall(interner_entry) => {
                    let func = var_reg.load_fn(interner_entry).ok_or_else(|| {
                        format!("in {}: function {interner_entry} does not exist", self.name)
                    })?;
                    let args = stack
                        .drain((stack.len() - func.arg_count as usize)..)
                        .collect::<Vec<_>>();
                    stack.push(stacker::maybe_grow(RED_ZONE, STACK_SIZE, || {
                        func._execute(args, var_reg, depth + 1, steps)
                    })?);
                }
                Step::Branch(amount) => {
                    let val = pop!()?;
                    if val != Number::ZERO {
                        bytecode.truncate_to_range(amount..);
                    }
                }
                Step::Jump(amount) => bytecode.truncate_to_range(amount..),
                Step::Operator(Operator::Copy) => {
                    let x = pop!()?;
                    stack.push(x);
                    stack.push(x);
                }
                Step::Operator(Operator::Swap) => {
                    let x = pop!()?;
                    let y = pop!()?;
                    stack.push(x);
                    stack.push(y);
                }
                Step::Operator(Operator::Rotate) => {
                    let x = pop!()?;
                    let y = pop!()?;
                    let z = pop!()?;
                    stack.push(x);
                    stack.push(z);
                    stack.push(y);
                }
                Step::Operator(opr) => {
                    let val = match opr {
                        Operator::Add => pop!()? + pop!()?,
                        Operator::Sub => {
                            let [b, a] = [pop!()?, pop!()?];
                            a - b
                        }
                        Operator::Mul => pop!()? * pop!()?,
                        Operator::Div => {
                            let [b, a] = [pop!()?, pop!()?];
                            a / b
                        }
                        Operator::Mod => {
                            let [b, a] = [pop!()?, pop!()?];
                            a % b
                        }
                        Operator::Neg => -pop!()?,
                        Operator::Less => {
                            let [b, a] = [pop!()?, pop!()?];
                            Number::Integer((a < b) as i64)
                        }
                        Operator::Leq => {
                            let [b, a] = [pop!()?, pop!()?];
                            Number::Integer((a <= b) as i64)
                        }
                        Operator::Great => {
                            let [b, a] = [pop!()?, pop!()?];
                            Number::Integer((a > b) as i64)
                        }
                        Operator::Geq => {
                            let [b, a] = [pop!()?, pop!()?];
                            Number::Integer((a >= b) as i64)
                        }
                        Operator::Eq => Number::Integer((pop!()? == pop!()?) as i64),
                        Operator::Neq => Number::Integer((pop!()? != pop!()?) as i64),
                        Operator::Cmp => {
                            let [b, a] = [pop!()?, pop!()?];
                            Number::Float(
                                // Ordering::Less => -1, Ordering::Equal => 0, Ordering::Greater => 1
                                // Thanks, rust stdlib
                                (a.partial_cmp(&b)).map_or(f64::NAN, |c| c as i8 as f64),
                            )
                        }
                        Operator::And => Number::Integer(i64::from(pop!()?) & i64::from(pop!()?)),
                        Operator::Or => Number::Integer(i64::from(pop!()?) | i64::from(pop!()?)),
                        Operator::Xor => Number::Integer(i64::from(pop!()?) ^ i64::from(pop!()?)),
                        Operator::Not => Number::Integer(!i64::from(pop!()?)),
                        Operator::Shl => {
                            let [b, a] = [pop!()?, pop!()?];
                            Number::Integer(i64::from(a) << ((i64::from(b) as u64) % 64))
                        }
                        Operator::Shr => {
                            let [b, a] = [pop!()?, pop!()?];
                            Number::Integer(
                                ((i64::from(a) as u64) >> (i64::from(b) as u64 % 64)) as i64,
                            )
                        }
                        Operator::AShr => {
                            let [b, a] = [pop!()?, pop!()?];
                            Number::Integer(i64::from(a) >> ((i64::from(b) as u64) % 64))
                        }
                        Operator::Pow => {
                            let [b, a] = [pop!()?, pop!()?];
                            a.pow(b)
                        }
                        Operator::Log => {
                            let [b, a] = [pop!()?, pop!()?];
                            a.log(b)
                        }
                        Operator::Abs => match pop!()? {
                            Number::Complex(c) => Number::Float(c.norm()),
                            other => {
                                if other < Number::ZERO {
                                    -other
                                } else {
                                    other
                                }
                            }
                        },
                        Operator::Sin => match pop!()? {
                            Number::Integer(i) => Number::Float((i as f64).sin()),
                            Number::Float(f) => Number::Float(f.sin()),
                            Number::Complex(c) => Number::Complex(c.sin()),
                        },
                        Operator::Cos => match pop!()? {
                            Number::Integer(i) => Number::Float((i as f64).cos()),
                            Number::Float(f) => Number::Float(f.cos()),
                            Number::Complex(c) => Number::Complex(c.cos()),
                        },
                        Operator::Tan => match pop!()? {
                            Number::Integer(i) => Number::Float((i as f64).tan()),
                            Number::Float(f) => Number::Float(f.tan()),
                            Number::Complex(c) => Number::Complex(c.tan()),
                        },
                        Operator::Asin => match pop!()? {
                            Number::Integer(i) => Number::Float((i as f64).asin()),
                            Number::Float(f) => Number::Float(f.asin()),
                            Number::Complex(c) => Number::Complex(c.asin()),
                        },
                        Operator::Acos => match pop!()? {
                            Number::Integer(i) => Number::Float((i as f64).acos()),
                            Number::Float(f) => Number::Float(f.acos()),
                            Number::Complex(c) => Number::Complex(c.acos()),
                        },
                        Operator::Atan => match pop!()? {
                            Number::Integer(i) => Number::Float((i as f64).atan()),
                            Number::Float(f) => Number::Float(f.atan()),
                            Number::Complex(c) => Number::Complex(c.atan()),
                        },
                        Operator::Real => match pop!()? {
                            Number::Complex(c) => Number::Float(c.re),
                            other => other,
                        },
                        Operator::Imag => match pop!()? {
                            Number::Complex(c) => Number::Float(c.im),
                            _other => Number::ZERO,
                        },
                        Operator::Arg => Number::Float(Complex64::from(pop!()?).arg()),
                        Operator::StackLength => Number::Integer(stack.len() as i64),
                        Operator::Copy | Operator::Swap | Operator::Rotate => {
                            unreachable!("stack operands should have already been handled")
                        }
                        Operator::Tern | Operator::LogicAnd | Operator::LogicOr => {
                            unreachable!("lazy operands should have already been handled")
                        }
                    };
                    stack.push(val);
                }
            }
        }
    }
}

impl ExprAST {
    pub fn compile(self) -> CompiledExpr {
        CompiledExpr {
            arg_count: self.arg_count,
            bytecode: self.nodes.to_bytecode(),
            name: self.name,
        }
    }
}

#[test]
fn nodetest() -> Result<(), Box<dyn std::error::Error>> {
    let reg = VariableRegistry::new();
    let entry = InternerEntry::get_or_intern(b"<test>");

    let func = ExprAST::parse(b"1 2 + 3 *", &reg, entry)?.compile();
    assert_eq!(
        Number::Integer(9),
        func.execute(vec![], &VariableRegistry::new())?
    );
    let func = ExprAST::parse(b"1 16 0.5 ** /", &reg, entry)?.compile();
    assert_eq!(
        Number::Float(0.25),
        func.execute(vec![], &VariableRegistry::new())?
    );

    Ok(())
}

#[test]
fn rectest() -> Result<(), Box<dyn std::error::Error>> {
    let string = b"
        [expr.fwd/fib/1]
        [expr.def/fib/$1 2 < $1 $1 1 - #fib $1 2 - #fib + ?]
        [expr.call/fib/2]
        [expr.call/fib/24]

        [expr.fwd/inf/0]
        [expr.def/inf/1 #inf +]
        [expr.def/lazy_test/1 1 > #inf 42 ?]
        [expr.call/lazy_test]
    ";

    static KILL_MACROS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    let mut exec = crate::Executor::new(b't');
    exec.add_stdlib();
    let mut reg = VariableRegistry::new();
    let mut func = exec.evaluate(string, &mut reg, None, None, &KILL_MACROS);
    let out = loop {
        if let Some(v) = func() {
            break v;
        }
    }.map_err(|v| panic!("{v}"))?;
    println!("{}", String::from_utf8_lossy(&out));
    Ok(())
}
