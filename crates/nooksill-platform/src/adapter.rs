//! 网络适配器类型检测。
//!
//! 控制中心的网络磁贴要区分 WiFi / Ethernet，`control::tiles_ordered`
//! 早就有 `wired` 参数，但调用方一直硬传 `false` —— 插着网线也画 WiFi。
//! 这里用 `GetAdaptersAddresses` 找一块**处于 Up 的以太网接口**补上。
//!
//! 不枚举无线网卡、不看 IP 是否拿到地址：磁贴表达的是"物理连接方式"，
//! 网线插着但 DHCP 还没给地址时仍该显示 Ethernet。

use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, IF_TYPE_ETHERNET_CSMACD, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::AF_UNSPEC;

/// 有没有已连接的有线网卡。
///
/// **任何失败都返回 `false`**（= 显示 WiFi 磁贴），与改动前的硬编码
/// 行为一致 —— 检测不可用时不会引入回归，只是维持原状。
#[must_use]
pub fn wired_connected() -> bool {
    let mut size = 0_u32;
    // 第一次只问大小。传空 buffer 时系统返回 ERROR_BUFFER_OVERFLOW
    // 并把所需字节数写进 `size`，这是这个 API 的标准两趟用法。
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

    // 结构体含 u64 字段，需要 8 字节对齐；`Vec<u8>` 只保证 1 字节。
    // 用 u64 数组当后备存储，再 cast 成结构体指针。
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
    // SAFETY: `buf` 在本次调用期间有效；系统按 `size` 字节写入，`Next`
    // 指向同一块 buffer 内的下一个结构（MSDN 保证）。
    unsafe { any_wired_up(buf.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()) }
}

/// 遍历 adapter 链表，找一块处于 Up 的以太网接口。
///
/// 抽成独立函数是为了能用假链表单测，不必依赖本机网卡状态。
///
/// # Safety
/// `head` 要么为 null，要么指向一条有效的、以 null `Next` 结尾的
/// `IP_ADAPTER_ADDRESSES_LH` 链表。
unsafe fn any_wired_up(head: *const IP_ADAPTER_ADDRESSES_LH) -> bool {
    let mut cur = head;
    // 上限防止系统给了坏链表时死循环。256 块网卡远超任何真实机器。
    for _ in 0..256 {
        if cur.is_null() {
            return false;
        }
        // SAFETY: 调用方保证链表节点有效。
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

    /// `IF_TYPE_IEEE80211`（IEEE 802.11 无线）。
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

    /// 真机冒烟：只要求不崩，`wired` 结果取决于本机有没有插网线。
    #[test]
    #[ignore = "依赖本机网卡状态，手动运行"]
    fn real_query_does_not_panic() {
        let _ = wired_connected();
    }
}
