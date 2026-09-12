use seahash::SeaHasher;
use std::{
    collections::HashMap, fmt::{Debug, Display, Formatter}, hash::{BuildHasherDefault, Hash, Hasher}, sync::{RwLock, atomic::{AtomicUsize, Ordering}}
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

macro_rules! with_interner {
    ($int: ident $body: expr) => {
        (|$int: ::std::sync::RwLockReadGuard<'_, crate::intern::Interner>| $body)($crate::intern::INTERNER.read().expect("other thread panicked"))
    };
}

macro_rules! with_interner_mut {
    ($int: ident $body: expr) => {
        (|mut $int: ::std::sync::RwLockWriteGuard<'_, crate::intern::Interner>| $body)($crate::intern::INTERNER.write().expect("other thread panicked"))
    };
}

static WITH_SEMAPHORE: AtomicUsize = AtomicUsize::new(0);

/// Invalidates all interner entries and clears the interner.
/// 
/// # Safety
/// There must not exist any references to the underlying interner string at the time of calling this function.
/// 
/// In essence, you must not be within a call to [`InternerEntry::with`], on **any** thread. 
pub unsafe fn clear() {
    if WITH_SEMAPHORE.load(Ordering::SeqCst) > 0 {
        panic!("tried to clear while within a call to InternerEntry::with, undefined behavior prevented; this panic is not guaranteed, so the program is unsound");
    }
    with_interner_mut! {int {
        for i in int.list.drain(..) {
            unsafe {
                let _ = Box::from_raw(i as *const [u8] as *mut [u8]);
            }
        }
        int.list.shrink_to_fit();
        int.map.clear();
        int.map.shrink_to_fit();
    } }
}

impl InternerEntry {
    /// Gets the entry corresponding to a string if it exists.
    pub fn get(string: &[u8]) -> Option<InternerEntry> {
        with_interner! { int {
            if let Some(id) = int.map.get(&string) {
                return Some(InternerEntry(*id));
            }
            None
        } }
    }
    /// Gets the entry corresponding to a string, or interns it and returns the new entry.
    pub fn get_or_intern(string: &[u8]) -> InternerEntry {
        with_interner_mut! { int {
            if let Some(id) = int.map.get(&string) {
                return InternerEntry(*id);
            }
            let id = int.list.len();
            let boxed_slice: Box<[u8]> = Box::from(string);
            let ptr: &'static [u8] = Box::leak(boxed_slice);
            int.map.insert(ptr, id);
            int.list.push(ptr);
            InternerEntry(id)
        } }
    }
    /// Gets the string corresponding to an entry.
    ///
    /// # Deadlocks
    /// This function will deadlock if the passed function or another thread calls [`InternerEntry::get_or_intern`] while this function is running.
    pub fn with<'slice, 'this: 'slice, T>(&'this self, fun: impl FnOnce(Option<&'slice [u8]>) -> T) -> T {
        WITH_SEMAPHORE.fetch_add(1, Ordering::SeqCst);
        with_interner! { int {
            let res = fun(int
                .list
                .get(self.0)
                .map(|v| *v)
            );
            WITH_SEMAPHORE.fetch_sub(1, Ordering::SeqCst);
            res
        } }
    }
}

impl Debug for InternerEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.with(|str| {
            write!(f, "{:?}", str.map_or_else(|| "<missing>".into(), String::from_utf8_lossy))
        })
    }
}

impl Display for InternerEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.with(|str| {
            write!(f, "{:?}", str.map_or_else(|| "<missing>".into(), String::from_utf8_lossy))
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