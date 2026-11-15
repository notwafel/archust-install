//! Partition arithmetic. Pure: no disk, no root, no shell. `--check` runs it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Esp,
    Swap,
    Root,
}

#[derive(Debug, Clone, Copy)]
pub struct Part {
    pub role: Role,
    pub start: u64, // MiB, inclusive
    pub end: u64,   // MiB, inclusive
}

impl Part {
    pub fn size(&self) -> u64 { self.end - self.start + 1 }
}

#[derive(Debug, Clone, Copy)]
pub struct Request {
    pub total_mib: u64,
    pub boot_mib: u64,
    pub swap_mib: u64,
    pub root_mib: u64,
    pub dualboot: bool,
    pub free_start: u64, // MiB, dualboot only
}

/// Unpartitioned room after the last existing partition. Dualboot plans live here, not in
/// `total_mib`: an existing system may fill the disk almost completely and still have room.
pub fn free_mib(r: &Request) -> u64 {
    // the last usable MiB is total - 2, the backup GPT needs the one after it
    r.total_mib.saturating_sub(r.free_start).saturating_sub(1)
}

/// `None` when the request does not fit. An existing ESP (dualboot) is not in the result.
pub fn plan(r: Request) -> Option<Vec<Part>> {
    let space = space_mib(&r);
    if space < 16384 {
        return None;
    }
    let esp_exists = r.dualboot && r.free_start > 1;
    if !esp_exists && r.boot_mib < 300 {
        return None;
    }
    let mut parts = Vec::with_capacity(3);
    let mut s = if r.dualboot { r.free_start } else { 1 };
    if !esp_exists {
        let e = s + r.boot_mib - 1;
        parts.push(Part { role: Role::Esp, start: s, end: e });
        s = e + 1;
    }
    if r.swap_mib > 0 {
        let e = s + r.swap_mib - 1;
        parts.push(Part { role: Role::Swap, start: s, end: e });
        s = e + 1;
    }
    let e = if r.root_mib > 0 { s + r.root_mib - 1 } else { r.total_mib - 2 };
    if e >= r.total_mib - 1 || e < s {
        return None;
    }
    parts.push(Part { role: Role::Root, start: s, end: e });
    Some(parts)
}

/// Usable space for the plan, in MiB.
pub fn space_mib(r: &Request) -> u64 {
    if r.dualboot { free_mib(r) } else { r.total_mib }
}

/// True when parts are ordered, non-overlapping, inside `total` and end with root.
fn valid(parts: &[Part], total: u64) -> bool {
    parts.last().map(|p| p.role) == Some(Role::Root)
        && parts.windows(2).all(|w| w[0].end < w[1].start)
        && parts.last().is_some_and(|p| p.end < total)
}


pub fn self_check() -> bool {
    let cases = [
        (102400u64, 0u64, 0u64, false, 1u64),
        (204800, 16, 0, false, 1),
        (204800, 8, 64, false, 1),
        (32768, 0, 0, false, 1),
        (204800, 0, 0, true, 102401),
        (204800, 16, 0, true, 102401),
    ];
    let mut ok = true;
    for (total, swap, root, dual, free) in cases {
        let r = Request {
            total_mib: total,
            boot_mib: if dual { 0 } else { 1024 },
            swap_mib: swap * 1024,
            root_mib: root * 1024,
            dualboot: dual,
            free_start: free,
        };
        match plan(r) {
            Some(p) if valid(&p, total) => {
                let last = p.last().unwrap();
                println!("OK  {:<6} root={:<4} swap={:<3} dual={:<5} -> root {}MiB", total, root, swap, dual, last.size());
            }
            _ => {
                println!("BAD {:<6} root={:<4} swap={:<3} dual={}", total, root, swap, dual);
                ok = false;
            }
        }
    }
    let rejected = [
        Request { total_mib: 204800, boot_mib: 1024, swap_mib: 0, root_mib: 300 * 1024, dualboot: false, free_start: 1 },
        Request { total_mib: 8192, boot_mib: 1024, swap_mib: 0, root_mib: 0, dualboot: false, free_start: 1 },
        Request { total_mib: 204800, boot_mib: 256, swap_mib: 0, root_mib: 0, dualboot: false, free_start: 1 },
        // dualboot, but the existing system fills the whole disk: nothing left to install into
        Request { total_mib: 204800, boot_mib: 0, swap_mib: 0, root_mib: 0, dualboot: true, free_start: 204800 },
        // dualboot with only 8 GiB of free space
        Request { total_mib: 204800, boot_mib: 0, swap_mib: 0, root_mib: 0, dualboot: true, free_start: 196608 },
    ];
    for r in rejected {
        if plan(r).is_some() {
            println!("BAD accepted {:?}", r);
            ok = false;
        }
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::{plan, Request};

    fn last_end(total_mib: u64, boot_mib: u64, swap_mib: u64, free_start: u64, dual: bool) -> u64 {
        let parts = plan(Request { total_mib, boot_mib, swap_mib, root_mib: 0, dualboot: dual, free_start })
            .expect("plan fits");
        parts.last().unwrap().end
    }

    #[test]
    fn root_leaves_a_mebibyte_for_the_backup_gpt() {
        // disk::create passes end + 1 to parted, and parted rejects a location at the device size
        for total in [81920, 204800] {
            assert!(last_end(total, 1024, 0, 1, false) + 1 < total);
            assert!(last_end(total, 1024, 16384, 1, false) + 1 < total);
            assert!(last_end(total, 0, 0, total / 2, true) + 1 < total);
        }
    }
}
