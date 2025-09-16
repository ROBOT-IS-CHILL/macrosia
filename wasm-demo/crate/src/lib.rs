use std::collections::{HashMap, HashSet};
use itertools::Itertools;
use std::sync::OnceLock;
use macrosia::*;
use std::{
    borrow::Cow,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::js_sys::{Array, Promise, Object, Reflect};

#[wasm_bindgen]
extern "C" {
    pub fn getTiles() -> JsValue;
    pub fn setTimeout(callback: JsValue, timeout_ms: u32);
}

fn get_tiles() -> Result<HashMap<String, TileData>, JsValue> {
    Ok(serde_wasm_bindgen::from_value(getTiles())?)
}

#[derive(serde::Deserialize, Clone)]
#[allow(dead_code)]
struct TileData {
    active_color: [f64; 2],
    inactive_color: [f64; 2],
    sprite: [String; 2],
    tags: HashSet<String>,
    tiling: String
}

static BASE_EXECUTOR: OnceLock<Executor> = OnceLock::new();

struct ExecFuture {
    exec: *mut Executor,
    reg: *mut VariableRegistry,
    string: *mut [u8],
    func: Option<Box<dyn FnMut() -> Option<Result<Cow<'static, [u8]>, MacroError>>>>,
}

// wasm-bindgen-futures does this. but i do not care. did it myself anyways.

impl Future for ExecFuture {
    type Output = Result<JsValue, JsValue>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<<Self as Future>::Output> {
        static ITERS_PER_POLL: usize = 4096;
        unsafe {
            let this = self.as_mut().get_unchecked_mut();
            let Some(ref mut func) = this.func else {
                panic!("polled future after done")
            };
            for _ in 0..ITERS_PER_POLL {
                let res = func();
                if let Some(v) = res {
                    let res = match v {
                        Ok(v) => v.into_owned(),
                        Err(e) => format!("[MACRO ERROR]\n{e}").into_bytes(),
                    };
                    let res_str = String::from_utf8_lossy(&res).into_owned();
                    this.func = None;
                    // Explicitly drop these
                    std::mem::drop(Box::from_raw(self.reg));
                    std::mem::drop(Box::from_raw(self.string));
                    std::mem::drop(Box::from_raw(self.exec));
                    return Poll::Ready(Ok(res_str.into()));
                }
            }
            let waker = ctx.waker().clone();
            let closure = Closure::once_into_js(move || waker.wake());
            setTimeout(closure, 0);
            Poll::Pending
        }
    }
}

static KILL_MACROS: AtomicBool = AtomicBool::new(false);

#[wasm_bindgen]
pub fn cancel_running_macro() {
    KILL_MACROS.store(true, Ordering::Relaxed)
}

struct TilesMacro;

impl Macro for TilesMacro {
    fn name(&self) -> &[u8] { b"tiles" }
    fn source(&self) -> &[u8] {
        b"<yea i can't be assed to figure out how to make this show up here sorry>"
    }
    fn eval<'arg, 'reg: 'arg, 'exec: 'reg>(
        &self,
        _x: &'exec macrosia::Executor,
        _v: &'reg mut VariableRegistry,
        _r: &mut macrosia::rand_xoshiro::Xoshiro128PlusPlus,
        args: &mut dyn Iterator<Item = &'arg [u8]>
    ) -> Result<Cow<'static, [u8]>, MacroError> {
        let queries = args
            .map(str::from_utf8)
            .process_results(|iter| iter
                .map(|v| {
                    let Some((query, value)) = v.split_once(':') else { return Err(format!("invalid query: {v}")) };
                    if !matches!(query, "name" | "tiling" | "source" | "tag") {
                        return Err(format!("invalid query: {v}"));
                    }
                    Ok((query, value))
                }).collect::<Vec<_>>()
            )?;
        static TILES: OnceLock<HashMap<String, TileData>> = OnceLock::new();
        let mut tiles =
            TILES.get_or_init(|| get_tiles().expect("failed to get tile data"))
            .clone();
        for query_res in queries {
            let (query, value) = query_res?;
            match query {
                "name" => {
                    let regex = regex::Regex::new(value).map_err(|err| format!("invalid regex {value}: {err}"))?;
                    tiles.retain(|k, _| regex.is_match(k) )
                },
                "tiling" => tiles.retain(|_, v| v.tiling == value),
                "source" => tiles.retain(|_, v| v.sprite[0] == value),
                "tag" => tiles.retain(|_, v| v.tags.contains(value)),
                _ => {}
            }
        }
        Ok(Cow::Owned(tiles.keys().sorted().map(|tilename| {
            tilename.replace("\\", "\\\\")
                    .replace("[", "\\[").replace("/", "\\/")
                    .replace("]", "\\]").replace(" ", "\\ ")
                    .replace("$", "\\$")
        }).join("/").into_bytes()))
    }
    fn clone(&self) -> Box<dyn macrosia::Macro> { Box::new(Self) }
    fn description(&self) -> &str { "" }
}

