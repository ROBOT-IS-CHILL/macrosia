use crate::{
    expr::CompiledExpr,
    intern::InternerEntry
};
use std::{
    borrow::Cow,
    collections::HashMap,
    hash::BuildHasherDefault,
};

/// A registry to store variables in during macro execution.
pub struct VariableRegistry {
    vars: HashMap<InternerEntry, Vec<u8>, BuildHasherDefault<seahash::SeaHasher>>,
    funcs: HashMap<InternerEntry, CompiledExpr, BuildHasherDefault<seahash::SeaHasher>>,
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
    pub fn load_fn<'slf, 'name>(&'slf self, name: InternerEntry) -> Option<&'slf CompiledExpr> {
        self.funcs.get(&name)
    }

    /// Loads a variable of a given name, giving a mutable reference to it.
    pub fn load_mut<'slf, 'name>(&'slf mut self, name: InternerEntry) -> Option<&'slf mut [u8]> {
        self.vars.get_mut(&name).map(|v| v.as_mut_slice())
    }
    
    /// Loads two variables of given names, giving mutable references to both.
    pub fn load_two_mut<'slf, 'name>(&'slf mut self, name1: InternerEntry, name2: InternerEntry) -> Option<[&'slf mut [u8]; 2]> {
        if name1 == name2 { return None; }
        // SAFETY: We checked that these two variables are not the same, and double-check afterwards.
        unsafe {
            let var1 = self.vars.get_mut(&name1)? as *mut Vec<u8>;
            let var2 = self.vars.get_mut(&name2)? as *mut Vec<u8>;
            if var1 == var2 { return None; }
            Some([(&mut *var1).as_mut_slice(), (&mut *var2).as_mut_slice()])
        }
    }

    /// Stores a variable into a given name, dropping the old value if it existed.
    pub fn store<'val, 'slf, 'name>(&'slf mut self, name: InternerEntry, val: Cow<'val, [u8]>) {
        self.vars.insert(name, val.into_owned());
    }

    /// Stores a function into a given name, dropping the old function if it existed.
    pub fn store_fn<'val, 'slf, 'name>(&'slf mut self, name: InternerEntry, val: CompiledExpr) {
        self.funcs.insert(name, val);
    }

    /// Drops a variable of a given name, returning a boolean for whether it existed in the first place.
    pub fn drop<'slf, 'name>(&'slf mut self, name: InternerEntry) -> bool {
        self.vars.remove(&name).is_some()
    }
}
