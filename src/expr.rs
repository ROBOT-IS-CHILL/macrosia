use crate::{number, Number, MacroError, VariableRegistry};
use num_complex::Complex64;

#[derive(Debug, Clone)]
enum Node {
	Operator(Operator),
	Input(u32),
	Number(Number),
	FuncCall(Vec<u8>)
}

#[derive(Debug, Copy, Clone)]
enum Operator {
	Add, Sub, Mul, Div, Mod, Neg,
	Less, Leq, Great, Geq, Eq, Neq, Cmp, Tern,
	And, Or, Xor, Not, Shl, Shr, AShr,
	Pow, Log, Abs,
	Sin, Cos, Tan, Asin, Acos, Atan,
	Real, Imag, Arg
}

impl Operator {
	const fn argument_count(&self) -> u32 {
		use Operator::*;
		match self {
			Add | Sub | Mul | Div | Mod |
			Less | Leq | Great | Geq | Eq | Neq | Cmp |
			And | Or | Xor | Shl | Shr | AShr |
			Pow | Log => 2,
			Tern => 3,
			_ => 1
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
			_ => return None
		})
	}
}

impl Node {
	fn try_parse(mut string: &[u8]) -> Option<Self> {
		if let Some(s) = string.strip_prefix(b"$") {
			string = s;
			if !string.iter().all(|c| c.is_ascii_digit()) { return None; }
			let num = str::from_utf8(&string).ok()?.parse::<u32>().ok()?;
			return Some(Self::Input(num));
		}
		if let Ok(r) = Number::try_from(string) {
			return Some(Node::Number(r));
		}
		if let Some(s) = string.strip_prefix(b"#") {
			string = s;
			if !string.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_') { return None; }
			let ident = str::from_utf8(string).ok()?;
			return Some(Node::FuncCall(ident.into()));
		} 
		Operator::try_parse(string).map(Node::Operator)
	}
}

pub struct ExpressionFunction {
	nodes: Vec<Node>,
	arg_count: u32
}

impl ExpressionFunction {
	pub fn parse(string: &[u8]) -> Result<Self, MacroError> {
		let nodes = string.split(|b| b.is_ascii_whitespace())
			.filter(|s| s.len() != 0)
			.map(Node::try_parse);
		let mut node_vec = Vec::new();
		let mut arg_count = 0;
		for (i, node) in nodes.enumerate() {
			let Some(node) = node else { return Err(format!("failed to parse node {i}"))?; };
			if let Node::Input(i) = node {
				if i == 0 { return Err("input with index 0 is not allowed in expression functions")?; }
				arg_count = arg_count.max(i);
			}
			node_vec.push(node);
		}
		if node_vec.is_empty() { return Err("empty expression")?; }
		Ok(Self { nodes: node_vec, arg_count })
	}

	#[inline]
	pub(crate) fn exec(&self, args: &[Number], reg: &VariableRegistry, name: &[u8]) -> Result<Number, MacroError> {
		let mut stack = Vec::new();
		self._exec(&mut stack, args.iter().map(|v| *v).rev().collect::<Vec<_>>().as_slice(), reg, name, 0)?;
		stack.pop().ok_or_else(|| format!("in {}: stack empty", String::from_utf8_lossy(name)).into())
	}

	fn _exec(&self, stack: &mut Vec<Number>, args: &[Number], reg: &VariableRegistry, name: &[u8], depth: u32) -> Result<(), MacroError> {
		const DEPTH_LIMIT: u32 = 1024;
		if depth > DEPTH_LIMIT {
			return Err(format!("in {}: function call depth limit of {DEPTH_LIMIT} exceeded", String::from_utf8_lossy(name)))?;
		}
		if self.arg_count != args.len() as u32 {
			return Err(format!("in {}: args.len() != self.arg_count (this should never happen)", String::from_utf8_lossy(name)))?;
		}
		for node in &self.nodes {
			match node {
				Node::Operator(opr) => {
					let res = opr.eval(stack).ok_or_else(|| format!("in {}: operator {opr:?} not given enough arguments (expected {})", String::from_utf8_lossy(name), opr.argument_count()))?;
					stack.push(res);
				},
				Node::Input(number) => {
					if *number > self.arg_count {
						return Err(format!("in {}: input node takes more than arg count (this should never happen)", String::from_utf8_lossy(name)))?;
					}
					let val = args.get((self.arg_count - number) as usize).ok_or_else(|| format!("in {}: function takes {} arguments, {} given", String::from_utf8_lossy(name), self.arg_count, args.len()))?;
					stack.push(*val);
				},
				Node::Number(num) => {
					stack.push(*num);
				},
				Node::FuncCall(fun) => {
					let func = reg.load_fn(&fun).ok_or_else(|| format!("in {}: function with name {} does not exist", String::from_utf8_lossy(name), String::from_utf8_lossy(fun)))?;
					if stack.len() < func.arg_count as usize {
						return Err(format!("in {}: function {} takes {} arguments, {} given", String::from_utf8_lossy(name), String::from_utf8_lossy(fun), func.arg_count, stack.len()))?;
					}
					let args = stack.split_off(stack.len() - (func.arg_count as usize)).into_iter().rev().collect::<Vec<_>>();
					func._exec(stack, &args, reg, fun, depth + 1)?;
				}
			}
		}
		Ok(())
	}
}