#[wasm_bindgen]
pub fn initialize_executor(database_macros: Object) {
    console_error_panic_hook::set_once();
    BASE_EXECUTOR.get_or_init(|| {
        let mut exec = Executor::new(b'x');
        for entry in Object::entries(&database_macros) {
            let entry = Array::from(&entry);
            let name: String = entry.get(0)
                .as_string()
                .expect("database macro name was not string");
            let data: Object = entry.get(1).into();
            let value: String = Reflect::get(&data, &JsValue::from_str("value"))
                .expect("value field did not exist on database macro")
                .as_string()
                .expect("database macro value was not string");
            let description: String = Reflect::get(&data, &JsValue::from_str("description"))
                .expect("description field did not exist on database macro")
                .as_string()
                .expect("database macro description was not string");
            exec.add_macro(TextMacro {
                name: Arc::new(name.into_bytes()),
                source: Arc::new(value.into_bytes()),
                description: Arc::new(description)
            })
        }

        exec.add_macro(TilesMacro);

        exec.add_stdlib();
        exec
    });
}

#[wasm_bindgen]
#[allow(unsafe_op_in_unsafe_fn)]
pub unsafe fn evaluate(mac: String) -> Promise {
    KILL_MACROS.store(false, Ordering::Relaxed);
    let mut exec: Executor = BASE_EXECUTOR.get()
        .expect("executor should be initialized by now")
        .clone();
    let mac = mac
        .lines()
        .filter(|line| {
            if line.starts_with("#define ") {
                let Some((name, source)) = line
                    .strip_prefix("#define ")
                    .and_then(|v| v.split_once(' '))
                else {
                    return true;
                };
                let name = name.trim();
                let source = source.trim();
                exec.add_macro(TextMacro {
                    name: Arc::new(String::from(name).into_bytes()),
                    source: Arc::new(String::from(source).into_bytes()),
                    description: Arc::new(String::new())
                });
                return false;
            }
            true
        })
        .collect::<Vec<&str>>()
        .join("\n");

    let exec = Box::into_raw(Box::new(exec));
    let reg = Box::into_raw(Box::new(VariableRegistry::new()));
    let s = Box::into_raw(mac.into_bytes().into_boxed_slice());
    let func = (&mut *exec).evaluate(&*s, &mut *reg, None, None, &KILL_MACROS);

    wasm_bindgen_futures::future_to_promise(ExecFuture {
        exec,
        reg,
        string: s,
        func: Some(Box::new(func)),
    })
}

#[wasm_bindgen]
pub fn get_stdlib_macro_names() -> Vec<String> {
    let mut exec: Executor = Executor::new(0);
    exec.add_stdlib();
    exec.macros().iter()
        .map(|(v, m)| format!("{}\n{}",
            String::from_utf8_lossy(&*v).into_owned(),
            m.description()
        ))
        .collect()
}
