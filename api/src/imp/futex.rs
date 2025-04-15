use alloc::sync::Arc;
use axerrno::{LinuxError, LinuxResult};
use axtask::{TaskExtRef, WaitQueue, current};
use linux_raw_sys::general::{
    FUTEX_CMD_MASK, FUTEX_CMP_REQUEUE, FUTEX_REQUEUE, FUTEX_WAIT, FUTEX_WAKE, timespec,
};

use crate::{
    ptr::{UserConstPtr, UserPtr, nullable},
    time::timespec_to_timevalue,
};

fn new_futex() -> Arc<WaitQueue> {
    Arc::new(WaitQueue::new())
}

pub fn sys_futex(
    uaddr: UserConstPtr<u32>,
    futex_op: u32,
    value: u32,
    timeout: UserConstPtr<timespec>,
    uaddr2: UserPtr<u32>,
    value3: u32,
) -> LinuxResult<isize> {
    info!("futex {:?} {} {}", uaddr.address(), futex_op, value);
    let curr = current();
    let futex_table = &curr.task_ext().process_data().futex_table;

    let addr = uaddr.address().as_usize();
    let command = futex_op & (FUTEX_CMD_MASK as u32);
    match command {
        FUTEX_WAIT => {
            if *uaddr.get_as_ref()? != value {
                return Err(LinuxError::EAGAIN);
            }
            let wq = futex_table
                .lock()
                .entry(addr)
                .or_insert_with(new_futex)
                .clone();

            if let Some(timeout) = nullable!(timeout.get_as_ref())? {
                wq.wait_timeout(timespec_to_timevalue(*timeout));
            } else {
                wq.wait();
            }

            Ok(0)
        }
        FUTEX_WAKE => {
            let wq = futex_table.lock().get(&addr).cloned();
            let mut count = 0;
            if let Some(wq) = wq {
                for _ in 0..value {
                    if !wq.notify_one(false) {
                        break;
                    }
                    count += 1;
                }
            }
            axtask::yield_now();
            Ok(count)
        }
        FUTEX_REQUEUE | FUTEX_CMP_REQUEUE => {
            if command == FUTEX_CMP_REQUEUE && *uaddr.get_as_ref()? != value3 {
                return Err(LinuxError::EAGAIN);
            }
            let value2 = timeout.address().as_usize() as u32;

            let mut futex_table = futex_table.lock();
            let wq = futex_table.get(&addr).cloned();
            let wq2 = futex_table
                .entry(uaddr2.address().as_usize())
                .or_insert_with(new_futex)
                .clone();
            drop(futex_table);

            let mut count = 0;
            if let Some(wq) = wq {
                for _ in 0..value {
                    if !wq.notify_one(false) {
                        break;
                    }
                    count += 1;
                }
                count += wq.requeue(value2 as usize, &wq2) as isize;
            }
            Ok(count)
        }
        _ => Err(LinuxError::ENOSYS),
    }
}
