use std::{borrow::Cow, pin::Pin, task::{Context, Poll}};
use wasm_bindgen::prelude::*;
use macrosia::*;
use wasm_bindgen_futures::js_sys::{self, Promise};

#[wasm_bindgen]
extern "C" {
    pub fn setTimeout(callback: JsValue, timeout_ms: u32);
}


struct ExecFuture {
    exec: *mut Executor,
    reg: *mut VariableRegistry,
    string: *mut [u8],
    func: Option<Box<dyn FnMut() -> Option<Result<Cow<'static, [u8]>, MacroError>>>>,
    last_yield_time: f64
}

// wasm-bindgen-futures does this. but i do not care. did it myself anyways.

impl Future for ExecFuture {

    type Output = Result<JsValue, JsValue>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<<Self as Future>::Output> {
        unsafe {
            let this = self.as_mut().get_unchecked_mut();
            let Some(ref mut func) = this.func else {panic!("polled future after done")};
            loop {
                let current_time = js_sys::Date::now();
                if current_time - this.last_yield_time > 16.0 { // 16 ms
                    let waker = ctx.waker().clone();
                    let closure = Closure::once_into_js(move || waker.wake());
                    setTimeout(closure, 0);
                    this.last_yield_time = current_time;
                    break Poll::Pending;
                }
                let res = (func)();
                if let Some(v) = res {
                    let res = match v {
                        Ok(v) => v.into_owned(),
                        Err(e) => format!("[MACROSCRIPT ERROR]\n{e}").into_bytes()
                    };
                    let res_str = String::from_utf8_lossy(&res).into_owned();
                    this.func = None;
                        // Explicitly drop these
                        std::mem::drop(Box::from_raw(self.reg));
                        std::mem::drop(Box::from_raw(self.string));
                        std::mem::drop(Box::from_raw(self.exec));
                    break Poll::Ready(Ok(res_str.into()))
                }
            }
        }
    }
}

#[wasm_bindgen]
#[allow(unsafe_op_in_unsafe_fn)]
pub unsafe fn evaluate(mac: String) -> Promise {
    console_error_panic_hook::set_once();
    let exec = Box::into_raw(Box::new(Executor::new().with_stdlib()));
    let reg = Box::into_raw(Box::new(VariableRegistry::new()));
    let s = Box::into_raw(mac.into_bytes().into_boxed_slice());
    let func = (&mut *exec).evaluate(&*s, &mut *reg);

    wasm_bindgen_futures::future_to_promise(ExecFuture {
        exec, reg, string: s, func: Some(Box::new(func)), last_yield_time: js_sys::Date::now()
    })
}

#[wasm_bindgen]
pub fn get_stdlib_macro_names() -> Vec<String> {
    let exec = Executor::new().with_stdlib();
    exec.macros().keys()
        .map(|v| String::from_utf8_lossy(&*v).into_owned())
        .collect()
}
