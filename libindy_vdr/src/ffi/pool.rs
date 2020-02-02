use crate::common::error::prelude::*;
use crate::pool::{PoolFactory, PoolRunner, RequestResult};

use std::collections::BTreeMap;
use std::os::raw::c_char;
use std::sync::RwLock;

use ffi_support::{rust_string_to_c, FfiStr};

use super::error::{set_last_error, ErrorCode};
use super::requests::{RequestHandle, REQUESTS};
use super::POOL_CONFIG;

new_handle_type!(PoolHandle, FFI_PH_COUNTER);

lazy_static! {
    pub static ref POOLS: RwLock<BTreeMap<PoolHandle, PoolRunner>> = RwLock::new(BTreeMap::new());
}

#[no_mangle]
pub extern "C" fn indy_vdr_pool_create_from_genesis_file(
    path: FfiStr,
    handle_p: *mut usize,
) -> ErrorCode {
    catch_err! {
        trace!("Create pool from genesis file");
        check_useful_c_ptr!(handle_p);
        let mut factory = PoolFactory::from_genesis_file(path.as_str())?;
        {
            let gcfg = read_lock!(POOL_CONFIG)?;
            factory.set_config(*gcfg)?;
        }
        let pool = factory.create_runner()?;
        let handle = PoolHandle::next();
        let mut pools = write_lock!(POOLS)?;
        pools.insert(handle, pool);
        unsafe {
            *handle_p = *handle;
        }
        Ok(ErrorCode::Success)
    }
}

#[no_mangle]
pub extern "C" fn indy_vdr_pool_get_transactions(
    pool_handle: usize,
    cb: Option<extern "C" fn(err: ErrorCode, response: *const c_char)>,
) -> ErrorCode {
    catch_err! {
        trace!("Get pool transactions");
        let cb = cb.ok_or_else(|| input_err("No callback provided"))?;
        let pools = read_lock!(POOLS)?;
        let pool = pools.get(&PoolHandle(pool_handle))
            .ok_or_else(|| input_err("Unknown pool handle"))?;
        pool.get_transactions(Box::new(
            move |result| {
                let (errcode, reply) = match result {
                    Ok(txns) => {
                        (ErrorCode::Success, txns.join("\n"))
                    },
                    Err(err) => {
                        let code = ErrorCode::from(&err);
                        set_last_error(Some(err));
                        (code, String::new())
                    }
                };
                cb(errcode, rust_string_to_c(reply))
            }))?;

        Ok(ErrorCode::Success)
    }
}

#[no_mangle]
pub extern "C" fn indy_vdr_pool_submit_request(
    pool_handle: usize,
    request_handle: usize,
    cb: Option<extern "C" fn(err: ErrorCode, response: *const c_char)>,
) -> ErrorCode {
    catch_err! {
        trace!("Submit request: {} {}", pool_handle, request_handle);
        let cb = cb.ok_or_else(|| input_err("No callback provided"))?;
        let pools = read_lock!(POOLS)?;
        let pool = pools.get(&PoolHandle(pool_handle))
            .ok_or_else(|| input_err("Unknown pool handle"))?;
        let req = {
            let mut reqs = write_lock!(REQUESTS)?;
            reqs.remove(&RequestHandle(request_handle))
                .ok_or_else(|| input_err("Unknown request handle"))?
        };
        pool.send_request(req, Box::new(
            move |result| {
                let (errcode, reply) = match result {
                    Ok((reply, _timing)) => {
                        match reply {
                            RequestResult::Reply(body) => {
                                (ErrorCode::Success, body)
                            }
                            RequestResult::Failed(err) => {
                                let code = ErrorCode::from(&err);
                                set_last_error(Some(err));
                                (code, String::new())
                            }
                        }
                    },
                    Err(err) => {
                        let code = ErrorCode::from(&err);
                        set_last_error(Some(err));
                        (code, String::new())
                    }
                };
                cb(errcode, rust_string_to_c(reply))
            }))?;
        Ok(ErrorCode::Success)
    }
}

// NOTE: at the moment, pending requests are allowed to complete
// and request callbacks are still run, even if we no longer have a
// reference to the pool here. Maybe an optional callback for when
// the close has completed?
#[no_mangle]
pub extern "C" fn indy_vdr_pool_close(pool_handle: usize) -> ErrorCode {
    catch_err! {
        let mut pools = write_lock!(POOLS)?;
        pools.remove(&PoolHandle(pool_handle))
            .ok_or_else(|| input_err("Unknown pool handle"))?;
        Ok(ErrorCode::Success)
    }
}
