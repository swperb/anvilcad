//! Minimal late-bound COM automation (IDispatch) for talking to SOLIDWORKS from Rust.
//!
//! Every SOLIDWORKS API interface is a dual interface, so calling methods and properties by
//! name through IDispatch reaches all of it without generated bindings.

use std::mem::ManuallyDrop;
use windows::core::{Interface, BSTR, GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::{VARIANT_BOOL, VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Variant::*;

const LOCALE_USER_DEFAULT: u32 = 0x0400;

#[derive(Debug, Clone)]
pub struct ComError(pub String);

impl std::fmt::Display for ComError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ComError {}

impl From<windows::core::Error> for ComError {
    fn from(e: windows::core::Error) -> Self {
        ComError(format!("{} (0x{:08X})", e.message(), e.code().0 as u32))
    }
}

pub type ComResult<T> = Result<T, ComError>;

/// Initializes COM for the current thread. The multithreaded apartment avoids routing
/// every reply through a message loop, which made calls to SOLIDWORKS ~100x slower.
pub fn init() -> ComResult<()> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    Ok(())
}

/// An automation object.
/// Member ids are cached per object: each lookup is a cross-process round trip.
#[derive(Clone)]
pub struct Disp {
    obj: IDispatch,
    ids: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, i32>>>,
}

/// A value returned from automation.
#[derive(Clone, Default)]
pub enum Value {
    #[default]
    Empty,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Disp(Disp),
    Array(Vec<Value>),
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Empty => write!(f, "Empty"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Str(s) => write!(f, "{s:?}"),
            Value::Disp(_) => write!(f, "<object>"),
            Value::Array(a) => f.debug_list().entries(a).finish(),
        }
    }
}

