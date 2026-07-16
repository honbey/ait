use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

static SIGNAL_THREAD_NAME: &str = "diagnostic-sigusr1";

pub fn install_signal_handler() {
    block_signal();
    spawn_handler_thread();
}

fn block_signal() {
    unsafe {
        let mut set = std::mem::zeroed::<libc::sigset_t>();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGUSR1);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

fn spawn_handler_thread() {
    std::thread::Builder::new()
        .name(SIGNAL_THREAD_NAME.into())
        .spawn(move || {
            // SIGUSR1 is already blocked on this thread (inherited from parent)
            // so sigwait can catch it
            unsafe {
                let mut set = std::mem::zeroed::<libc::sigset_t>();
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, libc::SIGUSR1);

                loop {
                    let mut sig: libc::c_int = 0;
                    let ret = libc::sigwait(&set, &mut sig);
                    if ret != 0 {
                        break;
                    }
                    if sig == libc::SIGUSR1 {
                        dump_thread_stacks();
                    }
                }
            }
        })
        .expect("failed to spawn diagnostic-sigusr1 thread");
}

fn dump_thread_stacks() {
    let task_dir = PathBuf::from("/proc/self/task");

    let entries = match fs::read_dir(&task_dir) {
        Ok(e) => e,
        Err(_) => {
            let _ = writeln!(
                io::stderr().lock(),
                "[SIGNAL] Failed to read /proc/self/task/"
            );
            return;
        }
    };

    let mut tids: Vec<u64> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if let Ok(tid_str) = name.into_string()
            && let Ok(tid) = tid_str.parse::<u64>()
        {
            tids.push(tid);
        }
    }
    tids.sort();

    let mut stderr = io::stderr().lock();

    let _ = writeln!(
        stderr,
        "[SIGNAL] ========== Thread Stack Dump (SIGUSR1, {} threads) ==========",
        tids.len()
    );

    for tid in &tids {
        let stack_path = task_dir.join(tid.to_string()).join("stack");
        match fs::read_to_string(&stack_path) {
            Ok(content) => {
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    let _ = writeln!(stderr, "[SIGNAL] TID {}: (no kernel stack / running)", tid);
                } else {
                    let _ = writeln!(stderr, "[SIGNAL] TID {}:", tid);
                    for line in trimmed.lines() {
                        let _ = writeln!(stderr, "[SIGNAL]   {}", line);
                    }
                }
            }
            Err(e) => {
                let _ = writeln!(stderr, "[SIGNAL] TID {}: failed to read stack: {}", tid, e);
            }
        }
    }

    let _ = writeln!(
        stderr,
        "[SIGNAL] ========== End Thread Stack Dump =========="
    );
}
