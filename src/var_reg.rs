
use std::{borrow::Cow, collections::BTreeMap};


/// A registry to store variables in during macro execution.
pub struct VariableRegistry {
	// This is vulnerable to hash collisions,
	// but the alternative would require me to
	// keep the name borrowed or clone it, which just won't work.
	// Also, considering how ephemeral and sandboxed variables are,
	// it doesn't really matter :shrug:
	vars: BTreeMap<u64, Vec<u8>>
}

impl VariableRegistry {
	/// Creates a new variable registry.
	pub const fn new() -> Self {
		Self { vars: BTreeMap::new() }
	}

	/// Loads a variable of a given name.
	pub fn load<'slf, 'name>(&'slf self, name: &'name [u8]) -> Option<&'slf [u8]> {
		let name_hash = seahash::hash(name);
		self.vars.get(&name_hash).map(|v| v.as_slice())
	}

	/// Loads a variable of a given name, giving a mutable reference to it.
	pub fn load_mut<'slf, 'name>(&'slf mut self, name: &'name [u8]) -> Option<&'slf mut [u8]> {
		let name_hash = seahash::hash(name);
		self.vars.get_mut(&name_hash).map(|v| v.as_mut_slice())
	}

	/// Stores a variable into a given name, dropping the old value if it existed.
	pub fn store<'val, 'slf, 'name>(&'slf mut self, name: &'name [u8], val: Cow<'val, [u8]>) {
		let name_hash = seahash::hash(name);
		self.vars.insert(name_hash, val.into_owned());
	}

	/// Drops a variable of a given name, returning a boolean for whether it existed in the first place.
	pub fn drop<'slf, 'name>(&'slf mut self, name: &'name [u8]) -> bool {
		let name_hash = seahash::hash(name);
		self.vars.remove(&name_hash).is_some()
	}
}
