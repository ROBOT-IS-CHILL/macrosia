use std::{borrow::Cow, pin::Pin, sync::{atomic::{AtomicBool, Ordering}, Arc}, task::{Context, Poll}};
use wasm_bindgen::prelude::*;
use macrosia::*;
use wasm_bindgen_futures::js_sys::Promise;

#[wasm_bindgen]
extern "C" {
    pub fn setTimeout(callback: JsValue, timeout_ms: u32);
}


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
        static ITERS_PER_POLL: usize = 16384;
        unsafe {
            let this = self.as_mut().get_unchecked_mut();
            let Some(ref mut func) = this.func else {panic!("polled future after done")};
            for _ in 0..ITERS_PER_POLL {
                let res = func();
                if let Some(v) = res {
                    let res = match v {
                        Ok(v) => v.into_owned(),
                        Err(e) => format!("[MACRO ERROR]\n{e}").into_bytes()
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
            if KILL_MACROS.load(Ordering::Relaxed) {
                KILL_MACROS.store(false, Ordering::Relaxed);
                return Poll::Ready(Ok("[Execution cancelled.]".into()));
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

#[wasm_bindgen]
#[allow(unsafe_op_in_unsafe_fn)]
pub unsafe fn evaluate(mac: String) -> Promise {
    console_error_panic_hook::set_once();
    let mut exec = Executor::new(b'x').with_stdlib();
    let mac = mac.lines().filter(|line| {
        if line.starts_with("#define ") {
            let Some((name, source)) = line.strip_prefix("#define ").and_then(|v| v.split_once(' ')) else { return true; };
            let name = name.trim();
            let source = source.trim();
            exec.add_macro(TextMacro {
                name: Arc::new(String::from(name).into_bytes()),
                source: Arc::new(String::from(source).into_bytes())
            });
            return false;
        }
        true
    }).collect::<Vec<&str>>().join("\n");

    let exec = Box::into_raw(Box::new(exec));
    let reg = Box::into_raw(Box::new(VariableRegistry::new()));
    let s = Box::into_raw(mac.into_bytes().into_boxed_slice());
    let func = (&mut *exec).evaluate(&*s, &mut *reg);

    wasm_bindgen_futures::future_to_promise(ExecFuture {
        exec, reg, string: s, func: Some(Box::new(func))
    })
}

#[wasm_bindgen]
pub fn get_stdlib_macro_names() -> Vec<String> {
    let exec = Executor::new(0).with_stdlib();
    exec.macros().keys()
        .map(|v| String::from_utf8_lossy(&*v).into_owned())
        .collect()
}
