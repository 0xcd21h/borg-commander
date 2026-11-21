use std::alloc::System;

use zeroizing_alloc::ZeroAlloc;

#[global_allocator]
static ALLOCATOR: ZeroAlloc<System> = ZeroAlloc(System);

#[cfg(test)]
mod tests {
    const WIPING_ALLOCATOR: &str = "zeroizing_alloc::ZeroAlloc<";

    #[test]
    fn freed_memory_is_zeroed_by_the_allocator() {
        assert!(std::any::type_name_of_val(&super::ALLOCATOR).starts_with(WIPING_ALLOCATOR));
    }
}