impl Value {
    pub fn f64(&self) -> Option<f64> {
        match *self {
            Value::Float(x) => Some(x),
            Value::Int(i) => Some(i as f64),
            Value::Bool(b) => Some(b as i32 as f64),
            _ => None,
        }
    }
    pub fn i64(&self) -> Option<i64> {
        match *self {
            Value::Int(i) => Some(i),
            Value::Float(x) => Some(x as i64),
            Value::Bool(b) => Some(b as i64),
            _ => None,
        }
    }
    pub fn bool(&self) -> Option<bool> {
        match *self {
            Value::Bool(b) => Some(b),
            Value::Int(i) => Some(i != 0),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn disp(&self) -> Option<&Disp> {
        match self {
            Value::Disp(d) => Some(d),
            _ => None,
        }
    }
    /// Array elements; a single value becomes a one-element list and Empty an empty one.
    pub fn list(&self) -> Vec<Value> {
        match self {
            Value::Array(a) => a.clone(),
            Value::Empty => Vec::new(),
            other => vec![other.clone()],
        }
    }
    pub fn floats(&self) -> Vec<f64> {
        self.list().iter().filter_map(|v| v.f64()).collect()
    }
    pub fn objects(&self) -> Vec<Disp> {
        self.list().iter().filter_map(|v| v.disp().cloned()).collect()
    }
}

/// An argument to an automation call.
pub enum Arg {
    Bool(bool),
    Int(i32),
    Float(f64),
    Str(String),
    Disp(Option<Disp>),
    /// An empty VARIANT (VB's `Nothing` for Object/Variant parameters).
    Empty,
    /// Output parameters, returned in order from [`Disp::call_out`].
    OutInt,
    OutStr,
    OutBool,
}

impl From<bool> for Arg {
    fn from(v: bool) -> Arg {
        Arg::Bool(v)
    }
}
impl From<i32> for Arg {
    fn from(v: i32) -> Arg {
        Arg::Int(v)
    }
}
impl From<f64> for Arg {
    fn from(v: f64) -> Arg {
        Arg::Float(v)
    }
}
impl From<&str> for Arg {
    fn from(v: &str) -> Arg {
        Arg::Str(v.to_string())
    }
}
impl From<&Disp> for Arg {
    fn from(v: &Disp) -> Arg {
        Arg::Disp(Some(v.clone()))
    }
}

fn vbool(b: bool) -> VARIANT_BOOL {
    if b {
        VARIANT_TRUE
    } else {
        VARIANT_FALSE
    }
}

unsafe fn variant(vt: VARENUM, fill: impl FnOnce(&mut VARIANT_0_0_0)) -> VARIANT {
    let mut v = unsafe { VariantInit() };
    unsafe {
        (*v.Anonymous.Anonymous).vt = vt;
        fill(&mut (*v.Anonymous.Anonymous).Anonymous);
    }
    v
}

/// Storage for output parameters; boxed so pointers stay valid during the call.
enum OutSlot {
    Int(Box<i32>),
    Str(Box<BSTR>),
    Bool(Box<VARIANT_BOOL>),
}

impl Disp {
    pub fn new(obj: IDispatch) -> Disp {
        Disp { obj, ids: Default::default() }
    }

    pub fn from_unknown(u: &windows::core::IUnknown) -> ComResult<Disp> {
        Ok(Disp::new(u.cast::<IDispatch>()?))
    }

    fn dispid(&self, name: &str) -> ComResult<i32> {
        if let Some(&id) = self.ids.borrow().get(name) {
            return Ok(id);
        }
        let wide = HSTRING::from(name);
        let names = [PCWSTR(wide.as_ptr())];
        let mut id = 0i32;
        unsafe {
            self.obj
                .GetIDsOfNames(&GUID::zeroed(), names.as_ptr(), 1, LOCALE_USER_DEFAULT, &mut id)
                .map_err(|e| ComError(format!("unknown member '{name}': {}", e.message())))?;
        }
        self.ids.borrow_mut().insert(name.to_string(), id);
        Ok(id)
    }

    fn invoke(&self, name: &str, flags: DISPATCH_FLAGS, args: &[Arg]) -> ComResult<(Value, Vec<Value>)> {
        let id = self.dispid(name)?;
        let mut outs: Vec<OutSlot> = Vec::new();
        // DISPPARAMS takes arguments in reverse order.
        let mut vars: Vec<VARIANT> = Vec::with_capacity(args.len());
        for a in args.iter().rev() {
            let v = unsafe {
                match a {
                    Arg::Bool(b) => variant(VT_BOOL, |u| u.boolVal = vbool(*b)),
                    Arg::Int(i) => variant(VT_I4, |u| u.lVal = *i),
                    Arg::Float(x) => variant(VT_R8, |u| u.dblVal = *x),
                    Arg::Str(s) => {
                        variant(VT_BSTR, |u| u.bstrVal = ManuallyDrop::new(BSTR::from(s.as_str())))
                    }
                    Arg::Disp(d) => variant(VT_DISPATCH, |u| {
                        u.pdispVal = ManuallyDrop::new(d.as_ref().map(|d| d.obj.clone()))
                    }),
                    Arg::Empty => VariantInit(),
                    Arg::OutInt => {
                        let mut b = Box::new(0i32);
                        let p: *mut i32 = &mut *b;
                        outs.push(OutSlot::Int(b));
                        variant(VARENUM(VT_I4.0 | VT_BYREF.0), |u| u.plVal = p)
                    }
                    Arg::OutStr => {
                        let mut b = Box::new(BSTR::new());
                        let p: *mut BSTR = &mut *b;
                        outs.push(OutSlot::Str(b));
                        variant(VARENUM(VT_BSTR.0 | VT_BYREF.0), |u| u.pbstrVal = p)
                    }
                    Arg::OutBool => {
                        let mut b = Box::new(VARIANT_FALSE);
                        let p: *mut VARIANT_BOOL = &mut *b;
                        outs.push(OutSlot::Bool(b));
                        variant(VARENUM(VT_BOOL.0 | VT_BYREF.0), |u| u.pboolVal = p)
                    }
                }
            };
            vars.push(v);
        }
        // Output slots were pushed in reverse argument order.
        outs.reverse();

        let mut named = DISPID_PROPERTYPUT;
        let params = DISPPARAMS {
            rgvarg: if vars.is_empty() { std::ptr::null_mut() } else { vars.as_mut_ptr() },
            rgdispidNamedArgs: if flags == DISPATCH_PROPERTYPUT { &mut named } else { std::ptr::null_mut() },
            cArgs: vars.len() as u32,
            cNamedArgs: if flags == DISPATCH_PROPERTYPUT { 1 } else { 0 },
        };
        let mut result = unsafe { VariantInit() };
        let mut excep = EXCEPINFO::default();
        let mut argerr = 0u32;
        let r = unsafe {
            self.obj.Invoke(
                id,
                &GUID::zeroed(),
                LOCALE_USER_DEFAULT,
                flags,
                &params,
                Some(&mut result),
                Some(&mut excep),
                Some(&mut argerr),
            )
        };
        for v in &mut vars {
            // Byref variants point at our boxes; clearing them is a no-op for the target.
            unsafe {
                let _ = VariantClear(v);
            }
        }
        if let Err(e) = r {
            let desc = excep.bstrDescription.to_string();
            return Err(ComError(format!(
                "{name}: {}{}",
                e.message(),
                if desc.is_empty() { String::new() } else { format!(" ({desc})") }
            )));
        }
        let value = unsafe { from_variant(&result) };
        unsafe {
            let _ = VariantClear(&mut result);
        }
        let outs = outs
            .into_iter()
            .map(|o| match o {
                OutSlot::Int(b) => Value::Int(*b as i64),
                OutSlot::Str(b) => Value::Str(b.to_string()),
                OutSlot::Bool(b) => Value::Bool(*b != VARIANT_FALSE),
            })
            .collect();
        Ok((value, outs))
    }

    /// Calls a method or reads a (possibly indexed) property.
    pub fn call(&self, name: &str, args: &[Arg]) -> ComResult<Value> {
        Ok(self.invoke(name, DISPATCH_FLAGS(DISPATCH_METHOD.0 | DISPATCH_PROPERTYGET.0), args)?.0)
    }

    /// Like [`call`](Self::call), also returning output parameters in order.
    pub fn call_out(&self, name: &str, args: &[Arg]) -> ComResult<(Value, Vec<Value>)> {
        self.invoke(name, DISPATCH_FLAGS(DISPATCH_METHOD.0 | DISPATCH_PROPERTYGET.0), args)
    }

    pub fn put(&self, name: &str, value: Arg) -> ComResult<()> {
        self.invoke(name, DISPATCH_PROPERTYPUT, &[value]).map(|_| ())
    }

    /// Calls and expects an object back.
    pub fn obj(&self, name: &str, args: &[Arg]) -> ComResult<Disp> {
        match self.call(name, args)? {
            Value::Disp(d) => Ok(d),
            other => Err(ComError(format!("{name}: expected an object, got {other:?}"))),
        }
    }

    /// Calls and returns an object, or None when the result is Nothing.
    pub fn opt_obj(&self, name: &str, args: &[Arg]) -> ComResult<Option<Disp>> {
        Ok(self.call(name, args)?.disp().cloned())
    }

    pub fn f64(&self, name: &str, args: &[Arg]) -> ComResult<f64> {
        let v = self.call(name, args)?;
        v.f64().ok_or_else(|| ComError(format!("{name}: expected a number, got {v:?}")))
    }

    pub fn i64(&self, name: &str, args: &[Arg]) -> ComResult<i64> {
        let v = self.call(name, args)?;
        v.i64().ok_or_else(|| ComError(format!("{name}: expected an integer, got {v:?}")))
    }

    pub fn bool(&self, name: &str, args: &[Arg]) -> ComResult<bool> {
        let v = self.call(name, args)?;
        v.bool().ok_or_else(|| ComError(format!("{name}: expected a boolean, got {v:?}")))
    }

    pub fn string(&self, name: &str, args: &[Arg]) -> ComResult<String> {
        match self.call(name, args)? {
            Value::Str(s) => Ok(s),
            Value::Empty => Ok(String::new()),
            other => Err(ComError(format!("{name}: expected a string, got {other:?}"))),
        }
    }
}

unsafe fn from_variant(v: &VARIANT) -> Value {
    unsafe {
        let inner = &v.Anonymous.Anonymous;
        let vt = inner.vt.0;
        let u = &inner.Anonymous;
        if vt & VT_ARRAY.0 != 0 {
            return from_safearray(u.parray, VARENUM(vt & !VT_ARRAY.0));
        }
        if vt & VT_BYREF.0 != 0 {
            if vt == VT_VARIANT.0 | VT_BYREF.0 && !u.pvarVal.is_null() {
                return from_variant(&*u.pvarVal);
            }
            return Value::Empty;
        }
        match VARENUM(vt) {
            VT_EMPTY | VT_NULL => Value::Empty,
            VT_BOOL => Value::Bool(u.boolVal != VARIANT_FALSE),
            VT_I1 | VT_UI1 => Value::Int(u.bVal as i64),
            VT_I2 => Value::Int(u.iVal as i64),
            VT_UI2 => Value::Int(u.uiVal as i64),
            VT_I4 | VT_INT => Value::Int(u.lVal as i64),
            VT_UI4 | VT_UINT => Value::Int(u.ulVal as i64),
            VT_I8 => Value::Int(u.llVal),
            VT_R4 => Value::Float(u.fltVal as f64),
            VT_R8 => Value::Float(u.dblVal),
            VT_BSTR => Value::Str(u.bstrVal.to_string()),
            VT_DISPATCH => match &*u.pdispVal {
                Some(d) => Value::Disp(Disp::new(d.clone())),
                None => Value::Empty,
            },
            VT_UNKNOWN => match &*u.punkVal {
                Some(k) => k.cast::<IDispatch>().map(|d| Value::Disp(Disp::new(d))).unwrap_or(Value::Empty),
                None => Value::Empty,
            },
            _ => Value::Empty,
        }
    }
}

unsafe fn from_safearray(sa: *mut SAFEARRAY, elem: VARENUM) -> Value {
    if sa.is_null() {
        return Value::Empty;
    }
    unsafe {
        if SafeArrayGetDim(sa) != 1 {
            return Value::Empty;
        }
        let (Ok(lo), Ok(hi)) = (SafeArrayGetLBound(sa, 1), SafeArrayGetUBound(sa, 1)) else {
            return Value::Empty;
        };
        let n = (hi - lo + 1).max(0) as usize;
        let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
        if SafeArrayAccessData(sa, &mut data).is_err() {
            return Value::Empty;
        }
        let out: Vec<Value> = (0..n)
            .map(|i| match elem {
                VT_R8 => Value::Float(*(data as *const f64).add(i)),
                VT_R4 => Value::Float(*(data as *const f32).add(i) as f64),
                VT_I4 | VT_INT => Value::Int(*(data as *const i32).add(i) as i64),
                VT_UI4 | VT_UINT => Value::Int(*(data as *const u32).add(i) as i64),
                VT_I2 => Value::Int(*(data as *const i16).add(i) as i64),
                VT_UI1 => Value::Int(*(data as *const u8).add(i) as i64),
                VT_BOOL => Value::Bool(*(data as *const VARIANT_BOOL).add(i) != VARIANT_FALSE),
                VT_BSTR => Value::Str((*(data as *const BSTR).add(i)).to_string()),
                VT_VARIANT => from_variant(&*(data as *const VARIANT).add(i)),
                VT_DISPATCH | VT_UNKNOWN => {
                    let raw = *(data as *const *mut core::ffi::c_void).add(i);
                    match windows::core::IUnknown::from_raw_borrowed(&raw) {
                        Some(k) => {
                            k.cast::<IDispatch>().map(|d| Value::Disp(Disp::new(d))).unwrap_or(Value::Empty)
                        }
                        None => Value::Empty,
                    }
                }
                _ => Value::Empty,
            })
            .collect();
        let _ = SafeArrayUnaccessData(sa);
        Value::Array(out)
    }
}

/// Attaches to a running instance of `progid`, or starts one. Returns the object and
/// whether we started it.
pub fn connect(progid: &str) -> ComResult<(Disp, bool)> {
    unsafe {
        let clsid = CLSIDFromProgID(&HSTRING::from(progid))
            .map_err(|e| ComError(format!("'{progid}' is not registered: {}", e.message())))?;
        let mut unk: Option<windows::core::IUnknown> = None;
        if GetActiveObject(&clsid, None, &mut unk).is_ok() {
            if let Some(u) = unk {
                return Ok((Disp::from_unknown(&u)?, false));
            }
        }
        let d: IDispatch = CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER)?;
        Ok((Disp::new(d), true))
    }
}
