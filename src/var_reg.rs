use crate::{
    expr::ExpressionFunction,
    intern::InternerEntry
};
use std::{
    borrow::Cow,
    collections::HashMap,
    hash::{BuildHasherDefault, BuildHasher, Hasher},
};

/// A registry to store variables in during macro execution.
pub struct VariableRegistry {
    vars: HashMap<InternerEntry, Vec<u8>, BuildHasherDefault<seahash::SeaHasher>>,
    funcs: HashMap<InternerEntry, ExpressionFunction, BuildHasherDefault<seahash::SeaHasher>>,
}

impl VariableRegistry {
    /// Creates a new variable registry.
    pub const fn new() -> Self {
        Self {
            vars: HashMap::with_hasher(BuildHasherDefault::new()),
            funcs: HashMap::with_hasher(BuildHasherDefault::new()),
        }
    }

    /// Loads a variable of a given name.
    pub fn load<'slf, 'name>(&'slf self, name: InternerEntry) -> Option<&'slf [u8]> {
        self.vars.get(&name).map(|v| v.as_slice())
    }

    /// Loads a function of a given name.
    pub fn load_fn<'slf, 'name>(&'slf self, name: InternerEntry) -> Option<&'slf ExpressionFunction> {
        self.funcs.get(&name)
    }

    /// Loads a variable of a given name, giving a mutable reference to it.
    pub fn load_mut<'slf, 'name>(&'slf mut self, name: InternerEntry) -> Option<&'slf mut [u8]> {
        self.vars.get_mut(&name).map(|v| v.as_mut_slice())
    }

    /// Stores a variable into a given name, dropping the old value if it existed.
    pub fn store<'val, 'slf, 'name>(&'slf mut self, name: InternerEntry, val: Cow<'val, [u8]>) {
        self.vars.insert(name, val.into_owned());
    }

    /// Stores a function into a given name, dropping the old function if it existed.
    pub fn store_fn<'val, 'slf, 'name>(&'slf mut self, name: InternerEntry, val: ExpressionFunction) {
        self.funcs.insert(name, val);
    }

    /// Drops a variable of a given name, returning a boolean for whether it existed in the first place.
    pub fn drop<'slf, 'name>(&'slf mut self, name: InternerEntry) -> bool {
        self.vars.remove(&name).is_some()
    }
}
