use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use once_cell::sync::Lazy;
use sendrs_chat::{ChatDirection, ChatStore};
use sendrs_core::{NetworkMode, PeerInfo, TransferTask};
use sendrs_discovery::{discover_peers, DiscoveryBeacon};
use sendrs_security::{load_or_create_identity, pair_peer as pair_peer_secure};
use sendrs_transfer::{build_manifest, total_size, DEFAULT_CHUNK_SIZE};

#[derive(Default)]
struct RuntimeState {
    peers: Vec<PeerInfo>,
    tasks: Vec<TransferTask>,
    last_error: Option<String>,
}

static STATE: Lazy<Mutex<RuntimeState>> = Lazy::new(|| Mutex::new(RuntimeState::default()));

#[no_mangle]
pub extern "C" fn start_discovery() -> i32 {
    with_result(|| {
        let identity = load_or_create_identity(None)?;
        let beacon = DiscoveryBeacon::from_identity(&identity.identity, 38080, false);
        let peers = discover_peers(&beacon, Duration::from_secs(2))?;

        let mut guard = STATE.lock().expect("state lock");
        guard.peers = peers
            .into_iter()
            .filter(|peer| peer.peer_id != identity.identity.device_id)
            .collect();
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn list_peers() -> *mut c_char {
    with_json(|| {
        let guard = STATE.lock().expect("state lock");
        Ok(guard.peers.clone())
    })
}

#[no_mangle]
pub extern "C" fn pair_peer(peer_id: *const c_char, code: *const c_char) -> i32 {
    with_result(|| {
        let peer_id = read_cstr(peer_id)?;
        let code = read_cstr(code)?;
        pair_peer_secure(&peer_id, &code)?;
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn send_path(
    peer_id: *const c_char,
    path: *const c_char,
    public_mode: i32,
) -> *mut c_char {
    with_json(|| {
        let peer_id = read_cstr(peer_id)?;
        let path = read_cstr(path)?;
        let manifest = build_manifest(Path::new(&path), DEFAULT_CHUNK_SIZE)?;
        let task = TransferTask::new_send(
            peer_id,
            path,
            if public_mode == 0 {
                NetworkMode::Lan
            } else {
                NetworkMode::Public
            },
            total_size(&manifest),
        );

        let mut guard = STATE.lock().expect("state lock");
        guard.tasks.push(task.clone());
        Ok(task)
    })
}

#[no_mangle]
pub extern "C" fn accept_transfer(request_id: *const c_char, target: *const c_char) -> i32 {
    with_result(|| {
        let request_id = read_cstr(request_id)?;
        let target = read_cstr(target)?;
        let mut guard = STATE.lock().expect("state lock");
        let Some(task) = guard
            .tasks
            .iter_mut()
            .find(|task| task.task_id == request_id)
        else {
            return Err(anyhow::anyhow!("task not found"));
        };
        task.accept_receive(target);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn list_tasks() -> *mut c_char {
    with_json(|| {
        let guard = STATE.lock().expect("state lock");
        Ok(guard.tasks.clone())
    })
}

#[no_mangle]
pub extern "C" fn send_chat(peer_id: *const c_char, message: *const c_char) -> i32 {
    with_result(|| {
        let peer_id = read_cstr(peer_id)?;
        let message = read_cstr(message)?;
        let store = ChatStore::open_default()?;
        store.append_message(&peer_id, ChatDirection::Outgoing, &message)?;
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn list_chat_messages(peer_id: *const c_char) -> *mut c_char {
    with_json(|| {
        let peer_id = read_cstr(peer_id)?;
        let store = ChatStore::open_default()?;
        let messages = store.list_messages(&peer_id, 200)?;
        Ok(messages)
    })
}

#[no_mangle]
pub extern "C" fn last_error_message() -> *mut c_char {
    with_json(|| {
        let guard = STATE.lock().expect("state lock");
        Ok(guard.last_error.clone().unwrap_or_default())
    })
}

#[no_mangle]
pub extern "C" fn free_c_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

fn read_cstr(ptr: *const c_char) -> anyhow::Result<String> {
    if ptr.is_null() {
        return Err(anyhow::anyhow!("received null pointer"));
    }
    let s = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| anyhow::anyhow!("invalid UTF-8 input"))?
        .to_string();
    Ok(s)
}

fn with_result<F>(f: F) -> i32
where
    F: FnOnce() -> anyhow::Result<()>,
{
    match f() {
        Ok(()) => {
            let mut guard = STATE.lock().expect("state lock");
            guard.last_error = None;
            0
        }
        Err(err) => {
            let mut guard = STATE.lock().expect("state lock");
            guard.last_error = Some(err.to_string());
            -1
        }
    }
}

fn with_json<T, F>(f: F) -> *mut c_char
where
    T: serde::Serialize,
    F: FnOnce() -> anyhow::Result<T>,
{
    match f()
        .and_then(|value| serde_json::to_string(&value).map_err(anyhow::Error::from))
        .and_then(|text| CString::new(text).map_err(anyhow::Error::from))
    {
        Ok(cstring) => {
            let mut guard = STATE.lock().expect("state lock");
            guard.last_error = None;
            cstring.into_raw()
        }
        Err(err) => {
            let mut guard = STATE.lock().expect("state lock");
            guard.last_error = Some(err.to_string());
            std::ptr::null_mut()
        }
    }
}
