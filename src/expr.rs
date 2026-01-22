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

#[derive(Clone)]
enum StackEntry {
	FuncCall {
		name: Vec<u8>,
		args: Vec<StackEntry>
	},
	Operation {
		operator: Operator,
		args: Vec<StackEntry>
	},
	Literal(Number)
}
impl StackEntry {
	fn eval(self, var: &VariableRegistry, depth: usize, name: &[u8]) -> Result<Number, MacroError> {
		const DEPTH_LIMIT: usize = 1024;
		if depth > DEPTH_LIMIT {
			return Err(format!("in {}: function call depth limit of {DEPTH_LIMIT} exceeded", String::from_utf8_lossy(name)))?;
		}
		match self {
			Self::Operation { operator, mut args } => 
				operator.eval(&mut args, var, depth, name).ok_or_else(|| format!("in {}: operator has not enough args past check, should never happen", String::from_utf8_lossy(name)).into()),
			Self::FuncCall { name: child, args } => {
				let func = var.load_fn(&child).ok_or_else(|| format!("in {}: function with name {} does not exist", String::from_utf8_lossy(name), String::from_utf8_lossy(&child)))?;
				let mut stack = Vec::<StackEntry>::new();
				func._exec(&mut stack, &args, var, &child)?;
				stack.pop().ok_or_else(|| format!("in {}: stack was empty at end of function", String::from_utf8_lossy(name)))?
					.eval(var, depth + 1, &child)
			}
			Self::Literal(num) => Ok(num)
		}
	}
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
		self._exec(&mut stack, args.iter().map(|v| StackEntry::Literal(*v)).rev().collect::<Vec<_>>().as_slice(), reg, name)?;
		let res = stack.pop().ok_or_else(|| format!("in {}: stack empty", String::from_utf8_lossy(name)))?;
		res.eval(reg, 0, name)
	}

	fn _exec(&self, stack: &mut Vec<StackEntry>, args: &[StackEntry], reg: &VariableRegistry, name: &[u8]) -> Result<(), MacroError> {
		if self.arg_count != args.len() as u32 {
			return Err(format!("in {}: function takes {} arguments, {} given", String::from_utf8_lossy(name), self.arg_count, args.len()))?;
		}
		for node in &self.nodes {
			match node {
				Node::Operator(opr) => {
					if stack.len() < opr.argument_count() as usize {
						return Err(format!("in {}: operator {opr:?} not given enough arguments (expected {})", String::from_utf8_lossy(name), opr.argument_count()))?;
					}
					let args = stack.split_off(stack.len() - (opr.argument_count() as usize));
					stack.push(StackEntry::Operation { operator: *opr, args });
				},
				Node::Input(number) => {
					if *number > self.arg_count {
						return Err(format!("in {}: input node takes more than arg count (this should never happen)", String::from_utf8_lossy(name)))?;
					}
					let val = args.get((self.arg_count - number) as usize).ok_or_else(|| format!("in {}: function takes {} arguments, {} given", String::from_utf8_lossy(name), self.arg_count, args.len()))?;
					stack.push(val.clone());
				},
				Node::Number(num) => {
					stack.push(StackEntry::Literal(*num));
				},
				Node::FuncCall(fun) => {
					let func = reg.load_fn(&fun).ok_or_else(|| format!("in {}: function with name {} does not exist", String::from_utf8_lossy(name), String::from_utf8_lossy(fun)))?;
					if stack.len() < func.arg_count as usize {
						return Err(format!("in {}: function {} takes {} arguments, {} given", String::from_utf8_lossy(name), String::from_utf8_lossy(fun), func.arg_count, stack.len()))?;
					}
					let args = stack.split_off(stack.len() - (func.arg_count as usize)).into_iter().rev().collect::<Vec<_>>();
					stack.push(StackEntry::FuncCall { name: fun.to_vec(), args });
				}
			}
		}
		Ok(())
	}
}

impl Operator {
	fn eval(&self, stack: &mut Vec<StackEntry>, var: &VariableRegistry, depth: usize, name: &[u8]) -> Option<Number> {
		use self::*;
		macro_rules! spop {
			() => { stack.pop()?.eval(var, depth + 1, name).ok()? }
		}
		Some(match self {
			Self::Add => spop!() + spop!(),
			Self::Sub => {let [b, a] = [spop!(), spop!()]; a - b},
			Self::Mul => spop!() * spop!(),
			Self::Div => {let [b, a] = [spop!(), spop!()]; a / b},
			Self::Mod => {let [b, a] = [spop!(), spop!()]; a % b},
			Self::Neg => -spop!(),
			Self::Less => { let [b, a] = [spop!(), spop!()]; Number::Integer((a < b) as i64) },
			Self::Leq => { let [b, a] = [spop!(), spop!()]; Number::Integer((a <= b) as i64) },
			Self::Great => { let [b, a] = [spop!(), spop!()]; Number::Integer((a > b) as i64) },
			Self::Geq => { let [b, a] = [spop!(), spop!()]; Number::Integer((a >= b) as i64) },
			Self::Eq => Number::Integer((spop!() == spop!()) as i64),
			Self::Neq => Number::Integer((spop!() != spop!()) as i64),
			Self::Cmp => {
				let [b, a] = [spop!(), spop!()];
				Number::Float(
					// Ordering::Less => -1, Ordering::Equal => 0, Ordering::Greater => 1
					// Thanks, rust stdlib
					(a.partial_cmp(&b)).map_or(f64::NAN, |c| c as i8 as f64)
				)
			},
			Self::Tern => {
				let falsy = spop!();
			    let truthy = spop!();
			    let cond = spop!();
				if cond == Number::ZERO { falsy } else { truthy }
			},
			Self::And => Number::Integer(i64::from(spop!()) & i64::from(spop!())),
			Self::Or => Number::Integer(i64::from(spop!()) | i64::from(spop!())),
			Self::Xor => Number::Integer(i64::from(spop!()) ^ i64::from(spop!())),
			Self::Not => Number::Integer(!i64::from(spop!())),
			Self::Shl => {let [b, a] = [spop!(), spop!()]; Number::Integer(i64::from(a) << ((i64::from(b) as u64) % 64))},
			Self::Shr => {let [b, a] = [spop!(), spop!()]; Number::Integer(((i64::from(a) as u64) >> (i64::from(b) as u64 % 64)) as i64)},
			Self::AShr => {let [b, a] = [spop!(), spop!()]; Number::Integer(i64::from(a) >> ((i64::from(b) as u64) % 64))},
			Self::Pow => {let [b, a] = [spop!(), spop!()]; a.pow(b)},
			Self::Log => {let [b, a] = [spop!(), spop!()]; a.log(b)},
			Self::Abs => match spop!() {
				Number::Complex(c) => Number::Float(c.norm()),
				other => if other < Number::ZERO {-other} else {other} 
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
				other => other
			},
			Self::Imag => match spop!() {
				Number::Complex(c) => Number::Float(c.im),
				other => Number::ZERO
			},
			Self::Arg => Number::Float(Complex64::from(spop!()).arg()),
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