impl Operator {
	fn eval(&self, stack: &mut Vec<Number>) -> Option<Number> {
		use self::*;
		Some(match self {
			Self::Add => stack.pop()? + stack.pop()?,
			Self::Sub => {let [b, a] = [stack.pop()?, stack.pop()?]; a - b},
			Self::Mul => stack.pop()? * stack.pop()?,
			Self::Div => {let [b, a] = [stack.pop()?, stack.pop()?]; a / b},
			Self::Mod => {let [b, a] = [stack.pop()?, stack.pop()?]; a % b},
			Self::Neg => -stack.pop()?,
			Self::Less => { let [b, a] = [stack.pop()?, stack.pop()?]; Number::Integer((a < b) as i64) },
			Self::Leq => { let [b, a] = [stack.pop()?, stack.pop()?]; Number::Integer((a <= b) as i64) },
			Self::Great => { let [b, a] = [stack.pop()?, stack.pop()?]; Number::Integer((a > b) as i64) },
			Self::Geq => { let [b, a] = [stack.pop()?, stack.pop()?]; Number::Integer((a >= b) as i64) },
			Self::Eq => Number::Integer((stack.pop()? == stack.pop()?) as i64),
			Self::Neq => Number::Integer((stack.pop()? != stack.pop()?) as i64),
			Self::Cmp => {
				let [b, a] = [stack.pop()?, stack.pop()?];
				Number::Float(
					// Ordering::Less => -1, Ordering::Equal => 0, Ordering::Greater => 1
					// Thanks, rust stdlib
					(a.partial_cmp(&b)).map_or(f64::NAN, |c| c as i8 as f64)
				)
			},
			Self::Tern => {
				let falsy = stack.pop()?;
			    let truthy = stack.pop()?;
			    let cond = stack.pop()?;
				if cond == Number::ZERO { falsy } else { truthy }
			},
			Self::And => Number::Integer(i64::from(stack.pop()?) & i64::from(stack.pop()?)),
			Self::Or => Number::Integer(i64::from(stack.pop()?) | i64::from(stack.pop()?)),
			Self::Xor => Number::Integer(i64::from(stack.pop()?) ^ i64::from(stack.pop()?)),
			Self::Not => Number::Integer(!i64::from(stack.pop()?)),
			Self::Shl => {let [b, a] = [stack.pop()?, stack.pop()?]; Number::Integer(i64::from(a) << ((i64::from(b) as u64) % 64))},
			Self::Shr => {let [b, a] = [stack.pop()?, stack.pop()?]; Number::Integer(((i64::from(a) as u64) >> (i64::from(b) as u64 % 64)) as i64)},
			Self::AShr => {let [b, a] = [stack.pop()?, stack.pop()?]; Number::Integer(i64::from(a) >> ((i64::from(b) as u64) % 64))},
			Self::Pow => {let [b, a] = [stack.pop()?, stack.pop()?]; a.pow(b)},
			Self::Log => {let [b, a] = [stack.pop()?, stack.pop()?]; a.log(b)},
			Self::Abs => match stack.pop()? {
				Number::Complex(c) => Number::Float(c.norm()),
				other => if other < Number::ZERO {-other} else {other} 
			},
			Self::Sin => match stack.pop()? {
				Number::Integer(i) => Number::Float((i as f64).sin()),
				Number::Float(f) => Number::Float(f.sin()),
				Number::Complex(c) => Number::Complex(c.sin()),
			},
			Self::Cos => match stack.pop()? {
				Number::Integer(i) => Number::Float((i as f64).cos()),
				Number::Float(f) => Number::Float(f.cos()),
				Number::Complex(c) => Number::Complex(c.cos()),
			},
			Self::Tan => match stack.pop()? {
				Number::Integer(i) => Number::Float((i as f64).tan()),
				Number::Float(f) => Number::Float(f.tan()),
				Number::Complex(c) => Number::Complex(c.tan()),
			},
			Self::Asin => match stack.pop()? {
				Number::Integer(i) => Number::Float((i as f64).asin()),
				Number::Float(f) => Number::Float(f.asin()),
				Number::Complex(c) => Number::Complex(c.asin()),
			},
			Self::Acos => match stack.pop()? {
				Number::Integer(i) => Number::Float((i as f64).acos()),
				Number::Float(f) => Number::Float(f.acos()),
				Number::Complex(c) => Number::Complex(c.acos()),
			},
			Self::Atan => match stack.pop()? {
				Number::Integer(i) => Number::Float((i as f64).atan()),
				Number::Float(f) => Number::Float(f.atan()),
				Number::Complex(c) => Number::Complex(c.atan()),
			},
			Self::Real => match stack.pop()? {
				Number::Complex(c) => Number::Float(c.re),
				other => other
			},
			Self::Imag => match stack.pop()? {
				Number::Complex(c) => Number::Float(c.im),
				other => Number::ZERO
			},
			Self::Arg => Number::Float(Complex64::from(stack.pop()?).arg()),
		})
	}
}

#[test]
fn nodetest() -> Result<(), Box<dyn std::error::Error>> {
	let func = ExpressionFunction::parse(b"1 2 + 3 *")?;
	assert_eq!(Number::Integer(9), func.exec(&[], &VariableRegistry::new(), b"<test>")?);
	let func = ExpressionFunction::parse(b"1 16 0.5 ** /")?;
	assert_eq!(Number::Float(0.25), func.exec(&[], &VariableRegistry::new(), b"<test>")?);

	Ok(())
}