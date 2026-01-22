use std::{borrow::Cow, collections::HashMap, hash::{BuildHasher, Hasher}};
use crate::expr::ExpressionFunction;

#[repr(transparent)]
struct IdentityHash(u64);
impl BuildHasher for IdentityHash {
    type Hasher = Self;
    fn build_hasher(&self) -> Self::Hasher {
        Self(0)
    }
}
impl Hasher for IdentityHash {
    fn write(&mut self, bytes: &[u8]) {
        let buf = unsafe { std::mem::transmute::<&mut u64, &mut [u8; 8]>(&mut self.0) };
        buf[..bytes.len().min(8)].copy_from_slice(bytes);
    }
    fn write_u64(&mut self, i: u64) { self.0 = i; }
    fn finish(&self) -> u64 { self.0 }
}

/// A registry to store variables in during macro execution.
pub struct VariableRegistry {
    // This is vulnerable to hash collisions,
    // but the alternative would require me to
    // keep the name borrowed or clone it, which just won't work.
    // Also, considering how ephemeral and sandboxed variables are,
    // it doesn't really matter :shrug:
    vars: HashMap<u64, Vec<u8>, IdentityHash>,
    funcs: HashMap<u64, ExpressionFunction, IdentityHash>
}

impl VariableRegistry {
    /// Creates a new variable registry.
    pub const fn new() -> Self {
        Self {
            vars: HashMap::with_hasher(IdentityHash(0)),
            funcs: HashMap::with_hasher(IdentityHash(0)),
        }
    }

    /// Loads a variable of a given name.
    pub fn load<'slf, 'name>(&'slf self, name: &'name [u8]) -> Option<&'slf [u8]> {
        let name_hash = seahash::hash(name);
        self.vars.get(&name_hash).map(|v| v.as_slice())
    }

    /// Loads a function of a given name.
    pub fn load_fn<'slf, 'name>(&'slf self, name: &'name [u8]) -> Option<&'slf ExpressionFunction> {
        let name_hash = seahash::hash(name);
        self.funcs.get(&name_hash)
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

    /// Stores a function into a given name, dropping the old function if it existed.
    pub fn store_fn<'val, 'slf, 'name>(&'slf mut self, name: &'name [u8], val: ExpressionFunction) {
        let name_hash = seahash::hash(name);
        self.funcs.insert(name_hash, val);
    }

    /// Drops a variable of a given name, returning a boolean for whether it existed in the first place.
    pub fn drop<'slf, 'name>(&'slf mut self, name: &'name [u8]) -> bool {
        let name_hash = seahash::hash(name);
        self.vars.remove(&name_hash).is_some()
    }
}
