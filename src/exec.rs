

use std::{borrow::Cow, collections::HashMap, hash::BuildHasherDefault, iter::FromFn};

use rand::SeedableRng;

use crate::{var_reg::VariableRegistry, Macro, MacroError};

type MacroMap = HashMap<Cow<'static, [u8]>, Box<dyn Macro>, BuildHasherDefault<seahash::SeaHasher>>;

/// An executor interface for Macrosia.
pub struct Executor {
	macros: MacroMap,
	context: u8
}

impl Clone for Executor {
	fn clone(&self) -> Self {
		Self {
			macros: self.macros.iter().map(|(key, mac)| (key.clone(), Macro::clone(&**mac))).collect(),
			context: self.context
		}
	}
}

struct StackTriple<'s> { start: usize, target: Cow<'s, [u8]>, end: usize }
impl<'s> StackTriple<'s> {
	fn concat(self, parent: &[u8]) -> Cow<'s, [u8]> {
		if self.start == 0 && self.end == parent.len() {
			return self.target;
		}
		let mut buf = vec![0; parent.len() + self.target.len() - (self.end - self.start)];
		buf[..self.start].copy_from_slice(&parent[..self.start]);
		buf[self.start .. self.start + self.target.len()].copy_from_slice(&*self.target);
		buf[self.start + self.target.len() ..].copy_from_slice(&parent[self.end..]);
		return Cow::Owned(buf)
	}
}

impl Executor {
	/// Creates a new, empty execution context.
	pub fn new(context: u8) -> Self {
		Self { macros: HashMap::default(), context }
	}

	/// Gets the context in which this executor is running.
	#[inline]
	pub const fn context(&self) -> u8 { self.context }

	/// Gets a macro from the executor.
	pub fn get_macro(&self, macro_name: &[u8]) -> Option<&dyn Macro> {
		self.macros.get(macro_name).map(|v| &**v)
	}

	/// Gets the map of all macros in the executor keyed with their names.
	pub fn macros(&self) -> &MacroMap {
		&self.macros
	}

	#[inline]
	/// Adds a macro to the executor.
	pub fn add_macro(&mut self, mac: impl Macro + 'static) {
		self.add_macro_mono(Box::new(mac))
	}

	fn add_macro_mono(&mut self, mac: Box<dyn Macro>) {
		self.macros.insert(mac.name(), mac);
	}

	fn find_first_block(str: &[u8]) -> Option<[usize; 2]> {
		let mut start = 0;
		let mut last_escape = false;
		for (i, c) in str.iter().copied().enumerate() {
			if last_escape {
				last_escape = false;
				continue;
			}
			if c == b'[' { start = i; continue; }
			if c == b']' { return Some([start, i+1]) }
			if c == b'\\' { last_escape = true; }
		}

		None
	}

