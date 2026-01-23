use seahash::SeaHasher;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher, BuildHasherDefault},
    sync::RwLock,
    fmt::{Debug, Display, Formatter}
};

pub(crate) struct Interner {
    map: HashMap<&'static [u8], usize, BuildHasherDefault<SeaHasher>>,
    list: Vec<&'static [u8]>,
}

/// A hook into a global string interner.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct InternerEntry(usize);

pub(crate) static INTERNER: RwLock<Interner> = RwLock::new(Interner {
    map: HashMap::with_hasher(BuildHasherDefault::<SeaHasher>::new()),
    list: Vec::new(),
});

impl InternerEntry {
    /// Gets the entry corresponding to a string, or interns it and returns the new entry.
    pub fn get_or_intern(string: &[u8]) -> InternerEntry {
        let mut lock = INTERNER
            .write()
            .map_err(|_| panic!("other thread panicked"))
            .unwrap();
        if let Some(id) = lock.map.get(&string) {
            return InternerEntry(*id);
        }
        let id = lock.list.len();
        let boxed_slice: Box<[u8]> = Box::from(string);
        let ptr: &'static [u8] = Box::leak(boxed_slice);
        lock.map.insert(ptr, id);
        lock.list.push(ptr);
        InternerEntry(id)
    }
    /// Gets the string corresponding to an entry.
    ///
    /// # Deadlocks
    /// This function will deadlock if the passed function or another thread calls [`InternerEntry::get_or_intern`] while this function is running.
    pub fn with<T>(&self, fun: impl FnOnce(&[u8]) -> T) -> T {
        let lock = INTERNER
            .read()
            .map_err(|err| panic!("other thread panicked: {err}"))
            .unwrap();
        fun(lock
            .list
            .get(self.0)
            .expect("interner entry existing implies it has a map entry"))
    }
}



impl Debug for InternerEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.with(|str| {
            write!(f, "{:?}", String::from_utf8_lossy(str))
        })
    }
}

impl Display for InternerEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.with(|str| {
            write!(f, "{}", String::from_utf8_lossy(str))
        })
    }
}

impl From<&[u8]> for InternerEntry {
    fn from(val: &[u8]) -> Self {
        Self::get_or_intern(val)
    }
}
impl From<&str> for InternerEntry {
    fn from(val: &str) -> Self {
        Self::get_or_intern(val.as_bytes())
    }
}

impl Hash for InternerEntry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}