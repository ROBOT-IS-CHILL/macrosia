use crate::{MacroError, Number, VariableRegistry, intern::InternerEntry};
use num_complex::Complex64;

const DEPTH_LIMIT: usize = 128;
const STEP_LIMIT: usize = 1024 * 1024 * 8;
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
}

impl Operator {
    const fn argument_count(&self) -> u32 {
        use Operator::*;
        match self {
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
pub struct ExpressionFunction {
    nodes: Option<StackEntry>,
    arg_count: u32,
}

#[derive(Debug, Clone)]
enum StackEntry {
    FuncCall {
        name: InternerEntry,
        args: Vec<StackEntry>,
    },
    Operation {
        operator: Operator,
        args: Vec<StackEntry>,
    },
    Argument {
        index: u32,
        arg_count: u32,
    },
    Literal(Number),
}
impl StackEntry {
    fn eval(
        &self,
        var: &VariableRegistry,
        args: &[StackEntry],
        depth: usize,
        name: InternerEntry,
        steps: &mut usize,
    ) -> Result<Number, MacroError> {
        if depth > DEPTH_LIMIT {
            return Err(format!(
                "in {}: function call depth limit of {DEPTH_LIMIT} exceeded (after {steps} steps)",
                name
            ))?;
        }
        if *steps > STEP_LIMIT {
            return Err(format!(
                "in {}: step limit of {STEP_LIMIT} exceeded",
                name
            ))?;
        }
        *steps += 1;
        match self {
            Self::Argument { index, arg_count } => {
                if arg_count
                    .checked_sub(*index)
                    .is_some_and(|idx| args.len() as u32 <= idx)
                {
                    return Err(format!(
                        "in {}: function takes {} arguments, {} given",
                        name,
                        arg_count,
                        args.len()
                    ))?;
                }
                let entry = args.get((arg_count - index) as usize).ok_or_else(|| {
                    format!(
                        "in {}: arg index {} out of bounds",
                        name,
                        index
                    )
                })?;
                match entry {
                    Self::Literal(n) => Ok(*n),
                    other => stacker::maybe_grow(RED_ZONE, STACK_SIZE, || {
                        other.eval(var, args, depth + 1, name, steps)
                    }),
                }
            }
            Self::Operation {
                operator,
                args: vals,
            } => {
                let mut vals = vals.clone();
                stacker::maybe_grow(RED_ZONE, STACK_SIZE, || {
                    operator.eval(&mut vals, args, var, depth + 1, name, steps)
                })
            }
            Self::FuncCall {
                name: child,
                args: cargs,
            } => {
                let func = var.load_fn(*child).ok_or_else(|| {
                    format!(
                        "in {}: function with name {} does not exist",
                        name,
                        child
                    )
                })?;

                let mut eval_args = Vec::with_capacity(cargs.len());
                for arg_tree in cargs.iter() {
                    let val = stacker::maybe_grow(RED_ZONE, STACK_SIZE, || {
                        arg_tree.eval(var, args, depth + 1, name, steps)
                    })?;
                    eval_args.push(StackEntry::Literal(val));
                }
                stacker::maybe_grow(RED_ZONE, STACK_SIZE, || {
                    func.nodes
                        .as_ref()
                        .ok_or_else(|| {
                            format!(
                                "in {}: tried to call partially defined function {}",
                                name,
                                child
                            )
                        })?
                        .eval(var, &eval_args, depth + 1, *child, steps)
                })
            }
            Self::Literal(num) => Ok(*num),
        }
    }
}

impl ExpressionFunction {
    pub fn parse(string: &[u8], reg: &VariableRegistry, name: InternerEntry) -> Result<Self, MacroError> {
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

    pub fn forward(arg_count: u32) -> Self {
        Self {
            arg_count,
            nodes: None,
        }
    }

    #[inline]
    pub(crate) fn exec(
        &self,
        args: &[Number],
        reg: &VariableRegistry,
        name: InternerEntry,
    ) -> Result<Number, MacroError> {
        self.nodes
            .as_ref()
            .ok_or_else(|| {
                format!(
                    "in {}: function is only partially defined",
                    name
                )
            })?
            .eval(
                reg,
                &args
                    .iter()
                    .map(|v| StackEntry::Literal(*v))
                    .collect::<Vec<_>>(),
                0,
                name,
                &mut 0,
            )
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
                    stack.push(StackEntry::Operation {
                        operator: opr,
                        args,
                    });
                }
                Node::Input(number) => {
                    stack.push(StackEntry::Argument {
                        index: number,
                        arg_count,
                    });
                }
                Node::Number(num) => {
                    stack.push(StackEntry::Literal(num));
                }
                Node::FuncCall(fun) => {
                    let func = reg.load_fn(fun).ok_or_else(|| {
                        format!(
                            "in {}: function with name {} does not exist",
                            name,
                            fun
                        )
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
                    stack.push(
                        StackEntry::FuncCall {
                            name: fun,
                            args,
                        }
                        .into(),
                    );
                }
            }
        }
        let res = stack
            .pop()
            .ok_or_else(|| format!("in {}: stack empty", name))?;
        Ok(Self {
            nodes: Some(res),
            arg_count,
        })
    }
}

impl Operator {
    fn eval(
        &self,
        stack: &mut Vec<StackEntry>,
        args: &[StackEntry],
        var: &VariableRegistry,
        depth: usize,
        name: InternerEntry,
        steps: &mut usize,
    ) -> Result<Number, MacroError> {
        use self::*;
        let stack_len = stack.len();
        macro_rules! spop {
            () => {
                stack
                    .pop()
                    .ok_or_else(|| {
                        format!(
                            "in {}: operator does not have enough arguments (expected {}, got {})",
                            name,
                            self.argument_count(),
                            stack_len
                        )
                    })?
                    .eval(var, args, depth + 1, name, steps)?
            };
            (lazy) => {
                stack.pop().ok_or_else(|| {
                    format!(
                        "in {}: operator does not have enough arguments (expected {}, got {})",
                        name,
                        self.argument_count(),
                        stack_len
                    )
                })?
            };
        }
        Ok(match self {
            Self::Add => spop!() + spop!(),
            Self::Sub => {
                let [b, a] = [spop!(), spop!()];
                a - b
            }
            Self::Mul => spop!() * spop!(),
            Self::Div => {
                let [b, a] = [spop!(), spop!()];
                a / b
            }
            Self::Mod => {
                let [b, a] = [spop!(), spop!()];
                a % b
            }
            Self::Neg => -spop!(),
            Self::Less => {
                let [b, a] = [spop!(), spop!()];
                Number::Integer((a < b) as i64)
            }
            Self::Leq => {
                let [b, a] = [spop!(), spop!()];
                Number::Integer((a <= b) as i64)
            }
            Self::Great => {
                let [b, a] = [spop!(), spop!()];
                Number::Integer((a > b) as i64)
            }
            Self::Geq => {
                let [b, a] = [spop!(), spop!()];
                Number::Integer((a >= b) as i64)
            }
            Self::Eq => Number::Integer((spop!() == spop!()) as i64),
            Self::Neq => Number::Integer((spop!() != spop!()) as i64),
            Self::Cmp => {
                let [b, a] = [spop!(), spop!()];
                Number::Float(
                    // Ordering::Less => -1, Ordering::Equal => 0, Ordering::Greater => 1
                    // Thanks, rust stdlib
                    (a.partial_cmp(&b)).map_or(f64::NAN, |c| c as i8 as f64),
                )
            }
            Self::Tern => {
                let falsy = spop!(lazy);
                let truthy = spop!(lazy);
                let cond = spop!();
                if cond == Number::ZERO {
                    falsy.eval(var, args, depth + 1, name, steps)?
                } else {
                    truthy.eval(var, args, depth + 1, name, steps)?
                }
            }
            Self::And => Number::Integer(i64::from(spop!()) & i64::from(spop!())),
            Self::Or => Number::Integer(i64::from(spop!()) | i64::from(spop!())),
            Self::LogicAnd => {
                let left = spop!();
                let right = spop!(lazy);
                if left == Number::ZERO {
                    Number::Integer(0)
                } else {
                    Number::Integer(
                        if right.eval(var, args, depth + 1, name, steps)? == Number::ZERO {
                            0
                        } else {
                            1
                        },
                    )
                }
            }
            Self::LogicOr => {
                let left = spop!();
                let right = spop!(lazy);
                if left == Number::ZERO {
                    Number::Integer(
                        if right.eval(var, args, depth + 1, name, steps)? == Number::ZERO {
                            0
                        } else {
                            1
                        },
                    )
                } else {
                    Number::Integer(1)
                }
            }
            Self::Xor => Number::Integer(i64::from(spop!()) ^ i64::from(spop!())),
            Self::Not => Number::Integer(!i64::from(spop!())),
            Self::Shl => {
                let [b, a] = [spop!(), spop!()];
                Number::Integer(i64::from(a) << ((i64::from(b) as u64) % 64))
            }
            Self::Shr => {
                let [b, a] = [spop!(), spop!()];
                Number::Integer(((i64::from(a) as u64) >> (i64::from(b) as u64 % 64)) as i64)
            }
            Self::AShr => {
                let [b, a] = [spop!(), spop!()];
                Number::Integer(i64::from(a) >> ((i64::from(b) as u64) % 64))
            }
            Self::Pow => {
                let [b, a] = [spop!(), spop!()];
                a.pow(b)
            }
            Self::Log => {
                let [b, a] = [spop!(), spop!()];
                a.log(b)
            }
            Self::Abs => match spop!() {
                Number::Complex(c) => Number::Float(c.norm()),
                other => {
                    if other < Number::ZERO {
                        -other
                    } else {
                        other
                    }
                }
            },
            Self::Sin => match spop!() {
                Number::Integer(i) => Number::Float((i as f64).sin()),
                Number::Float(f) => Number::Float(f.sin()),
                Number::Complex(c) => Number::Complex(c.sin()),
            },
            Self::Cos => match spop!() {
                Number::Integer(i) => Number::Float((i as f64).cos()),
                Number::Float(f) => Number::Float(f.cos()),
                Number::Complex(c) => Number::Complex(c.cos()),
            },
            Self::Tan => match spop!() {
                Number::Integer(i) => Number::Float((i as f64).tan()),
                Number::Float(f) => Number::Float(f.tan()),
                Number::Complex(c) => Number::Complex(c.tan()),
            },
            Self::Asin => match spop!() {
                Number::Integer(i) => Number::Float((i as f64).asin()),
                Number::Float(f) => Number::Float(f.asin()),
                Number::Complex(c) => Number::Complex(c.asin()),
            },
            Self::Acos => match spop!() {
                Number::Integer(i) => Number::Float((i as f64).acos()),
                Number::Float(f) => Number::Float(f.acos()),
                Number::Complex(c) => Number::Complex(c.acos()),
            },
            Self::Atan => match spop!() {
                Number::Integer(i) => Number::Float((i as f64).atan()),
                Number::Float(f) => Number::Float(f.atan()),
                Number::Complex(c) => Number::Complex(c.atan()),
            },
            Self::Real => match spop!() {
                Number::Complex(c) => Number::Float(c.re),
                other => other,
            },
            Self::Imag => match spop!() {
                Number::Complex(c) => Number::Float(c.im),
                _other => Number::ZERO,
            },
            Self::Arg => Number::Float(Complex64::from(spop!()).arg()),
        })
    }
}

#[test]
fn nodetest() -> Result<(), Box<dyn std::error::Error>> {
    let mut reg = VariableRegistry::new();
    let entry = InternerEntry::get_or_intern(b"<test>");

    let func = ExpressionFunction::parse(b"1 2 + 3 *", &reg, entry)?;
    assert_eq!(
        Number::Integer(9),
        func.exec(&[], &VariableRegistry::new(), entry)?
    );
    let func = ExpressionFunction::parse(b"1 16 0.5 ** /", &reg, entry)?;
    assert_eq!(
        Number::Float(0.25),
        func.exec(&[], &VariableRegistry::new(), entry)?
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
    }
    .map_err(|v| panic!("{v}"))?;
    println!("{}", String::from_utf8_lossy(&out));
    Ok(())
}