	// Split a macro block `a/b/c/d/...` into its arguments, properly handling any escaped slashes.
	// Returns a single empty string if the macro is empty.
	pub(crate) fn split_args<'a>(str: &'a [u8]) -> FromFn<impl FnMut() -> Option<&'a [u8]>> {
		let mut str_left = str;
		let mut done = false;

		let mut was_escape = false;
		std::iter::from_fn(move || {
			if done { return None }
			let mut idx = 0;
			let mut iter = str_left.iter();
			loop {
				let Some(c) = iter.next() else { done = true; return Some(str_left) };
				if was_escape { was_escape = false; }
				else {
					if *c == b'/' { break }
					if *c == b'\\' { was_escape = true; }
				}
				idx += 1;
			}
			let res = &str_left[..idx];
			// Skip past the /
			str_left = &str_left[idx + 1..];
			Some(res)
		})
	}

	/*
	a := [b][c], b := [c][c], [c] := !
	(, foo[a]bar, )
	(, foo[a]bar, ) (foo, [b][c], bar)
	(, foo[a]bar, ) (foo, [b][c], bar) (, [c][c], [c])
	(, foo[a]bar, ) (foo, [b][c], bar) (, [c][c], [c]) (, !, [c]) Concat pre+res+suf to -1.res
	(, foo[a]bar, ) (foo, [b][c], bar) (, ![c], [c])
	(, foo[a]bar, ) (foo, [b][c], bar) (, ![c], [c]) (, !, )
	(, foo[a]bar, ) (foo, [b][c], bar) (, !!, [c])
	(, foo[a]bar, ) (foo, !![c], bar)
	(, foo[a]bar, ) (foo, !![c], bar), (!!, !, )
	(, foo[a]bar, ) (foo, !!!, bar)
	(, foo!!!bar, )
	*/

	/// Evaluates a given bytestring, returning it with all macros expanded...
	///
	/// # Errors
	/// ...or an error if one occurred within one of the expanded macros.
	///
	/// # Why is the return like that.
	/// If I had implemented this normally, this function may not terminate immediately - in fact, it may not terminate at all.
	/// This returns a function that you can call to iterate one time over what would've been a loop.
	/// The function will return a [`None`] until it is done, and then a [`Some`] containing the output value.
	/// If you call the function after that, it will simply return a `Some(Err(...))`.
	///
	/// Think of it like a [`Coroutine`](core::ops::Coroutine).
	pub fn evaluate<'slf, 'reg: 'slf, 'buf: 'reg>(&'slf self, string: &'buf [u8], reg: &'reg mut VariableRegistry) -> impl FnMut() -> Option<Result<Cow<'buf, [u8]>, MacroError>> {
		let mut rng = rand::rngs::SmallRng::from_rng(&mut rand::rng());

		let mut stack_opt = Some(Vec::<StackTriple<'buf>>::from([StackTriple{start: 0, target: Cow::Borrowed(string), end: 0}]));

		move || {
			let Some(ref mut stack) = stack_opt else { return Some(Err("called after done".into())); };
			let top = match stack.last_mut().ok_or("macro stack empty") {
				Ok(v) => v,
				Err(e) => return Some(Err(e.into()))
			};
			let (res, start, end) = {
				let Some([start, end]) = Self::find_first_block(&top.target) else {
					let top = stack.pop().unwrap();
					let Some(triple) = stack.last_mut() else {
						stack_opt.take();
						// We're done
						return Some(Ok(top.concat(b"")))
					};
					triple.target = top.concat(&triple.target);
					return None;
				};
				let mut args = Self::split_args(&top.target[start + 1..end - 1]);
				let name = args.next().expect("macro must have at least one argument (its name)");
				let Some(mac) = self.get_macro(name) else {
					std::mem::drop(args);
					return Some(Err(MacroError {
						message: Cow::Owned(format!("macro with name {} does not exist", String::from_utf8_lossy(name))),
						trace: self.get_trace(stack_opt.take().unwrap())
					}));
				};
				let res = match mac.eval(self, reg, &mut rng, &mut args) {
					Ok(val) => val,
					Err(mut e) => {
						let mac = Vec::from(&top.target[start..end]);
						std::mem::drop(args);
						stack.push(StackTriple { start: 0, target: Cow::Owned(mac), end: 0 });
						e.trace = self.get_trace(stack_opt.take().unwrap());
						return Some(Err(e));
					}
				};

				(res, start, end)
			};
			if start == 0 && end == top.target.len() {
				top.target = res;
				return None;
			}
			// SAFETY: try_reserve won't reallocate if it errors, meaning top will still be valid
			if stack.try_reserve(1).is_err() {
				stack.clear(); // hopefully take back some of our memory
				let mut err = MacroError::from("memory exhausted during expansion - is the macro stuck in an infinite loop?");
				err.trace = self.get_trace(stack_opt.take().unwrap());
				return Some(Err(err));
			}
			stack.push(StackTriple { start, target: res, end });
			None
		}
	}

	fn get_trace(&self, stack: Vec<StackTriple<'_>>) -> Vec<Vec<u8>> {
        stack.into_iter().map(|s| s.target.into_owned()).collect()
    }
}



#[cfg(test)]
mod test {
    use crate::exec::Executor;

	#[test]
	fn argsplit() {
		macro_rules! check {
		    ($a: literal, [$($l: literal),*]) => {
				assert_eq!((Executor::split_args($a).collect::<Vec<_>>().as_slice()), &[$($l as &[_]),*])
		    };
		}
		check!(b"[]", [b""]);
		check!(b"[abcde]", [b"abcde"]);
		check!(b"[abcde/]", [b"abcde", b""]);
		check!(b"[abc/de]", [b"abc", b"de"]);
		check!(br"[abc\/de]", [br"abc\/de"]);
		check!(br"[abc\\/de]", [br"abc\\", b"de"]);
		check!(br"[abc\\/]", [br"abc\\", b""]);
		check!(br"[abc\\/]", [br"abc\\", b""]);
		check!(br"[abc/de/]", [br"abc", b"de", b""]);
	}
}
