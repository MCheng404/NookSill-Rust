//! Network adapter type detection.
//!
//! Reports the physical medium for the network tile, so an ethernet adapter
//! counts as wired even before DHCP hands out an address.

use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, IF_TYPE_ETHERNET_CSMACD, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::AF_UNSPEC;

/// Whether a wired adapter is connected.
///
/// Any failure returns `false`, which keeps the old Wi-Fi-tile behavior.
#[must_use]
pub fn wired_connected() -> bool {
    let mut size = 0_u32;
    // First call only asks for the size: a null buffer returns
    // ERROR_BUFFER_OVERFLOW with the byte count written to `size`.
    let first = unsafe {
        GetAdaptersAddresses(
            u32::from(AF_UNSPEC.0),
            Default::default(),
            None,
            None,
            &raw mut size,
        )
    };
    if first != ERROR_BUFFER_OVERFLOW.0 || size == 0 {
        return false;
    }

    // The structs contain u64 fields and need 8-byte alignment, while `Vec<u8>`
    // guarantees one. Back the buffer with u64 words and cast the pointer.
    let mut buf = vec![0_u64; (size as usize).div_ceil(8)];
    let ret = unsafe {
        GetAdaptersAddresses(
            u32::from(AF_UNSPEC.0),
            Default::default(),
            None,
            Some(buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()),
            &raw mut size,
        )
    };
    if ret != NO_ERROR.0 {
        return false;
    }
    // SAFETY: `buf` outlives the call; the system writes `size` bytes and every
    // `Next` points inside the same buffer.
    unsafe { any_wired_up(buf.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()) }
}

/// Walk the adapter list for an ethernet interface that is Up.
///
/// Split out so tests can feed a fake list instead of relying on host NICs.
///
/// # Safety
/// `head` is null or points at a valid `IP_ADAPTER_ADDRESSES_LH` list
/// terminated by a null `Next`.
unsafe fn any_wired_up(head: *const IP_ADAPTER_ADDRESSES_LH) -> bool {
    let mut cur = head;
    // Cap the walk in case the system returns a cyclic list; 256 adapters is
    // far beyond any real machine.
    for _ in 0..256 {
        if cur.is_null() {
            return false;
        }
        // SAFETY: the caller guarantees every node is valid.
        let a = unsafe { &*cur };
        if a.IfType == IF_TYPE_ETHERNET_CSMACD && a.OperStatus == IfOperStatusUp {
            return true;
        }
        cur = a.Next;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::NetworkManagement::Ndis::{IfOperStatusDown, IF_OPER_STATUS};

    /// `IF_TYPE_IEEE80211` (IEEE 802.11 wireless).
    const WIFI: u32 = 71;

    fn node(
        if_type: u32,
        status: IF_OPER_STATUS,
        next: *mut IP_ADAPTER_ADDRESSES_LH,
    ) -> Box<IP_ADAPTER_ADDRESSES_LH> {
        let mut n = Box::new(IP_ADAPTER_ADDRESSES_LH::default());
        n.IfType = if_type;
        n.OperStatus = status;
        n.Next = next;
        n
    }

    #[test]
    fn an_empty_list_is_not_wired() {
        assert!(!unsafe { any_wired_up(std::ptr::null()) });
    }

    #[test]
    fn wifi_only_is_not_wired() {
        let n = node(WIFI, IfOperStatusUp, std::ptr::null_mut());
        assert!(!unsafe { any_wired_up(&raw const *n) });
    }

    #[test]
    fn a_connected_ethernet_adapter_counts_even_if_it_is_not_first() {
        let mut wifi = node(WIFI, IfOperStatusUp, std::ptr::null_mut());
        let next = &raw mut *wifi as *mut IP_ADAPTER_ADDRESSES_LH;
        let eth = node(IF_TYPE_ETHERNET_CSMACD, IfOperStatusUp, next);
        assert!(unsafe { any_wired_up(&raw const *eth) });
        drop(wifi);
    }

    #[test]
    fn a_down_ethernet_adapter_does_not_count() {
        let n = node(
            IF_TYPE_ETHERNET_CSMACD,
            IfOperStatusDown,
            std::ptr::null_mut(),
        );
        assert!(!unsafe { any_wired_up(&raw const *n) });
    }

    /// Hardware smoke test: only asserts no panic, the result depends on
    /// whether a cable is plugged in.
    #[test]
    #[ignore = "依赖本机网卡状态，手动运行"]
    fn real_query_does_not_panic() {
        let _ = wired_connected();
    }
}
